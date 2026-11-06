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
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["name"], "demo");
    assert_eq!(
        listed[0]["tasks"], 1,
        "the task count comes from the snapshot"
    );

    let got = call(&ctx, ToolName::GetApp, json!({"app_id": app_id})).await;
    assert_eq!(got["app"]["name"], "demo");
    assert_eq!(got["tasks"].as_array().unwrap().len(), 1);

    let overview = call(&ctx, ToolName::GetOverview, json!({})).await;
    assert_eq!(overview["apps"], 1);
    assert_eq!(overview["running_tasks"], 0);
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
