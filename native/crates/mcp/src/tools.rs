//! The 22 MCP tools.
//!
//! Names and input shapes are taken from `src/main/mcp/tools.ts`; a compatibility test asserts
//! the list stays complete, so a tool cannot be silently dropped during a refactor. Every tool
//! is a thin wrapper over `CoreHandle` — no tool reaches the database, the PTY or the process
//! table on its own, which is what keeps the UI and MCP in step.

use serde_json::{Value, json};

/// Every tool this server exposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolName {
    GetOverview,
    ListApps,
    GetApp,
    ListRunning,
    GetLogs,
    SearchLogs,
    ListEnvVars,
    GetPorts,
    ScanFolder,
    ListNodeVersions,
    AddApp,
    UpdateApp,
    RemoveApp,
    AddTask,
    RemoveTask,
    SetEnvVar,
    SetReadiness,
    StartApp,
    StopApp,
    RestartApp,
    StartTask,
    StopTask,
}

impl ToolName {
    /// Wire name (what clients see).
    pub fn as_str(self) -> &'static str {
        match self {
            ToolName::GetOverview => "get_overview",
            ToolName::ListApps => "list_apps",
            ToolName::GetApp => "get_app",
            ToolName::ListRunning => "list_running",
            ToolName::GetLogs => "get_logs",
            ToolName::SearchLogs => "search_logs",
            ToolName::ListEnvVars => "list_env_vars",
            ToolName::GetPorts => "get_ports",
            ToolName::ScanFolder => "scan_folder",
            ToolName::ListNodeVersions => "list_node_versions",
            ToolName::AddApp => "add_app",
            ToolName::UpdateApp => "update_app",
            ToolName::RemoveApp => "remove_app",
            ToolName::AddTask => "add_task",
            ToolName::RemoveTask => "remove_task",
            ToolName::SetEnvVar => "set_env_var",
            ToolName::SetReadiness => "set_readiness",
            ToolName::StartApp => "start_app",
            ToolName::StopApp => "stop_app",
            ToolName::RestartApp => "restart_app",
            ToolName::StartTask => "start_task",
            ToolName::StopTask => "stop_task",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        ALL.iter().copied().find(|tool| tool.as_str() == name)
    }
}

/// All 22 tools, in the order the legacy server listed them.
pub const ALL: [ToolName; 22] = [
    ToolName::GetOverview,
    ToolName::ListApps,
    ToolName::GetApp,
    ToolName::ListRunning,
    ToolName::GetLogs,
    ToolName::SearchLogs,
    ToolName::ListEnvVars,
    ToolName::GetPorts,
    ToolName::ScanFolder,
    ToolName::ListNodeVersions,
    ToolName::AddApp,
    ToolName::UpdateApp,
    ToolName::RemoveApp,
    ToolName::AddTask,
    ToolName::RemoveTask,
    ToolName::SetEnvVar,
    ToolName::SetReadiness,
    ToolName::StartApp,
    ToolName::StopApp,
    ToolName::RestartApp,
    ToolName::StartTask,
    ToolName::StopTask,
];

/// A tool as advertised to clients.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolSpec {
    pub name: ToolName,
    pub description: &'static str,
    /// JSON Schema for the arguments.
    pub input_schema: Value,
}

fn app_id() -> Value {
    json!({"type": "string", "description": "应用 ID"})
}

fn task_id() -> Value {
    json!({"type": "string", "description": "任务 ID"})
}

/// The advertised tool list.
pub fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: ToolName::GetOverview,
            description: "总览：应用数量、运行中的任务、CPU/内存汇总与端口。",
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        },
        ToolSpec {
            name: ToolName::ListApps,
            description: "列出所有应用（含任务数量、状态、标签、文件夹）。",
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        },
        ToolSpec {
            name: ToolName::GetApp,
            description: "读取单个应用的详情与任务列表。",
            input_schema: json!({
                "type": "object",
                "properties": {"app_id": app_id(), "path": {"type": "string", "description": "或用路径查找"}},
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::ListRunning,
            description: "列出正在运行的任务（pid、启动时间、CPU/内存、端口）。",
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        },
        ToolSpec {
            name: ToolName::GetLogs,
            description: "读取一个任务的日志尾部（默认 200 行）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": task_id(),
                    "lines": {"type": "integer", "minimum": 1, "maximum": 2000, "default": 200}
                },
                "required": ["task_id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::SearchLogs,
            description: "在所有保留的任务日志里搜索（字面量或正则）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "regex": {"type": "boolean", "default": false},
                    "case_sensitive": {"type": "boolean", "default": false},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::ListEnvVars,
            description: "列出某个作用域的环境变量（秘密只回报“已设置”）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "scope": {"type": "string", "enum": ["global", "app", "task"]},
                    "app_id": app_id(),
                    "task_id": task_id()
                },
                "required": ["scope"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::GetPorts,
            description: "列出任务端口：confirmed 为真实监听，hinted 仅来自日志提及。",
            input_schema: json!({
                "type": "object",
                "properties": {"task_id": task_id()},
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::ScanFolder,
            description: "扫描目录下的 Node 项目（浅层，仅直接子目录）。",
            input_schema: json!({
                "type": "object",
                "properties": {"dir": {"type": "string"}},
                "required": ["dir"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::ListNodeVersions,
            description: "列出本机可用的 Node 安装（nvm/fnm/volta/asdf/system）。",
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        },
        ToolSpec {
            name: ToolName::AddApp,
            description: "添加应用（可一次创建多个任务与环境变量，原子完成）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "name": {"type": "string"},
                    "package_manager": {"type": "string", "enum": ["npm", "yarn", "pnpm", "bun"]},
                    "default_script": {"type": "string"},
                    "tasks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string"},
                                "script": {"type": "string"},
                                "command": {"type": "string"},
                                "working_dir": {"type": "string"}
                            },
                            "additionalProperties": false
                        }
                    },
                    "env": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": {"type": "string"},
                                "value": {"type": "string"},
                                "secret": {"type": "boolean", "default": false}
                            },
                            "required": ["key", "value"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::UpdateApp,
            description: "更新应用字段（名称、标签、文件夹、自动启动等）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "app_id": app_id(),
                    "name": {"type": "string"},
                    "folder": {"type": ["string", "null"]},
                    "tags": {"type": "array", "items": {"type": "string"}},
                    "auto_start": {"type": "boolean"},
                    "auto_restart_on_change": {"type": "boolean"}
                },
                "required": ["app_id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::RemoveApp,
            description: "移除应用注册（不删除磁盘上的项目目录；运行中会被拒绝）。",
            input_schema: json!({
                "type": "object",
                "properties": {"app_id": app_id()},
                "required": ["app_id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::AddTask,
            description: "为应用新增任务（脚本或自定义命令，可设依赖）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "app_id": app_id(),
                    "name": {"type": "string"},
                    "script": {"type": "string"},
                    "command": {"type": "string"},
                    "depends_on": {"type": "array", "items": {"type": "string"}},
                    "readiness": {"type": "string", "enum": ["none", "port", "log", "http", "exit", "delay"]}
                },
                "required": ["app_id", "name"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::RemoveTask,
            description: "删除任务（仍被其他任务依赖时拒绝）。",
            input_schema: json!({
                "type": "object",
                "properties": {"task_id": task_id()},
                "required": ["task_id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::SetEnvVar,
            description: "设置一个环境变量（global/app/task 三层；秘密值会被加密）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "scope": {"type": "string", "enum": ["global", "app", "task"]},
                    "app_id": app_id(),
                    "task_id": task_id(),
                    "key": {"type": "string"},
                    "value": {"type": "string"},
                    "secret": {"type": "boolean", "default": false},
                    "enabled": {"type": "boolean", "default": true}
                },
                "required": ["scope", "key", "value"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::SetReadiness,
            description: "设置任务的就绪探测（六种；none 表示不验证）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": task_id(),
                    "kind": {"type": "string", "enum": ["none", "port", "log", "http", "exit", "delay"]},
                    "port": {"type": "integer"},
                    "host": {"type": "string"},
                    "regex": {"type": "string"},
                    "flags": {"type": "string"},
                    "url": {"type": "string"},
                    "status": {"type": "integer"},
                    "code": {"type": "integer"},
                    "ms": {"type": "integer"}
                },
                "required": ["task_id", "kind"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::StartApp,
            description: "启动应用（按依赖顺序，等待就绪；约 45 秒后返回 still starting）。",
            input_schema: json!({
                "type": "object",
                "properties": {"app_id": app_id()},
                "required": ["app_id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::StopApp,
            description: "按反向依赖顺序停止应用。",
            input_schema: json!({
                "type": "object",
                "properties": {"app_id": app_id()},
                "required": ["app_id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::RestartApp,
            description: "重启应用（先逆序停止再启动）。",
            input_schema: json!({
                "type": "object",
                "properties": {"app_id": app_id()},
                "required": ["app_id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::StartTask,
            description: "启动单个任务（调试用）。",
            input_schema: json!({
                "type": "object",
                "properties": {"task_id": task_id()},
                "required": ["task_id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: ToolName::StopTask,
            description: "停止单个任务。",
            input_schema: json!({
                "type": "object",
                "properties": {"task_id": task_id()},
                "required": ["task_id"],
                "additionalProperties": false
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_twenty_two_tools_are_advertised_exactly_once() {
        let specs = tool_specs();
        assert_eq!(specs.len(), 22);
        let mut names: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 22, "no duplicates");
        assert_eq!(ALL.len(), 22);
        for tool in ALL {
            assert!(
                specs.iter().any(|spec| spec.name == tool),
                "{} is missing a spec",
                tool.as_str()
            );
        }
    }

    #[test]
    fn the_wire_names_match_the_legacy_server() {
        let expected = [
            "get_overview",
            "list_apps",
            "get_app",
            "list_running",
            "get_logs",
            "search_logs",
            "list_env_vars",
            "get_ports",
            "scan_folder",
            "list_node_versions",
            "add_app",
            "update_app",
            "remove_app",
            "add_task",
            "remove_task",
            "set_env_var",
            "set_readiness",
            "start_app",
            "stop_app",
            "restart_app",
            "start_task",
            "stop_task",
        ];
        let mut actual: Vec<&str> = tool_specs().iter().map(|spec| spec.name.as_str()).collect();
        actual.sort_unstable();
        let mut expected_sorted = expected.to_vec();
        expected_sorted.sort_unstable();
        assert_eq!(actual, expected_sorted);
    }

    #[test]
    fn every_spec_carries_an_object_schema() {
        for spec in tool_specs() {
            assert_eq!(
                spec.input_schema.get("type").and_then(Value::as_str),
                Some("object"),
                "{} must take an object",
                spec.name.as_str()
            );
            assert!(
                !spec.description.is_empty(),
                "{} needs a description",
                spec.name.as_str()
            );
        }
    }

    #[test]
    fn required_arguments_are_declared_for_the_tools_that_need_them() {
        let specs = tool_specs();
        let required = |name: &str| -> Vec<String> {
            specs
                .iter()
                .find(|spec| spec.name.as_str() == name)
                .and_then(|spec| spec.input_schema.get("required"))
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        assert_eq!(required("get_logs"), vec!["task_id".to_string()]);
        assert_eq!(required("add_app"), vec!["path".to_string()]);
        assert_eq!(
            required("set_env_var"),
            vec!["scope".to_string(), "key".to_string(), "value".to_string()]
        );
        assert!(required("list_apps").is_empty(), "list_apps takes nothing");
    }

    #[test]
    fn names_round_trip_through_parse() {
        for tool in ALL {
            assert_eq!(ToolName::parse(tool.as_str()), Some(tool));
        }
        assert_eq!(ToolName::parse("no_such_tool"), None);
    }
}
