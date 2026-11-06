//! Tool behaviour through the MCP dispatch layer, against a real core.
//!
//! These are the properties a client depends on: the same command path as the UI, secrets that
//! never come back as text, `remove_app` refusing to orphan a running stack, and ports that are
//! reported as confirmed only when a socket really listens.

use neohub_core::service::CoreService;
use neohub_mcp::dispatch::{ToolContext, dispatch};
use neohub_mcp::redaction::Redactor;
use neohub_mcp::tools::ToolName;
use neohub_storage::DataDir;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-fixtures/process")
        .join(name)
}

struct Harness {
    _dir: tempfile::TempDir,
    service: Arc<CoreService>,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let service = CoreService::open(DataDir::new(dir.path())).expect("core");
        Harness { _dir: dir, service }
    }

    fn context(&self) -> ToolContext {
        ToolContext {
            handle: self.service.handle(),
            redactor: Redactor::new(),
        }
    }

    fn project(&self, name: &str) -> String {
        let project = self._dir.path().join(name);
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("package.json"),
            r#"{"name":"demo","scripts":{"dev":"node ."}}"#,
        )
        .unwrap();
        project.to_string_lossy().into_owned()
    }
}

async fn call(ctx: &ToolContext, tool: ToolName, args: Value) -> Value {
    dispatch(ctx, tool, &args)
        .await
        .unwrap_or_else(|err| err.to_json())
}

#[tokio::test]
async fn add_app_then_list_and_get_it_through_the_tool_layer() {
    let harness = Harness::new();
    let ctx = harness.context();
    let path = harness.project("demo");

    let created = call(
        &ctx,
        ToolName::AddApp,
        json!({"path": path, "name": "demo", "tasks": [{"name": "dev", "script": "dev"}]}),
    )
    .await;
    let app_id = created["id"].as_str().expect("app id").to_string();

    let listed = call(&ctx, ToolName::ListApps, json!({})).await;
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["apps"][0]["name"], "demo");

    let got = call(&ctx, ToolName::GetApp, json!({"app_id": app_id})).await;
    assert_eq!(got["name"], "demo");
    assert_eq!(got["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(got["ports"].as_array().unwrap().len(), 0);
    assert!(got["envFiles"].is_array(), "env file names are listed");
    assert!(got["recentRuns"].is_array());

    let overview = call(&ctx, ToolName::GetOverview, json!({})).await;
    assert_eq!(overview["appCount"], 1);
    assert_eq!(overview["running"], 0);
}

#[tokio::test]
async fn a_secret_never_comes_back_as_text() {
    let harness = Harness::new();
    let ctx = harness.context();
    let path = harness.project("secrets");
    let created = call(&ctx, ToolName::AddApp, json!({"path": path})).await;
    let app_id = created["id"].as_str().unwrap().to_string();

    let stored = call(
        &ctx,
        ToolName::SetEnvVar,
        json!({"scope": "app", "app_id": app_id, "key": "DB_PASSWORD", "value": "hunter2", "secret": true}),
    )
    .await;
    assert!(
        !stored.to_string().contains("hunter2"),
        "the write reply must not echo the secret: {stored}"
    );

    let listed = call(
        &ctx,
        ToolName::ListEnvVars,
        json!({"scope": "app", "app_id": app_id}),
    )
    .await;
    let text = listed.to_string();
    assert!(
        !text.contains("hunter2"),
        "listing leaks the secret: {text}"
    );
    assert_eq!(listed["scope"], "app");
    assert_eq!(listed["app"], "secrets");
    assert!(
        listed["vars"][0]["value"].is_null(),
        "a secret is reported as 'set only'"
    );
}

#[tokio::test]
async fn logs_and_ports_come_from_the_running_task() {
    let harness = Harness::new();
    let ctx = harness.context();
    let path = harness.project("logger");
    let port = 4451u16;
    let command = format!("sh {} {port}", fixture("listens-on-port.sh").display());
    let created = call(
        &ctx,
        ToolName::AddApp,
        json!({"path": path, "tasks": [{"name": "run", "command": command}]}),
    )
    .await;
    let app_id = created["id"].as_str().unwrap().to_string();

    let started = call(&ctx, ToolName::StartApp, json!({"app_id": app_id})).await;
    assert_eq!(started["started"], true);

    let running = call(&ctx, ToolName::ListRunning, json!({})).await;
    assert_eq!(running["count"], 1);
    let _task_id = running["running"][0]["taskId"]
        .as_str()
        .unwrap()
        .to_string();

    // Ports: the listener is real, so it must show up as confirmed.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        // Look the port up by number: the legacy tool answers with its owner.
        let ports = call(&ctx, ToolName::GetPorts, json!({"port": port})).await;
        if ports["owner"].as_str().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "port never confirmed: {ports}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // A running app cannot be removed: that would orphan the process.
    let removed = call(
        &ctx,
        ToolName::RemoveApp,
        json!({"app_id": app_id, "confirm": true}),
    )
    .await;
    assert!(
        removed.get("error").is_some(),
        "remove while running must fail"
    );
    assert_ne!(
        removed["error"]["field"].as_str(),
        Some("confirm"),
        "the rejection must be about the live app, not the missing confirmation"
    );

    let stopped = call(&ctx, ToolName::StopApp, json!({"app_id": app_id})).await;
    assert_eq!(stopped["stopped"], true);
    let removed = call(
        &ctx,
        ToolName::RemoveApp,
        json!({"app_id": app_id, "confirm": true}),
    )
    .await;
    assert_eq!(removed["removed"].as_str(), Some("logger"));
}

#[tokio::test]
async fn log_search_and_readiness_updates_work_through_tools() {
    let harness = Harness::new();
    let ctx = harness.context();
    let path = harness.project("search");
    let command = format!("sh {}", fixture("steady-log.sh").display());
    let created = call(
        &ctx,
        ToolName::AddApp,
        json!({"path": path, "tasks": [{"name": "run", "command": command}]}),
    )
    .await;
    let app_id = created["id"].as_str().unwrap().to_string();
    call(&ctx, ToolName::StartApp, json!({"app_id": app_id})).await;

    let running = call(&ctx, ToolName::ListRunning, json!({})).await;
    let task_id = running["running"][0]["taskId"]
        .as_str()
        .unwrap()
        .to_string();

    // Wait for output, then search for it across every task.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let logs = call(
            &ctx,
            ToolName::GetLogs,
            json!({"task_id": task_id, "lines": 50}),
        )
        .await;
        if logs["text"]
            .as_str()
            .unwrap_or("")
            .contains("[fixture] tick")
        {
            break;
        }
        assert!(Instant::now() < deadline, "no log output: {logs}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let hits = call(
        &ctx,
        ToolName::SearchLogs,
        json!({"query": "tick", "limit": 5}),
    )
    .await;
    assert_eq!(hits["interpretedAs"], "literal");
    assert!(
        hits["count"].as_u64().unwrap() > 0,
        "search found nothing: {hits}"
    );
    assert!(
        hits["matches"][0]["app"].is_string(),
        "matches name the app"
    );

    // Readiness can be changed without restarting the task.
    let updated = call(
        &ctx,
        ToolName::SetReadiness,
        json!({"task_id": task_id, "kind": "log", "regex": "tick"}),
    )
    .await;
    assert_eq!(updated["readiness"]["kind"], "log");
    assert_eq!(updated["task"], "run");
    let _ = call(&ctx, ToolName::StopApp, json!({"app_id": app_id})).await;
}

#[tokio::test]
async fn invalid_arguments_come_back_as_field_errors() {
    let harness = Harness::new();
    let ctx = harness.context();

    let missing = call(&ctx, ToolName::AddApp, json!({})).await;
    assert_eq!(missing["error"]["field"], "path");

    let bad_scope = call(&ctx, ToolName::ListEnvVars, json!({"scope": "nope"})).await;
    assert_eq!(bad_scope["error"]["field"], "scope");

    let bad_kind = call(
        &ctx,
        ToolName::SetReadiness,
        json!({"task_id": "t1", "kind": "telepathy"}),
    )
    .await;
    assert_eq!(bad_kind["error"]["field"], "kind");
}

#[tokio::test]
async fn a_tool_mutation_broadcasts_catalog_changed() {
    // The window and MCP share one command path, so a tool call must reach open windows as an
    // event. Without this the sidebar would keep showing the pre-call state.
    let harness = Harness::new();
    let ctx = harness.context();
    let mut subscription = harness.service.handle().subscribe();
    let path = harness.project("events");

    call(
        &ctx,
        ToolName::AddApp,
        json!({"path": path, "name": "events"}),
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(5), subscription.recv())
        .await
        .expect("an event within 5s")
        .expect("an event");
    assert!(
        matches!(
            event.kind,
            neohub_core::events::CoreEventKind::CatalogChanged
        ),
        "expected CatalogChanged, got {:?}",
        event.kind
    );
    assert!(event.revision > 0, "the event carries a new revision");
}

#[tokio::test]
async fn a_read_only_tool_does_not_broadcast() {
    let harness = Harness::new();
    let ctx = harness.context();
    let mut subscription = harness.service.handle().subscribe();

    call(&ctx, ToolName::ListApps, json!({})).await;
    let outcome = tokio::time::timeout(Duration::from_millis(300), subscription.recv()).await;
    assert!(outcome.is_err(), "a query must not wake every window");
}

#[tokio::test]
async fn read_tools_return_the_legacy_field_names() {
    // Parity with `src/main/mcp/tools.ts`: camelCase fields, `{count, apps}` for list_apps and
    // the `state` filter. A client written against the Electron server keeps working.
    let harness = Harness::new();
    let ctx = harness.context();
    let path = harness.project("legacy-shape");
    call(
        &ctx,
        ToolName::AddApp,
        json!({"path": path, "name": "legacy"}),
    )
    .await;

    let overview = call(&ctx, ToolName::GetOverview, json!({})).await;
    assert_eq!(overview["appCount"], 1);
    assert_eq!(overview["running"], 0);
    assert_eq!(overview["crashed"], 0);
    assert!(overview["activePorts"].as_array().unwrap().is_empty());
    assert_eq!(overview["apps"][0]["name"], "legacy");
    assert_eq!(overview["apps"][0]["state"], "idle");
    assert!(overview.get("devharborVersion").is_some());

    let listed = call(&ctx, ToolName::ListApps, json!({})).await;
    assert_eq!(listed["count"], 1);
    let app = &listed["apps"][0];
    assert_eq!(app["name"], "legacy");
    assert_eq!(app["state"], "idle");
    assert!(app.get("defaultScript").is_some(), "camelCase brief fields");
    assert!(app.get("packageManager").is_some());
    assert_eq!(app["autoStart"], false);
    assert!(app["ports"].as_array().unwrap().is_empty());

    // The state filter is exact.
    let idle = call(&ctx, ToolName::ListApps, json!({"state": "idle"})).await;
    assert_eq!(idle["count"], 1);
    let running = call(&ctx, ToolName::ListApps, json!({"state": "running"})).await;
    assert_eq!(running["count"], 0);

    // get_app resolves by id, exact name or absolute path.
    for argument in [
        json!({"app_id": app["id"]}),
        json!({"app": "legacy"}),
        json!({"path": app["path"]}),
    ] {
        let got = call(&ctx, ToolName::GetApp, argument.clone()).await;
        assert_eq!(got["name"], "legacy", "resolution failed for {argument}");
    }
}

#[tokio::test]
async fn mutating_tools_accept_the_legacy_argument_names() {
    let harness = Harness::new();
    let ctx = harness.context();
    let path = harness.project("legacy-args");
    call(
        &ctx,
        ToolName::AddApp,
        json!({"path": path, "name": "legacy"}),
    )
    .await;

    // update_app takes `app` plus camelCase fields.
    let updated = call(
        &ctx,
        ToolName::UpdateApp,
        json!({"app": "legacy", "tags": ["api"], "autoStart": true, "nodeVersion": "20.11.0"}),
    )
    .await;
    assert_eq!(updated["app"]["name"], "legacy");
    assert_eq!(updated["app"]["tags"][0], "api");
    assert_eq!(updated["app"]["autoStart"], true);

    // add_task takes `app`, `command`, `dependsOn` by name and `oneShot`.
    let first = call(
        &ctx,
        ToolName::AddTask,
        json!({"app": "legacy", "name": "migrate", "command": "true", "oneShot": true}),
    )
    .await;
    assert_eq!(first["task"]["name"], "migrate");
    assert_eq!(first["task"]["one_shot"], true);
    assert_eq!(
        first["task"]["readiness"]["kind"], "exit",
        "a one-shot task defaults to exit-code readiness"
    );

    let second = call(
        &ctx,
        ToolName::AddTask,
        json!({"app": "legacy", "name": "web", "script": "dev", "dependsOn": ["migrate"]}),
    )
    .await;
    assert_eq!(second["task"]["depends_on"].as_array().unwrap().len(), 1);

    // Exactly one of script/command is enforced.
    let both = call(
        &ctx,
        ToolName::AddTask,
        json!({"app": "legacy", "name": "bad", "script": "dev", "command": "node ."}),
    )
    .await;
    assert_eq!(both["error"]["field"], "script");

    let unknown_dep = call(
        &ctx,
        ToolName::AddTask,
        json!({"app": "legacy", "name": "bad2", "script": "dev", "dependsOn": ["nope"]}),
    )
    .await;
    assert_eq!(unknown_dep["error"]["field"], "dependsOn");

    // remove_app demands an explicit confirmation.
    let unconfirmed = call(&ctx, ToolName::RemoveApp, json!({"app": "legacy"})).await;
    assert_eq!(unconfirmed["error"]["field"], "confirm");
    let confirmed = call(
        &ctx,
        ToolName::RemoveApp,
        json!({"app": "legacy", "confirm": true}),
    )
    .await;
    assert_eq!(confirmed["removed"], "legacy");
}

#[tokio::test]
async fn get_logs_splits_per_task_and_get_ports_groups_by_app() {
    let harness = Harness::new();
    let ctx = harness.context();
    let path = harness.project("detail");
    let command = format!("sh {}", fixture("steady-log.sh").display());
    let created = call(
        &ctx,
        ToolName::AddApp,
        json!({"path": path, "name": "detail",
               "tasks": [{"name": "alpha", "command": command}, {"name": "beta", "command": command}]}),
    )
    .await;
    let app_id = created["id"].as_str().unwrap().to_string();
    call(&ctx, ToolName::StartApp, json!({"app_id": app_id})).await;

    // Wait for both tasks to emit, then read the app-level logs.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let logs = call(
            &ctx,
            ToolName::GetLogs,
            json!({"app": "detail", "lines": 20}),
        )
        .await;
        let tasks = logs["header"]["tasks"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if tasks.len() == 2 && tasks.iter().all(|task| task["hasOutput"] == true) {
            assert_eq!(logs["header"]["linesPerTask"], 20);
            let text = logs["text"].as_str().unwrap();
            assert!(text.contains("=== alpha ("), "per-task sections: {text}");
            assert!(text.contains("=== beta ("), "per-task sections: {text}");
            break;
        }
        assert!(Instant::now() < deadline, "logs not ready: {logs}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // A single task can be selected by name.
    let one = call(
        &ctx,
        ToolName::GetLogs,
        json!({"app": "detail", "task": "beta", "lines": 5}),
    )
    .await;
    assert_eq!(one["header"]["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(one["header"]["tasks"][0]["task"], "beta");
    assert!(
        !one["text"].as_str().unwrap().contains("=== alpha"),
        "a single task carries no section header"
    );

    // get_ports groups by app.
    let ports = call(&ctx, ToolName::GetPorts, json!({})).await;
    let apps = ports["apps"].as_array().expect("apps array");
    assert!(
        apps.iter().all(|entry| entry["app"].is_string()),
        "every group names its app: {ports}"
    );

    let _ = call(&ctx, ToolName::StopApp, json!({"app_id": app_id})).await;
}

#[tokio::test]
async fn add_app_accepts_the_legacy_arguments_and_workspace_tasks() {
    let harness = Harness::new();
    let ctx = harness.context();

    // A plain project with an explicit script: the task is named after the script.
    let plain = harness.project("plain");
    let created = call(
        &ctx,
        ToolName::AddApp,
        json!({"path": plain, "name": "plain", "script": "dev", "autoStart": true}),
    )
    .await;
    assert_eq!(created["name"], "plain");
    assert_eq!(
        created["autoStart"], true,
        "autoStart is applied after creation"
    );
    assert_eq!(created["tasks"][0]["name"], "dev");
    assert_eq!(created["tasks"][0]["script"], "dev");
    assert!(
        created["scripts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "dev")
    );

    // A project with no explicit script falls back to the detected default.
    let detected = harness.project("detected");
    let created = call(&ctx, ToolName::AddApp, json!({"path": detected})).await;
    assert_eq!(
        created["tasks"][0]["name"], "dev",
        "the detected default script"
    );

    // script and command together are refused.
    let both = call(
        &ctx,
        ToolName::AddApp,
        json!({"path": harness.project("both"), "script": "dev", "command": "node ."}),
    )
    .await;
    assert_eq!(both["error"]["field"], "script");
}

#[tokio::test]
async fn workspace_tasks_create_one_task_per_package() {
    let harness = Harness::new();
    let ctx = harness.context();
    let root = harness._dir.path().join("mono");
    std::fs::create_dir_all(root.join("apps/api")).unwrap();
    std::fs::create_dir_all(root.join("apps/web")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"mono","workspaces":["apps/*"]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("apps/api/package.json"),
        r#"{"name":"@mono/api","scripts":{"dev":"node api.js"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("apps/web/package.json"),
        r#"{"name":"@mono/web","scripts":{"dev":"node web.js"}}"#,
    )
    .unwrap();

    let created = call(
        &ctx,
        ToolName::AddApp,
        json!({"path": root.to_string_lossy(), "workspaceTasks": true}),
    )
    .await;
    let tasks = created["tasks"].as_array().expect("tasks");
    assert_eq!(tasks.len(), 2, "one task per workspace package: {created}");
    let mut names: Vec<&str> = tasks
        .iter()
        .filter_map(|task| task["name"].as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["@mono/api", "@mono/web"]);
    // Each task runs from its own directory.
    let mut dirs: Vec<&str> = tasks
        .iter()
        .filter_map(|task| task["working_dir_override"].as_str())
        .collect();
    dirs.sort_unstable();
    assert_eq!(dirs, vec!["apps/api", "apps/web"]);
    assert_eq!(
        created["workspaces"].as_array().unwrap().len(),
        2,
        "detected workspaces are reported back"
    );
}

#[tokio::test]
async fn set_readiness_validates_its_arguments_and_keeps_probes_local() {
    let harness = Harness::new();
    let ctx = harness.context();
    let path = harness.project("probe");
    let created = call(
        &ctx,
        ToolName::AddApp,
        json!({"path": path, "name": "probe", "script": "dev"}),
    )
    .await;
    assert_eq!(created["name"], "probe");

    // Legacy arguments: app + task (by name) + kind.
    let ok = call(
        &ctx,
        ToolName::SetReadiness,
        json!({"app": "probe", "task": "dev", "kind": "port", "port": 4455}),
    )
    .await;
    assert_eq!(ok["app"], "probe");
    assert_eq!(ok["task"], "dev");
    assert_eq!(ok["readiness"]["kind"], "port");
    assert!(ok["note"].as_str().unwrap().contains("next start"));

    // log: flags default to "i", and a broken regex is an error rather than a silent no-match.
    let logged = call(
        &ctx,
        ToolName::SetReadiness,
        json!({"app": "probe", "task": "dev", "kind": "log", "regex": "ready in"}),
    )
    .await;
    assert_eq!(logged["readiness"]["flags"], "i");
    let bad_regex = call(
        &ctx,
        ToolName::SetReadiness,
        json!({"app": "probe", "task": "dev", "kind": "log", "regex": "([unclosed"}),
    )
    .await;
    assert_eq!(bad_regex["error"]["field"], "regex");

    // http: loopback only, because the app polls this URL itself.
    let remote = call(
        &ctx,
        ToolName::SetReadiness,
        json!({"app": "probe", "task": "dev", "kind": "http", "url": "https://example.com/health"}),
    )
    .await;
    assert_eq!(remote["error"]["field"], "url");
    let local = call(
        &ctx,
        ToolName::SetReadiness,
        json!({"app": "probe", "task": "dev", "kind": "http", "url": "http://127.0.0.1:4000/ready"}),
    )
    .await;
    assert_eq!(local["readiness"]["kind"], "http");

    // Missing per-kind fields are named.
    for (arguments, field) in [
        (
            json!({"app": "probe", "task": "dev", "kind": "port"}),
            "port",
        ),
        (
            json!({"app": "probe", "task": "dev", "kind": "delay"}),
            "ms",
        ),
        (
            json!({"app": "probe", "task": "dev", "kind": "http"}),
            "url",
        ),
        (
            json!({"app": "probe", "task": "dev", "kind": "telepathy"}),
            "kind",
        ),
    ] {
        let response = call(&ctx, ToolName::SetReadiness, arguments.clone()).await;
        assert_eq!(response["error"]["field"], field, "for {arguments}");
    }
}
