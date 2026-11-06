//! Protocol-level acceptance: real HTTP requests against a running endpoint.
//!
//! This is the "real client" check the plan asks for, done with plain HTTP so it exercises the
//! same path MCP Inspector would: initialize → tools/list → tools/call, plus the gate
//! (Host/Origin/Bearer) and a token rotation.

use neohub_core::service::CoreService;
use neohub_mcp::dispatch::ToolContext;
use neohub_mcp::redaction::Redactor;
use neohub_mcp::server::{McpConfig, start};
use neohub_storage::DataDir;
use serde_json::{Value, json};
use std::sync::Arc;

struct Fixture {
    _dir: tempfile::TempDir,
    service: Arc<CoreService>,
    handle: neohub_mcp::ServerHandle,
    client: reqwest::Client,
    token: String,
}

async fn fixture(require_auth: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("core");
    let project = dir.path().join("demo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("package.json"),
        r#"{"scripts":{"dev":"node ."}}"#,
    )
    .unwrap();
    let path = project.to_string_lossy().into_owned();
    service
        .storage_call(move |storage| {
            neohub_core::AppCatalog::new(storage)
                .create_app(neohub_core::catalog::CreateAppInput {
                    path,
                    name: Some("demo".into()),
                    node_version_pref: None,
                    package_manager: None,
                    default_script: None,
                    first_task: Some(neohub_core::catalog::CreateTaskSpec {
                        name: "dev".into(),
                        command_kind: Some(neohub_domain::CommandKind::Script),
                        script: Some("dev".into()),
                        custom_command: None,
                        working_dir_override: None,
                    }),
                    tasks: vec![],
                    env_vars: vec![],
                })
                .expect("app");
        })
        .expect("storage");

    let token = "test-token-123".to_string();
    let ctx = Arc::new(ToolContext {
        handle: service.handle(),
        redactor: Redactor::new(),
    });
    let handle = start(
        McpConfig {
            port: 0,
            require_auth,
            token: Some(token.clone()),
        },
        ctx,
    )
    .await
    .expect("server");
    Fixture {
        _dir: dir,
        service,
        handle,
        client: reqwest::Client::new(),
        token,
    }
}

impl Fixture {
    async fn post(&self, body: Value) -> reqwest::Response {
        self.client
            .post(self.handle.url())
            .header("authorization", format!("Bearer {}", self.token))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .expect("request")
    }
}

#[tokio::test]
async fn initialize_then_list_then_call_a_tool() {
    let fixture = fixture(true).await;

    let init = fixture
        .post(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                     "params": {"protocolVersion": "2025-06-18", "clientInfo": {"name": "test"}}}))
        .await;
    assert_eq!(init.status(), 200);
    let init: Value = init.json().await.unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "neohub");
    assert_eq!(
        init["result"]["capabilities"]["tools"]["listChanged"],
        false
    );

    let list = fixture
        .post(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .await;
    let list: Value = list.json().await.unwrap();
    let tools = list["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 22);
    assert!(tools.iter().any(|tool| tool["name"] == "get_overview"));
    assert!(
        tools
            .iter()
            .all(|tool| tool["inputSchema"]["type"] == "object"),
        "every tool advertises an object schema"
    );

    let call = fixture
        .post(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                     "params": {"name": "list_apps", "arguments": {}}}))
        .await;
    let call: Value = call.json().await.unwrap();
    assert_eq!(call["result"]["isError"], false);
    let text = call["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    let apps: Value = serde_json::from_str(text).expect("result is JSON text");
    assert_eq!(apps["count"], 1);
    assert_eq!(apps["apps"][0]["name"], "demo");
}

#[tokio::test]
async fn the_gate_rejects_bad_hosts_origins_and_tokens() {
    let fixture = fixture(true).await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();

    // A DNS-rebinding style Host header.
    let bad_host = fixture
        .client
        .post(fixture.handle.url())
        .header("authorization", format!("Bearer {}", fixture.token))
        .header("host", "localhost.evil.com")
        .body(body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(bad_host.status(), 403);

    // A browser Origin that is not loopback.
    let bad_origin = fixture
        .client
        .post(fixture.handle.url())
        .header("authorization", format!("Bearer {}", fixture.token))
        .header("origin", "https://evil.example")
        .body(body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(bad_origin.status(), 403);

    // Missing and wrong tokens.
    let missing = fixture
        .client
        .post(fixture.handle.url())
        .body(body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 401);
    let wrong = fixture
        .client
        .post(fixture.handle.url())
        .header("authorization", "Bearer wrong-token")
        .body(body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);

    // A loopback Origin is fine (MCP Inspector sends one).
    let good_origin = fixture
        .client
        .post(fixture.handle.url())
        .header("authorization", format!("Bearer {}", fixture.token))
        .header("origin", "http://localhost:6274")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(good_origin.status(), 200);
}

#[tokio::test]
async fn an_unknown_method_and_a_bad_tool_are_reported_as_jsonrpc_errors() {
    let fixture = fixture(false).await;

    let unknown = fixture
        .post(json!({"jsonrpc": "2.0", "id": 9, "method": "resources/list"}))
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(unknown["error"]["code"], -32601);

    let bad_tool = fixture
        .post(json!({"jsonrpc": "2.0", "id": 10, "method": "tools/call",
                     "params": {"name": "delete_everything", "arguments": {}}}))
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(bad_tool["error"]["code"], -32602);

    // A tool that fails validation reports isError with a field-shaped message.
    let invalid = fixture
        .post(json!({"jsonrpc": "2.0", "id": 11, "method": "tools/call",
                     "params": {"name": "add_app", "arguments": {}}}))
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(invalid["result"]["isError"], true);
    let text = invalid["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("path"), "field must be named: {text}");
}

#[tokio::test]
async fn a_notification_gets_no_reply_body() {
    let fixture = fixture(false).await;
    let response = fixture
        .post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await;
    assert_eq!(response.status(), 202);
    assert_eq!(response.text().await.unwrap(), "");
}

#[tokio::test]
async fn rotating_the_token_invalidates_the_previous_one() {
    // Two servers on the same shape: the old token only works against the old one.
    let old = fixture(true).await;
    let new = fixture(true).await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();

    let ok_old = old
        .client
        .post(old.handle.url())
        .header("authorization", format!("Bearer {}", old.token))
        .body(body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(ok_old.status(), 200);

    // After a rotation the previous token is rejected by the restarted endpoint.
    let rotated = "rotated-token-456";
    let ctx = Arc::new(ToolContext {
        handle: new.service.handle(),
        redactor: Redactor::new(),
    });
    let restarted = start(
        McpConfig {
            port: 0,
            require_auth: true,
            token: Some(rotated.to_string()),
        },
        ctx,
    )
    .await
    .expect("server");
    let rejected = old
        .client
        .post(restarted.url())
        .header("authorization", format!("Bearer {}", old.token))
        .body(body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 401, "the old token must not work");
    let accepted = old
        .client
        .post(restarted.url())
        .header("authorization", format!("Bearer {rotated}"))
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), 200);
}

#[tokio::test]
async fn requiring_auth_without_a_token_refuses_to_serve() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("core");
    let ctx = Arc::new(ToolContext {
        handle: service.handle(),
        redactor: Redactor::new(),
    });
    let handle = start(
        McpConfig {
            port: 0,
            require_auth: true,
            token: None,
        },
        ctx,
    )
    .await
    .expect("server");
    let response = reqwest::Client::new()
        .post(handle.url())
        .body(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        401,
        "an unconfigured token must not mean 'open'"
    );
}
