//! Turning one MCP tool call into a core command.
//!
//! Every tool goes through `CoreHandle`, exactly like the UI does. That is the property that
//! keeps the two surfaces honest: MCP cannot start a task the window does not see, and a
//! validation rule added to the catalog applies to both.
//!
//! Errors are field-oriented (`{ "field": …, "message": … }`) so a client can tell a bad
//! argument from a failed operation.

use crate::redaction::Redactor;
use crate::tools::ToolName;
use neohub_core::catalog::{AddTaskInput, CreateTaskSpec, UpdateAppInput, UpdateTaskInput};
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::service::{CoreHandle, EnvScope, EnvVarInput};
use neohub_domain::{AppId, CommandKind, PackageManager, ReadinessSignal, Task, TaskId};
use serde_json::{Value, json};

/// One tool invocation's context.
pub struct ToolContext {
    pub handle: CoreHandle,
    pub redactor: Redactor,
}

#[derive(Debug)]
pub struct ToolError {
    pub field: Option<&'static str>,
    pub message: String,
}

impl ToolError {
    pub fn new(message: impl Into<String>) -> Self {
        ToolError {
            field: None,
            message: message.into(),
        }
    }

    pub fn field(field: &'static str, message: impl Into<String>) -> Self {
        ToolError {
            field: Some(field),
            message: message.into(),
        }
    }

    pub fn to_json(&self) -> Value {
        match self.field {
            Some(field) => json!({"error": {"field": field, "message": self.message}}),
            None => json!({"error": {"message": self.message}}),
        }
    }
}

type ToolResult = Result<Value, ToolError>;

/// Argument helpers: an MCP client's arguments are untrusted JSON.
fn require_str(args: &Value, field: &'static str) -> Result<String, ToolError> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ToolError::field(field, format!("{field} 必填")))
}

fn opt_str(args: &Value, field: &str) -> Option<String> {
    args.get(field).and_then(Value::as_str).map(str::to_string)
}

fn opt_bool(args: &Value, field: &str) -> Option<bool> {
    args.get(field).and_then(Value::as_bool)
}

fn opt_u64(args: &Value, field: &str) -> Option<u64> {
    args.get(field).and_then(Value::as_u64)
}

fn app_id(args: &Value) -> Result<AppId, ToolError> {
    Ok(AppId::from_existing(require_str(args, "app_id")?))
}

fn task_id(args: &Value) -> Result<TaskId, ToolError> {
    Ok(TaskId::from_existing(require_str(args, "task_id")?))
}

fn scope(args: &Value) -> Result<EnvScope, ToolError> {
    match require_str(args, "scope")?.as_str() {
        "global" => Ok(EnvScope::Global),
        "app" => Ok(EnvScope::App(app_id(args)?)),
        "task" => Ok(EnvScope::Task(task_id(args)?)),
        other => Err(ToolError::field(
            "scope",
            format!("scope 必须是 global/app/task，收到 {other}"),
        )),
    }
}

/// Dispatch one call.
///
/// Every arm returns its own result; the match is exhaustive over `ToolName`, so a new tool
/// cannot be added without a dispatcher.
pub async fn dispatch(ctx: &ToolContext, tool: ToolName, args: &Value) -> ToolResult {
    match tool {
        ToolName::GetOverview => {
            let snapshot = ctx
                .handle
                .request(CoreCommand::GetSnapshot)
                .await
                .map_err(core_error)?;
            let CoreReply::Snapshot(snapshot) = snapshot else {
                return Err(ToolError::new("unexpected snapshot reply"));
            };
            let state_of = |app: &neohub_domain::App| {
                snapshot
                    .app_states
                    .get(&app.id)
                    .cloned()
                    .unwrap_or(neohub_domain::ProcessState::Idle)
            };
            let running = snapshot
                .apps
                .iter()
                .filter(|app| {
                    matches!(
                        state_of(app),
                        neohub_domain::ProcessState::Running
                            | neohub_domain::ProcessState::Starting
                    )
                })
                .count();
            let crashed = snapshot
                .apps
                .iter()
                .filter(|app| state_of(app) == neohub_domain::ProcessState::Crashed)
                .count();
            let mut ports: Vec<u16> = snapshot
                .task_ports
                .values()
                .flat_map(|sample| sample.confirmed.iter().copied())
                .collect();
            ports.sort_unstable();
            ports.dedup();
            let apps: Vec<Value> = snapshot
                .apps
                .iter()
                .map(|app| {
                    json!({
                        "name": app.name,
                        "state": state_of(app).as_str(),
                    })
                })
                .collect();
            Ok(ctx.redactor.scrub_json(&json!({
                "devharborVersion": env!("CARGO_PKG_VERSION"),
                "appCount": snapshot.apps.len(),
                "running": running,
                "crashed": crashed,
                "activePorts": ports,
                "apps": apps,
            })))
        }
        ToolName::ListApps => {
            let snapshot = ctx
                .handle
                .request(CoreCommand::GetSnapshot)
                .await
                .map_err(core_error)?;
            let CoreReply::Snapshot(snapshot) = snapshot else {
                return Err(ToolError::new("unexpected snapshot reply"));
            };
            let filter = opt_str(args, "state");
            let apps: Vec<Value> = snapshot
                .apps
                .iter()
                .filter_map(|app| {
                    let state = snapshot
                        .app_states
                        .get(&app.id)
                        .cloned()
                        .unwrap_or(neohub_domain::ProcessState::Idle);
                    if filter
                        .as_deref()
                        .is_some_and(|wanted| wanted != state.as_str())
                    {
                        return None;
                    }
                    let ports = app_ports(&snapshot, &app.id);
                    Some(json!({
                        "id": app.id,
                        "name": app.name,
                        "path": app.path,
                        "state": state.as_str(),
                        "ports": ports,
                        "tags": app.tags,
                        "folder": app.folder,
                        "defaultScript": app.default_script,
                        "packageManager": app.package_manager.map(|manager| manager.as_str()),
                        "autoStart": app.auto_start,
                        "lastStartedAt": app.last_started_at,
                    }))
                })
                .collect();
            Ok(ctx.redactor.scrub_json(&json!({
                "count": apps.len(),
                "apps": apps,
            })))
        }
        ToolName::GetApp => {
            let snapshot = ctx
                .handle
                .request(CoreCommand::GetSnapshot)
                .await
                .map_err(core_error)?;
            let CoreReply::Snapshot(snapshot) = snapshot else {
                return Err(ToolError::new("unexpected snapshot reply"));
            };
            // The legacy tool took one `app` argument resolved as id, exact name or absolute
            // path; `app_id`/`path` stay accepted so older clients keep working.
            let needle = opt_str(args, "app");
            let by_id = opt_str(args, "app_id").or_else(|| needle.clone());
            let by_path = opt_str(args, "path").or_else(|| needle.clone());
            let by_name = needle.clone();
            let app = snapshot.apps.iter().find(|app| {
                by_id.as_deref() == Some(app.id.as_str())
                    || by_path.as_deref() == Some(app.path.as_str())
                    || by_name.as_deref() == Some(app.name.as_str())
            });
            let Some(app) = app else {
                return Err(ToolError::field("app", "没有应用匹配该 id、名称或路径"));
            };
            let tasks = snapshot
                .tasks_by_app
                .get(&app.id)
                .cloned()
                .unwrap_or_default();
            Ok(ctx.redactor.scrub_json(&json!({
                "app": app,
                "tasks": tasks.iter().map(task_json).collect::<Vec<_>>(),
                "state": snapshot.app_states.get(&app.id),
            })))
        }
        ToolName::ListRunning => {
            let snapshot = ctx
                .handle
                .request(CoreCommand::GetSnapshot)
                .await
                .map_err(core_error)?;
            let CoreReply::Snapshot(snapshot) = snapshot else {
                return Err(ToolError::new("unexpected snapshot reply"));
            };
            let now = neohub_storage::db::now_ms();
            let name_of = |app_id: &AppId| {
                snapshot
                    .apps
                    .iter()
                    .find(|app| &app.id == app_id)
                    .map(|app| app.name.clone())
                    .unwrap_or_else(|| app_id.to_string())
            };
            let running: Vec<Value> = snapshot
                .running_tasks
                .iter()
                .filter(|task| task.state.is_live())
                .map(|task| {
                    let ports = snapshot
                        .task_ports
                        .get(&task.task_id)
                        .map(|sample| sample.confirmed.clone())
                        .unwrap_or_default();
                    json!({
                        "app": name_of(&task.app_id),
                        "appId": task.app_id,
                        "taskId": task.task_id,
                        "pid": task.pid,
                        "state": task.state.as_str(),
                        // null means "no probe": up is not the same as serving.
                        "ready": snapshot.task_ready.get(&task.task_id).copied(),
                        "listening": !ports.is_empty(),
                        "uptimeSec": ((now - task.started_at).max(0)) / 1000,
                        "cpu": snapshot.task_stats.get(&task.task_id).map(|stats| stats.cpu_percent).unwrap_or(0.0),
                        "memMB": snapshot.task_stats.get(&task.task_id).map(|stats| stats.mem_mb).unwrap_or(0.0),
                        "ports": ports,
                    })
                })
                .collect();
            Ok(ctx.redactor.scrub_json(&json!({
                "count": running.len(),
                "running": running,
            })))
        }
        ToolName::GetLogs => {
            let task = task_id(args)?;
            let lines = opt_u64(args, "lines").unwrap_or(200) as usize;
            let reply = ctx
                .handle
                .request(CoreCommand::GetTerminalText {
                    task_id: task.clone(),
                })
                .await
                .map_err(core_error)?;
            let CoreReply::Logs(text) = reply else {
                return Err(ToolError::new("unexpected logs reply"));
            };
            let tail: Vec<&str> = text.lines().rev().take(lines).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            Ok(ctx.redactor.scrub_json(
                &json!({"task_id": task, "lines": tail.len(), "text": tail.join("\n")}),
            ))
        }
        ToolName::SearchLogs => {
            let query = require_str(args, "query")?;
            let use_regex = opt_bool(args, "regex").unwrap_or(false);
            let options = neohub_core::logs::SearchOptions {
                regex: use_regex,
                case_sensitive: opt_bool(args, "case_sensitive").unwrap_or(false),
                limit: opt_u64(args, "limit").unwrap_or(100).min(1000) as usize,
            };
            // An app filter is resolved to its tasks; the core search takes one task or all.
            let app = opt_str(args, "app");
            let reply = ctx
                .handle
                .request(CoreCommand::SearchLogs {
                    task_id: None,
                    query,
                    options,
                })
                .await
                .map_err(core_error)?;
            let CoreReply::LogMatches(matches) = reply else {
                return Err(ToolError::new("unexpected search reply"));
            };
            let snapshot = ctx
                .handle
                .request(CoreCommand::GetSnapshot)
                .await
                .map_err(core_error)?;
            let CoreReply::Snapshot(snapshot) = snapshot else {
                return Err(ToolError::new("unexpected snapshot reply"));
            };
            let app_matches = |task_id: &TaskId| -> bool {
                match app.as_deref() {
                    None => true,
                    Some(needle) => snapshot
                        .apps
                        .iter()
                        .find(|candidate| {
                            candidate.id.as_str() == needle
                                || candidate.name == needle
                                || candidate.path == needle
                        })
                        .map(|candidate| {
                            snapshot
                                .tasks_by_app
                                .get(&candidate.id)
                                .map(|tasks| tasks.iter().any(|task| &task.id == task_id))
                                .unwrap_or(false)
                        })
                        .unwrap_or(false),
                }
            };
            let rows: Vec<Value> = matches
                .into_iter()
                .filter(|(task_id, _)| app_matches(task_id))
                .map(|(task_id, hit)| {
                    let (app_name, task_name) = locate(&snapshot, &task_id);
                    json!({"app": app_name, "task": task_name, "line": hit.line})
                })
                .collect();
            Ok(ctx.redactor.scrub_json(&json!({
                "interpretedAs": if use_regex { "regex" } else { "literal" },
                "count": rows.len(),
                "matches": rows,
            })))
        }
        ToolName::ListEnvVars => {
            // Legacy arguments: `app` (plus optional `task`); `scope` stays accepted so callers
            // written against the earlier shape keep working.
            let scope = if let Some(task) = opt_str(args, "task") {
                EnvScope::Task(TaskId::from_existing(task))
            } else if let Some(app) = opt_str(args, "app") {
                EnvScope::App(resolve_app(&ctx.handle, &app).await?)
            } else {
                scope(args)?
            };
            let reply = ctx
                .handle
                .request(CoreCommand::ListEnvVars {
                    scope: scope.clone(),
                })
                .await
                .map_err(core_error)?;
            let CoreReply::EnvVars(rows) = reply else {
                return Err(ToolError::new("unexpected env reply"));
            };
            let vars: Vec<Value> = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "key": row.key,
                        // A secret is reported as null, never as text.
                        "value": row.value,
                        "isSecret": row.is_secret,
                        "enabled": row.enabled,
                    })
                })
                .collect();
            let app_name = app_name_for_scope(&ctx.handle, &scope).await;
            Ok(ctx.redactor.scrub_json(&json!({
                "app": app_name,
                "scope": match scope {
                    EnvScope::Global => "global",
                    EnvScope::App(_) => "app",
                    EnvScope::Task(_) => "task",
                },
                "vars": vars,
            })))
        }
        ToolName::GetPorts => {
            let snapshot = ctx
                .handle
                .request(CoreCommand::GetSnapshot)
                .await
                .map_err(core_error)?;
            let CoreReply::Snapshot(snapshot) = snapshot else {
                return Err(ToolError::new("unexpected snapshot reply"));
            };
            let filter = opt_str(args, "task_id");
            let ports: Vec<Value> = snapshot
                .task_ports
                .iter()
                // Keep the requested task when a filter is given; otherwise report all.
                .filter(|(task, _)| match filter.as_deref() {
                    Some(wanted) => task.as_str() == wanted,
                    None => true,
                })
                .map(|(task, sample)| {
                    json!({
                        "task_id": task,
                        // Confirmed means a real LISTEN socket; hinted is only a URL in the log.
                        "confirmed": sample.confirmed,
                        "hinted": sample.hinted,
                    })
                })
                .collect();
            Ok(ctx.redactor.scrub_json(&json!(ports)))
        }
        ToolName::ScanFolder => {
            let dir = require_str(args, "dir")?;
            let reply = ctx
                .handle
                .request(CoreCommand::ScanFolder { dir: dir.into() })
                .await
                .map_err(core_error)?;
            let CoreReply::ImportCandidates(candidates) = reply else {
                return Err(ToolError::new("unexpected scan reply"));
            };
            Ok(ctx.redactor.scrub_json(&json!(candidates)))
        }
        ToolName::ListNodeVersions => {
            let reply = ctx
                .handle
                .request(CoreCommand::ListNodeVersions)
                .await
                .map_err(core_error)?;
            let CoreReply::NodeVersions(versions) = reply else {
                return Err(ToolError::new("unexpected node reply"));
            };
            let list: Vec<Value> = versions
                .into_iter()
                .map(|install| {
                    json!({
                        "source": install.source.as_str(),
                        "version": install.version,
                        "bin_dir": install.bin_dir,
                    })
                })
                .collect();
            Ok(ctx.redactor.scrub_json(&json!(list)))
        }
        ToolName::AddApp => {
            let path = require_str(args, "path")?;
            let package_manager =
                opt_str(args, "package_manager").and_then(|name| PackageManager::parse(&name));
            let tasks: Vec<CreateTaskSpec> = args
                .get("tasks")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| CreateTaskSpec {
                            name: opt_str(item, "name").unwrap_or_default(),
                            command_kind: Some(if opt_str(item, "command").is_some() {
                                CommandKind::Custom
                            } else {
                                CommandKind::Script
                            }),
                            script: opt_str(item, "script"),
                            custom_command: opt_str(item, "command"),
                            working_dir_override: opt_str(item, "working_dir"),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let env: Vec<(String, String, bool)> = args
                .get("env")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            Some((
                                require_str(item, "key").ok()?,
                                require_str(item, "value").ok()?,
                                opt_bool(item, "secret").unwrap_or(false),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut tasks = tasks;
            let first_task = if tasks.is_empty() {
                None
            } else {
                Some(tasks.remove(0))
            };
            let reply = ctx
                .handle
                .request(CoreCommand::CreateApp {
                    input: Box::new(neohub_core::catalog::CreateAppInput {
                        path,
                        name: opt_str(args, "name"),
                        node_version_pref: None,
                        package_manager,
                        default_script: opt_str(args, "default_script"),
                        first_task,
                        tasks,
                        env_vars: env,
                    }),
                })
                .await
                .map_err(core_error)?;
            let CoreReply::App(app) = reply else {
                return Err(ToolError::new("unexpected create reply"));
            };
            Ok(json!({"app": app}))
        }
        ToolName::UpdateApp => {
            let id = app_id(args)?;
            let reply = ctx
                .handle
                .request(CoreCommand::UpdateApp {
                    app_id: id,
                    input: Box::new(UpdateAppInput {
                        name: opt_str(args, "name"),
                        folder: args
                            .get("folder")
                            .map(|value| value.as_str().map(str::to_string)),
                        tags: args.get("tags").and_then(Value::as_array).map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        }),
                        auto_start: opt_bool(args, "auto_start"),
                        auto_restart_on_change: opt_bool(args, "auto_restart_on_change"),
                        ..Default::default()
                    }),
                })
                .await
                .map_err(core_error)?;
            let CoreReply::App(app) = reply else {
                return Err(ToolError::new("unexpected update reply"));
            };
            Ok(json!({"app": app}))
        }
        ToolName::RemoveApp => {
            let id = app_id(args)?;
            ctx.handle
                .request(CoreCommand::RemoveApp { app_id: id.clone() })
                .await
                .map_err(core_error)?;
            Ok(json!({"removed": id}))
        }
        ToolName::AddTask => {
            let app = app_id(args)?;
            let reply = ctx
                .handle
                .request(CoreCommand::AddTask {
                    app_id: app,
                    input: Box::new(AddTaskInput {
                        name: require_str(args, "name")?,
                        command_kind: Some(if opt_str(args, "command").is_some() {
                            CommandKind::Custom
                        } else {
                            CommandKind::Script
                        }),
                        script: opt_str(args, "script"),
                        custom_command: opt_str(args, "command"),
                        depends_on: args
                            .get("depends_on")
                            .and_then(Value::as_array)
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(TaskId::from_existing)
                                    .collect()
                            })
                            .unwrap_or_default(),
                        ..Default::default()
                    }),
                })
                .await
                .map_err(core_error)?;
            let CoreReply::Task(task) = reply else {
                return Err(ToolError::new("unexpected add-task reply"));
            };
            Ok(json!({"task": task_json(&task)}))
        }
        ToolName::RemoveTask => {
            let task = task_id(args)?;
            ctx.handle
                .request(CoreCommand::RemoveTask {
                    task_id: task.clone(),
                })
                .await
                .map_err(core_error)?;
            Ok(json!({"removed": task}))
        }
        ToolName::SetEnvVar => {
            let scope = scope(args)?;
            let key = require_str(args, "key")?;
            let value = require_str(args, "value")?;
            let secret = opt_bool(args, "secret").unwrap_or(false);
            let enabled = opt_bool(args, "enabled").unwrap_or(true);
            // Read-modify-write: the command replaces a whole scope, so existing rows are kept.
            let existing = ctx
                .handle
                .request(CoreCommand::ListEnvVars {
                    scope: scope.clone(),
                })
                .await
                .map_err(core_error)?;
            let CoreReply::EnvVars(rows) = existing else {
                return Err(ToolError::new("unexpected env reply"));
            };
            let mut vars: Vec<EnvVarInput> = rows
                .into_iter()
                .map(|row| EnvVarInput {
                    key: row.key,
                    // Masked secrets stay masked: `None` means "keep what is stored".
                    value: row.value,
                    is_secret: row.is_secret,
                    enabled: row.enabled,
                    note: row.note,
                })
                .collect();
            match vars.iter_mut().find(|var| var.key == key) {
                Some(var) => {
                    var.value = Some(value);
                    var.is_secret = secret;
                    var.enabled = enabled;
                }
                None => vars.push(EnvVarInput {
                    key,
                    value: Some(value),
                    is_secret: secret,
                    enabled,
                    note: None,
                }),
            }
            let reply = ctx
                .handle
                .request(CoreCommand::SetEnvVars { scope, vars })
                .await
                .map_err(core_error)?;
            let CoreReply::EnvVars(rows) = reply else {
                return Err(ToolError::new("unexpected env reply"));
            };
            let rows: Vec<Value> = rows
                .into_iter()
                .map(|row| json!({"key": row.key, "value": row.value, "is_secret": row.is_secret}))
                .collect();
            Ok(ctx.redactor.scrub_json(&json!(rows)))
        }
        ToolName::SetReadiness => {
            let task = task_id(args)?;
            let kind = require_str(args, "kind")?;
            let readiness = match kind.as_str() {
                "none" => ReadinessSignal::None,
                "port" => ReadinessSignal::Port {
                    port: opt_u64(args, "port").unwrap_or(0) as u16,
                    host: opt_str(args, "host"),
                },
                "log" => ReadinessSignal::Log {
                    regex: require_str(args, "regex")?,
                    flags: opt_str(args, "flags"),
                },
                "http" => ReadinessSignal::Http {
                    url: require_str(args, "url")?,
                    status: opt_u64(args, "status").map(|value| value as u16),
                },
                "exit" => ReadinessSignal::Exit {
                    code: opt_u64(args, "code").map(|value| value as i32),
                },
                "delay" => ReadinessSignal::Delay {
                    ms: opt_u64(args, "ms").unwrap_or(0),
                },
                other => {
                    return Err(ToolError::field(
                        "kind",
                        format!("未知的 readiness 类型：{other}"),
                    ));
                }
            };
            let reply = ctx
                .handle
                .request(CoreCommand::UpdateTask {
                    task_id: task.clone(),
                    input: Box::new(UpdateTaskInput {
                        readiness: Some(readiness),
                        ..Default::default()
                    }),
                })
                .await
                .map_err(core_error)?;
            let CoreReply::Task(task) = reply else {
                return Err(ToolError::new("unexpected update reply"));
            };
            Ok(json!({"task": task_json(&task)}))
        }
        ToolName::StartApp => {
            ctx.handle
                .request(CoreCommand::StartApp {
                    app_id: app_id(args)?,
                })
                .await
                .map_err(core_error)?;
            Ok(json!({"started": true}))
        }
        ToolName::StopApp => {
            ctx.handle
                .request(CoreCommand::StopApp {
                    app_id: app_id(args)?,
                })
                .await
                .map_err(core_error)?;
            Ok(json!({"stopped": true}))
        }
        ToolName::RestartApp => {
            ctx.handle
                .request(CoreCommand::RestartApp {
                    app_id: app_id(args)?,
                })
                .await
                .map_err(core_error)?;
            Ok(json!({"restarted": true}))
        }
        ToolName::StartTask => {
            ctx.handle
                .request(CoreCommand::StartTask {
                    task_id: task_id(args)?,
                })
                .await
                .map_err(core_error)?;
            Ok(json!({"started": true}))
        }
        ToolName::StopTask => {
            ctx.handle
                .request(CoreCommand::StopTask {
                    task_id: task_id(args)?,
                })
                .await
                .map_err(core_error)?;
            Ok(json!({"stopped": true}))
        }
    }
}

/// Resolve `app` as an id, exact name or absolute path.
async fn resolve_app(handle: &CoreHandle, needle: &str) -> Result<AppId, ToolError> {
    let reply = handle
        .request(CoreCommand::GetSnapshot)
        .await
        .map_err(core_error)?;
    let CoreReply::Snapshot(snapshot) = reply else {
        return Err(ToolError::new("unexpected snapshot reply"));
    };
    snapshot
        .apps
        .iter()
        .find(|app| app.id.as_str() == needle || app.name == needle || app.path == needle)
        .map(|app| app.id.clone())
        .ok_or_else(|| {
            ToolError::field(
                "app",
                "没有应用匹配该 id、名称或路径（先用 list_apps 看看有哪些）",
            )
        })
}

/// App name for app/task scopes (a task scope resolves through its owner app).
async fn app_name_for_scope(handle: &CoreHandle, scope: &EnvScope) -> Option<String> {
    let app_id = match scope {
        EnvScope::Global => return None,
        EnvScope::App(app_id) => app_id.clone(),
        EnvScope::Task(task_id) => {
            let reply = handle.request(CoreCommand::GetSnapshot).await.ok()?;
            let CoreReply::Snapshot(snapshot) = reply else {
                return None;
            };
            snapshot
                .tasks_by_app
                .iter()
                .find(|(_, tasks)| tasks.iter().any(|task| &task.id == task_id))
                .map(|(app_id, _)| app_id.clone())?
        }
    };
    let reply = handle.request(CoreCommand::GetSnapshot).await.ok()?;
    let CoreReply::Snapshot(snapshot) = reply else {
        return None;
    };
    snapshot
        .apps
        .iter()
        .find(|app| app.id == app_id)
        .map(|app| app.name.clone())
}

/// `(app name, task name)` for one task id.
fn locate(snapshot: &neohub_core::snapshot::CoreSnapshot, task_id: &TaskId) -> (String, String) {
    for (app_id, tasks) in &snapshot.tasks_by_app {
        if let Some(task) = tasks.iter().find(|task| &task.id == task_id) {
            let app_name = snapshot
                .apps
                .iter()
                .find(|app| &app.id == app_id)
                .map(|app| app.name.clone())
                .unwrap_or_else(|| app_id.to_string());
            return (app_name, task.name.clone());
        }
    }
    (task_id.to_string(), task_id.to_string())
}

/// Confirmed listening ports across every task of one app (the legacy `aggregatePorts`).
fn app_ports(snapshot: &neohub_core::snapshot::CoreSnapshot, app_id: &AppId) -> Vec<u16> {
    let mut ports: Vec<u16> = snapshot
        .tasks_by_app
        .get(app_id)
        .map(|tasks| {
            tasks
                .iter()
                .flat_map(|task| {
                    snapshot
                        .task_ports
                        .get(&task.id)
                        .map(|sample| sample.confirmed.clone())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    ports.sort_unstable();
    ports.dedup();
    ports
}

fn task_json(task: &Task) -> Value {
    json!({
        "id": task.id,
        "app_id": task.app_id,
        "name": task.name,
        "position": task.position,
        "command_kind": task.command_kind,
        "script": task.script,
        "custom_command": task.custom_command,
        "depends_on": task.depends_on,
        "readiness": task.readiness,
        "one_shot": task.one_shot,
        "enabled": task.enabled,
    })
}

fn core_error(err: neohub_core::CoreError) -> ToolError {
    let message = err.to_string();
    let lowered = message.to_lowercase();
    let field = if lowered.contains("name") {
        Some("name")
    } else if lowered.contains("path") {
        Some("path")
    } else if lowered.contains("readiness") {
        Some("readiness")
    } else if lowered.contains("unknown task") || lowered.contains("depends") {
        Some("depends_on")
    } else {
        None
    };
    ToolError { field, message }
}
