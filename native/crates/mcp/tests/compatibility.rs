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
    let app_id = created["app"]["id"].as_str().expect("app id").to_string();

    let listed = call(&ctx, ToolName::ListApps, json!({})).await;
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["apps"][0]["name"], "demo");

    let got = call(&ctx, ToolName::GetApp, json!({"app_id": app_id})).await;
    assert_eq!(got["app"]["name"], "demo");
    assert_eq!(got["tasks"].as_array().unwrap().len(), 1);

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
    let app_id = created["app"]["id"].as_str().unwrap().to_string();

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
    assert!(
        listed[0]["value"].is_null(),
        "a secret is reported as 'set only'"
    );
    assert_eq!(listed[0]["is_secret"], true);
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
    let app_id = created["app"]["id"].as_str().unwrap().to_string();

    let started = call(&ctx, ToolName::StartApp, json!({"app_id": app_id})).await;
    assert_eq!(started["started"], true);

    let running = call(&ctx, ToolName::ListRunning, json!({})).await;
    assert_eq!(running.as_array().unwrap().len(), 1);
    let task_id = running[0]["task_id"].as_str().unwrap().to_string();

    // Ports: the listener is real, so it must show up as confirmed.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let ports = call(&ctx, ToolName::GetPorts, json!({"task_id": task_id})).await;
        let confirmed = &ports[0]["confirmed"];
        if confirmed
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .any(|value| value.as_u64() == Some(port as u64))
            })
            .unwrap_or(false)
        {
            break;
        }
        assert!(Instant::now() < deadline, "port never confirmed: {ports}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // A running app cannot be removed: that would orphan the process.
    let removed = call(&ctx, ToolName::RemoveApp, json!({"app_id": app_id})).await;
    assert!(
        removed.get("error").is_some(),
        "remove while running must fail"
    );

    let stopped = call(&ctx, ToolName::StopApp, json!({"app_id": app_id})).await;
    assert_eq!(stopped["stopped"], true);
    let removed = call(&ctx, ToolName::RemoveApp, json!({"app_id": app_id})).await;
    assert_eq!(removed["removed"].as_str(), Some(app_id.as_str()));
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
    let app_id = created["app"]["id"].as_str().unwrap().to_string();
    call(&ctx, ToolName::StartApp, json!({"app_id": app_id})).await;

    let running = call(&ctx, ToolName::ListRunning, json!({})).await;
    let task_id = running[0]["task_id"].as_str().unwrap().to_string();

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
    assert!(
        !hits.as_array().unwrap().is_empty(),
        "search found nothing: {hits}"
    );

    // Readiness can be changed without restarting the task.
    let updated = call(
        &ctx,
        ToolName::SetReadiness,
        json!({"task_id": task_id, "kind": "log", "regex": "tick"}),
    )
    .await;
    assert_eq!(updated["task"]["readiness"]["kind"], "log");
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
        assert_eq!(
            got["app"]["name"], "legacy",
            "resolution failed for {argument}"
        );
    }
}
