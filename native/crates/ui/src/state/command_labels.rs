//! Human labels for commands, used when a fire-and-forget command fails.
//!
//! Deliberately short: the label ends up as "启动失败：…" in a toast, so it has to read as the
//! action the user just took, not as an API name.
//!
//! This lives in its own module rather than inside the view because the view file is large enough
//! that a `#[test]` there trips the macro recursion limit — and a pure function deserves a small
//! home anyway.

use neohub_core::commands::CoreCommand;

/// What the user was trying to do.
pub fn describe_command(command: &CoreCommand) -> &'static str {
    match command {
        CoreCommand::StartApp { .. } | CoreCommand::StartTask { .. } => "启动",
        CoreCommand::StopApp { .. } | CoreCommand::StopTask { .. } => "停止",
        CoreCommand::RestartApp { .. } => "重启",
        CoreCommand::CreateApp { .. } => "添加项目",
        CoreCommand::RemoveApp { .. } => "移除项目",
        CoreCommand::UpdateApp { .. } => "保存应用配置",
        CoreCommand::AddTask { .. } | CoreCommand::UpdateTask { .. } => "保存任务",
        CoreCommand::RemoveTask { .. } => "删除任务",
        CoreCommand::ReorderTasks { .. } => "调整任务顺序",
        CoreCommand::SetEnvVars { .. } => "保存环境变量",
        CoreCommand::UpdateSettings { .. } => "保存设置",
        CoreCommand::ScanFolder { .. } => "扫描目录",
        CoreCommand::ExportLogs { .. } => "导出日志",
        CoreCommand::ClearLogs { .. } => "清空日志",
        CoreCommand::ExportDatabase { .. } => "导出数据库",
        CoreCommand::ResetDatabase => "重置数据库",
        _ => "操作",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_domain::{AppId, TaskId};

    #[test]
    fn start_and_stop_read_as_the_action() {
        let app = AppId::from_existing("a");
        assert_eq!(
            describe_command(&CoreCommand::StartApp {
                app_id: app.clone()
            }),
            "启动"
        );
        assert_eq!(
            describe_command(&CoreCommand::StartTask {
                task_id: TaskId::from_existing("t")
            }),
            "启动"
        );
        assert_eq!(
            describe_command(&CoreCommand::StopApp { app_id: app }),
            "停止"
        );
    }

    #[test]
    fn an_unmapped_command_still_has_a_label() {
        // Every unhandled command falls back to "操作", so a toast never reads "失败：…".
        assert_eq!(
            describe_command(&CoreCommand::GetSnapshot),
            "操作",
            "a read command is not a user action, but it must not be empty"
        );
    }

    #[test]
    fn labels_stay_short_enough_for_a_toast() {
        let app = AppId::from_existing("a");
        for label in [
            describe_command(&CoreCommand::StartApp {
                app_id: app.clone(),
            }),
            describe_command(&CoreCommand::RestartApp {
                app_id: app.clone(),
            }),
            describe_command(&CoreCommand::ResetDatabase),
            describe_command(&CoreCommand::SetEnvVars {
                scope: neohub_core::service::EnvScope::Global,
                vars: Vec::new(),
            }),
        ] {
            assert!(!label.is_empty());
            assert!(label.chars().count() <= 8, "{label} is too long");
        }
    }
}
