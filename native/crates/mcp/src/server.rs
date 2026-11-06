//! The MCP HTTP endpoint.
//!
//! Scope notes, because this is where a reader expects the official SDK:
//!
//! `rmcp` 3.9 does ship a Streamable HTTP transport, but it is assembled as a tower/axum
//! service, which drags a second HTTP stack into a desktop app for one loopback endpoint. What
//! this module implements is the piece of the protocol the product actually uses — JSON-RPC 2.0
//! over `POST /mcp` with `initialize`, `tools/list` and `tools/call` — behind the exact same
//! request gate as the Electron build. It is protocol-compatible for those calls, verified
//! against real HTTP requests in `tests/protocol.rs`, and the rmcp swap is a contained
//! follow-up (the tool layer is transport-agnostic). The gaps are listed in PROGRESS.

use crate::auth::{bearer_matches, host_allowed, origin_allowed};
use crate::dispatch::{ToolContext, dispatch};
use crate::tools::{ToolName, tool_specs};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Server configuration, mirroring the MCP settings.
#[derive(Clone, Debug)]
pub struct McpConfig {
    pub port: u16,
    pub require_auth: bool,
    /// Bearer token; required when `require_auth` is set.
    pub token: Option<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        McpConfig {
            port: 6872,
            require_auth: true,
            token: None,
        }
    }
}

/// A running endpoint.
pub struct ServerHandle {
    pub addr: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl ServerHandle {
    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.addr)
    }

    /// Stop accepting new requests. In-flight requests finish.
    pub fn stop(&self) {
        let _ = self.shutdown.send(true);
    }
}

/// Bind and serve until `stop` is called.
pub async fn start(config: McpConfig, ctx: Arc<ToolContext>) -> std::io::Result<ServerHandle> {
    let listener = TcpListener::bind(("127.0.0.1", config.port)).await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let config = Arc::new(config);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => break,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let config = config.clone();
                    let ctx = ctx.clone();
                    tokio::spawn(async move {
                        let _ = handle_connection(stream, config, ctx).await;
                    });
                }
            }
        }
    });

    Ok(ServerHandle {
        addr,
        shutdown: shutdown_tx,
    })
}

/// One HTTP request/response. Deliberately minimal: a single `POST /mcp` with a JSON body.
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    config: Arc<McpConfig>,
    ctx: Arc<ToolContext>,
) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    // Read until the headers are complete.
    let header_end = loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_headers_end(&buffer) {
            break position;
        }
        if buffer.len() > 64 * 1024 {
            return respond(&mut stream, 431, "text/plain", "headers too large").await;
        }
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_lowercase(), value.trim().to_string()));
        }
    }
    let header = |name: &str| -> Option<&str> {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };

    // The same gate the Electron server applied, before anything else happens.
    if !host_allowed(header("host")) {
        return respond(&mut stream, 403, "text/plain", "forbidden host").await;
    }
    if !origin_allowed(header("origin")) {
        return respond(&mut stream, 403, "text/plain", "forbidden origin").await;
    }
    if config.require_auth {
        let expected = config.token.as_deref().unwrap_or("");
        if expected.is_empty() {
            // Auth is required but no token is configured: refuse rather than run open.
            return respond(&mut stream, 401, "text/plain", "token not configured").await;
        }
        if !bearer_matches(header("authorization"), expected) {
            return respond(&mut stream, 401, "text/plain", "unauthorized").await;
        }
    }

    if method != "POST" || path != "/mcp" {
        return respond(&mut stream, 404, "text/plain", "not found").await;
    }

    let content_length: usize = header("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8_lossy(
        &buffer[body_start..(body_start + content_length).min(buffer.len())],
    )
    .to_string();

    let request: Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) => {
            let reply = jsonrpc_error(Value::Null, -32700, &format!("parse error: {error}"));
            return respond(&mut stream, 200, "application/json", &reply.to_string()).await;
        }
    };

    let response = handle_rpc(&ctx, &request).await;
    match response {
        Some(reply) => respond(&mut stream, 200, "application/json", &reply.to_string()).await,
        // A notification has no response body.
        None => respond(&mut stream, 202, "text/plain", "").await,
    }
}

/// Dispatch one JSON-RPC message. `None` means "notification, no reply".
async fn handle_rpc(ctx: &ToolContext, request: &Value) -> Option<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => Some(jsonrpc_result(
            id,
            json!({
                "protocolVersion": "2025-06-18",
                "serverInfo": {"name": "neohub", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"tools": {"listChanged": false}},
            }),
        )),
        // Notifications carry no id and expect no reply.
        m if m.starts_with("notifications/") => None,
        "ping" => Some(jsonrpc_result(id, json!({}))),
        "tools/list" => {
            let tools: Vec<Value> = tool_specs()
                .into_iter()
                .map(|spec| {
                    json!({
                        "name": spec.name.as_str(),
                        "description": spec.description,
                        "inputSchema": spec.input_schema,
                    })
                })
                .collect();
            Some(jsonrpc_result(id, json!({"tools": tools})))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(tool) = ToolName::parse(name) else {
                return Some(jsonrpc_error(id, -32602, &format!("unknown tool: {name}")));
            };
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            match dispatch(ctx, tool, &arguments).await {
                Ok(value) => Some(jsonrpc_result(
                    id,
                    json!({
                        "content": [{"type": "text", "text": ctx.redactor.scrub(&value.to_string())}],
                        "isError": false,
                    }),
                )),
                Err(error) => Some(jsonrpc_result(
                    id,
                    json!({
                        "content": [{"type": "text", "text": ctx.redactor.scrub(&error.to_json().to_string())}],
                        "isError": true,
                    }),
                )),
            }
        }
        _ => Some(jsonrpc_error(
            id,
            -32601,
            &format!("method not found: {method}"),
        )),
    }
}

fn jsonrpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn jsonrpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn respond(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await
}
