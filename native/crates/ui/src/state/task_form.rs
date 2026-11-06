//! The task editor's form state.
//!
//! Goals (each tested): typing marks the draft dirty; a core validation error is mapped back
//! to the exact field that caused it; a refresh triggered by MCP cannot overwrite what the
//! user is typing; and the form produces a validated `UpdateTaskInput` for the command layer.

use crate::state::edit_session::{Draft, ExternalOutcome, SaveRequest};
use crate::state::text_input::{KeyOutcome, TextField};
use neohub_domain::{CommandKind, ReadinessSignal, Task};

/// Which text field has the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Command,
    WorkingDir,
    ReadinessArg,
}

impl Field {
    /// Tab order for the editor.
    pub const ORDER: [Field; 4] = [
        Field::Name,
        Field::Command,
        Field::WorkingDir,
        Field::ReadinessArg,
    ];

    pub fn next(self) -> Field {
        let index = Field::ORDER
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0);
        Field::ORDER[(index + 1) % Field::ORDER.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            Field::Name => "名称",
            Field::Command => "命令",
            Field::WorkingDir => "工作目录",
            Field::ReadinessArg => "就绪参数",
        }
    }
}

/// Readiness kinds the editor can switch between (all six are preserved).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessChoice {
    None,
    Port,
    Log,
    Http,
    Exit,
    Delay,
}

impl ReadinessChoice {
    pub const ALL: [ReadinessChoice; 6] = [
        ReadinessChoice::None,
        ReadinessChoice::Port,
        ReadinessChoice::Log,
        ReadinessChoice::Http,
        ReadinessChoice::Exit,
        ReadinessChoice::Delay,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ReadinessChoice::None => "none（不验证）",
            ReadinessChoice::Port => "port",
            ReadinessChoice::Log => "log",
            ReadinessChoice::Http => "http",
            ReadinessChoice::Exit => "exit",
            ReadinessChoice::Delay => "delay",
        }
    }

    #[cfg(test)]
    fn from_signal(signal: &ReadinessSignal) -> Self {
        match signal {
            ReadinessSignal::None => ReadinessChoice::None,
            ReadinessSignal::Port { .. } => ReadinessChoice::Port,
            ReadinessSignal::Log { .. } => ReadinessChoice::Log,
            ReadinessSignal::Http { .. } => ReadinessChoice::Http,
            ReadinessSignal::Exit { .. } => ReadinessChoice::Exit,
            ReadinessSignal::Delay { .. } => ReadinessChoice::Delay,
        }
    }

    fn default_arg(self) -> String {
        match self {
            ReadinessChoice::Port => "3000".into(),
            ReadinessChoice::Log => "ready".into(),
            ReadinessChoice::Http => "http://127.0.0.1:3000/health".into(),
            ReadinessChoice::Exit => "0".into(),
            ReadinessChoice::Delay => "1000".into(),
            ReadinessChoice::None => String::new(),
        }
    }
}

/// Editable task fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskFormFields {
    pub name: String,
    pub command: String,
    pub working_dir: String,
    pub readiness: ReadinessChoice,
    pub readiness_arg: String,
    pub one_shot: bool,
    pub enabled: bool,
}

impl TaskFormFields {
    fn from_task(task: &Task) -> Self {
        let (readiness, readiness_arg) = match &task.readiness {
            ReadinessSignal::None => (ReadinessChoice::None, String::new()),
            ReadinessSignal::Port { port, .. } => (ReadinessChoice::Port, port.to_string()),
            ReadinessSignal::Log { regex, .. } => (ReadinessChoice::Log, regex.clone()),
            ReadinessSignal::Http { url, .. } => (ReadinessChoice::Http, url.clone()),
            ReadinessSignal::Exit { code } => (
                ReadinessChoice::Exit,
                code.map(|c| c.to_string()).unwrap_or_default(),
            ),
            ReadinessSignal::Delay { ms } => (ReadinessChoice::Delay, ms.to_string()),
        };
        TaskFormFields {
            name: task.name.clone(),
            command: task
                .custom_command
                .clone()
                .or_else(|| task.script.clone())
                .unwrap_or_default(),
            working_dir: task.working_dir_override.clone().unwrap_or_default(),
            readiness,
            readiness_arg,
            one_shot: task.one_shot,
            enabled: task.enabled,
        }
    }

    /// Build the readiness signal; `Err` carries the field-level message.
    pub fn readiness_signal(&self) -> Result<ReadinessSignal, String> {
        let arg = self.readiness_arg.trim();
        match self.readiness {
            ReadinessChoice::None => Ok(ReadinessSignal::None),
            ReadinessChoice::Port => {
                let port: u16 = arg
                    .parse()
                    .map_err(|_| "端口必须是 1–65535 的数字".to_string())?;
                if port == 0 {
                    return Err("端口不能是 0".to_string());
                }
                Ok(ReadinessSignal::Port { port, host: None })
            }
            ReadinessChoice::Log => {
                if arg.is_empty() {
                    return Err("log 就绪需要匹配模式".to_string());
                }
                Ok(ReadinessSignal::Log {
                    regex: arg.to_string(),
                    flags: None,
                })
            }
            ReadinessChoice::Http => {
                if !arg.starts_with("http://") && !arg.starts_with("https://") {
                    return Err("http 就绪需要以 http:// 或 https:// 开头的 URL".to_string());
                }
                Ok(ReadinessSignal::Http {
                    url: arg.to_string(),
                    status: None,
                })
            }
            ReadinessChoice::Exit => {
                let code: i32 = arg.parse().map_err(|_| "退出码必须是整数".to_string())?;
                Ok(ReadinessSignal::Exit { code: Some(code) })
            }
            ReadinessChoice::Delay => {
                let ms: u64 = arg.parse().map_err(|_| "延迟必须是毫秒数".to_string())?;
                Ok(ReadinessSignal::Delay { ms })
            }
        }
    }
}

/// A field-level error coming back from the core (or from local validation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    pub field: Field,
    pub message: String,
}

/// The editor.
pub struct TaskForm {
    task_id: String,
    draft: Draft<TaskFormFields>,
    /// Caret buffers, one per text field, so switching focus keeps the text.
    name_text: TextField,
    command_text: TextField,
    working_dir_text: TextField,
    readiness_text: TextField,
    focused: Field,
    error: Option<FieldError>,
}

impl TaskForm {
    pub fn new(task: &Task) -> Self {
        let fields = TaskFormFields::from_task(task);
        TaskForm {
            task_id: task.id.to_string(),
            name_text: TextField::new(fields.name.clone()),
            command_text: TextField::new(fields.command.clone()),
            working_dir_text: TextField::new(fields.working_dir.clone()),
            readiness_text: TextField::new(fields.readiness_arg.clone()),
            draft: Draft::new(task.id.to_string(), fields),
            focused: Field::Name,
            error: None,
        }
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn fields(&self) -> &TaskFormFields {
        self.draft.local()
    }

    pub fn focused(&self) -> Field {
        self.focused
    }

    pub fn error(&self) -> Option<&FieldError> {
        self.error.as_ref()
    }

    pub fn is_dirty(&self) -> bool {
        self.draft.is_dirty()
    }

    pub fn conflict(&self) -> Option<&TaskFormFields> {
        self.draft.conflict()
    }

    pub fn focus(&mut self, field: Field) {
        self.focused = field;
    }

    pub fn focus_next(&mut self) {
        self.focused = self.focused.next();
    }

    fn text_mut(&mut self) -> &mut TextField {
        match self.focused {
            Field::Name => &mut self.name_text,
            Field::Command => &mut self.command_text,
            Field::WorkingDir => &mut self.working_dir_text,
            Field::ReadinessArg => &mut self.readiness_text,
        }
    }

    /// Apply a keystroke to the focused field.
    pub fn apply_key(&mut self, key: &str, characters: Option<&str>) -> KeyOutcome {
        let outcome = self.text_mut().apply_key(key, characters);
        if outcome == KeyOutcome::Changed {
            self.sync_from_texts();
        }
        outcome
    }

    /// Push the text buffers into the draft (and bump its revision).
    fn sync_from_texts(&mut self) {
        let name = self.name_text.text().to_string();
        let command = self.command_text.text().to_string();
        let working_dir = self.working_dir_text.text().to_string();
        let readiness_arg = self.readiness_text.text().to_string();
        self.draft.edit(|fields| {
            fields.name = name;
            fields.command = command;
            fields.working_dir = working_dir;
            fields.readiness_arg = readiness_arg;
        });
        self.error = None;
    }

    pub fn toggle_enabled(&mut self) {
        self.draft.edit(|fields| fields.enabled = !fields.enabled);
    }

    pub fn toggle_one_shot(&mut self) {
        self.draft.edit(|fields| fields.one_shot = !fields.one_shot);
        if self.draft.local().one_shot && self.draft.local().readiness != ReadinessChoice::Exit {
            // One-shot tasks must use exit readiness; switch it here rather than failing on save.
            self.set_readiness(ReadinessChoice::Exit);
        }
    }

    pub fn set_readiness(&mut self, choice: ReadinessChoice) {
        let arg = choice.default_arg();
        self.readiness_text.set_text(arg.clone());
        self.draft.edit(|fields| {
            fields.readiness = choice;
            fields.readiness_arg = arg;
        });
    }

    /// Validate locally, then return the payload for `CoreCommand::UpdateTask`.
    pub fn to_update_input(&self) -> Result<neohub_core::catalog::UpdateTaskInput, FieldError> {
        let fields = self.draft.local();
        if fields.name.trim().is_empty() {
            return Err(FieldError {
                field: Field::Name,
                message: "任务名称不能为空".into(),
            });
        }
        if fields.command.trim().is_empty() {
            return Err(FieldError {
                field: Field::Command,
                message: "命令或脚本不能为空".into(),
            });
        }
        let readiness = fields.readiness_signal().map_err(|message| FieldError {
            field: Field::ReadinessArg,
            message,
        })?;
        let command = fields.command.trim();
        // A command that is literally a package-manager script run stays a `script` task so the
        // spawn path keeps its argument-vector form; anything else is a custom shell command.
        let looks_like_script = command.starts_with("npm run ")
            || command.starts_with("pnpm run ")
            || command.starts_with("yarn ")
            || command.starts_with("bun run ");
        let command_kind = if looks_like_script {
            CommandKind::Script
        } else {
            CommandKind::Custom
        };
        let working_dir_override = if fields.working_dir.trim().is_empty() {
            None
        } else {
            Some(fields.working_dir.trim().to_string())
        };
        Ok(neohub_core::catalog::UpdateTaskInput {
            name: Some(fields.name.trim().to_string()),
            command_kind: Some(command_kind),
            script: None,
            custom_command: Some(Some(command.to_string())),
            working_dir_override: Some(working_dir_override),
            readiness: Some(readiness),
            one_shot: Some(fields.one_shot),
            enabled: Some(fields.enabled),
            ..Default::default()
        })
    }

    pub fn take_save(&self) -> Option<SaveRequest<TaskFormFields>> {
        self.draft.take_save()
    }

    /// The core rejected the write: show it on the field that caused it.
    pub fn apply_core_error(&mut self, message: &str) {
        let lowered = message.to_lowercase();
        let field = if lowered.contains("name") || lowered.contains("名称") {
            Field::Name
        } else if lowered.contains("command") || lowered.contains("命令") {
            Field::Command
        } else if lowered.contains("readiness") || lowered.contains("就绪") {
            Field::ReadinessArg
        } else if lowered.contains("working") || lowered.contains("目录") {
            Field::WorkingDir
        } else {
            self.focused
        };
        self.error = Some(FieldError {
            field,
            message: message.to_string(),
        });
        self.draft.reject(message);
    }

    /// A refreshed task arrived from the core (e.g. an MCP edit).
    pub fn apply_external(&mut self, task: &Task) -> ExternalOutcome {
        let outcome = self.draft.apply_external(TaskFormFields::from_task(task));
        if outcome == ExternalOutcome::Applied {
            let fields = self.draft.local().clone();
            self.name_text.set_text(fields.name);
            self.command_text.set_text(fields.command);
            self.working_dir_text.set_text(fields.working_dir);
            self.readiness_text.set_text(fields.readiness_arg);
        }
        outcome
    }

    /// The save succeeded: adopt the stored values as the new baseline.
    pub fn ack(&mut self, task: &Task, revision: u64) {
        self.draft.ack(TaskFormFields::from_task(task), revision);
        self.error = None;
    }
}

impl std::fmt::Debug for TaskForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskForm")
            .field("task", &self.task_id)
            .field("focused", &self.focused)
            .field("dirty", &self.is_dirty())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_domain::{AppId, TaskId};

    fn task() -> Task {
        Task {
            id: TaskId::from_existing("task1"),
            app_id: AppId::from_existing("app1"),
            name: "dev".into(),
            position: 0,
            command_kind: CommandKind::Custom,
            script: None,
            custom_command: Some("pnpm run dev".into()),
            working_dir_override: None,
            package_manager_override: None,
            node_version_pref_override: None,
            depends_on: vec![],
            readiness: ReadinessSignal::Port {
                port: 3000,
                host: None,
            },
            one_shot: false,
            enabled: true,
            env_overrides: Default::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn typing_marks_the_form_dirty_and_reaches_the_payload() {
        let mut form = TaskForm::new(&task());
        assert!(!form.is_dirty());
        form.focus(Field::Name);
        form.apply_key("x", Some("x"));
        assert!(form.is_dirty());
        form.focus(Field::Command);
        form.apply_key("y", Some("y"));
        let input = form.to_update_input().expect("valid");
        assert!(input.name.as_deref().unwrap().ends_with('x'));
        assert!(input.custom_command.unwrap().unwrap().ends_with('y'));
    }

    #[test]
    fn tab_cycles_through_the_fields() {
        let mut form = TaskForm::new(&task());
        assert_eq!(form.focused(), Field::Name);
        form.focus_next();
        assert_eq!(form.focused(), Field::Command);
        form.focus_next();
        assert_eq!(form.focused(), Field::WorkingDir);
        form.focus_next();
        assert_eq!(form.focused(), Field::ReadinessArg);
        form.focus_next();
        assert_eq!(form.focused(), Field::Name, "wraps around");
    }

    #[test]
    fn local_validation_points_at_the_field() {
        let mut form = TaskForm::new(&task());
        form.focus(Field::Name);
        // Clear the whole name.
        for _ in 0.."dev".len() {
            form.apply_key("backspace", None);
        }
        let error = form.to_update_input().expect_err("blank name");
        assert_eq!(error.field, Field::Name);

        let mut form = TaskForm::new(&task());
        form.set_readiness(ReadinessChoice::Port);
        form.focus(Field::ReadinessArg);
        for _ in 0..form.fields().readiness_arg.len() {
            form.apply_key("backspace", None);
        }
        form.apply_key("0", Some("0"));
        let error = form.to_update_input().expect_err("port 0 is invalid");
        assert_eq!(error.field, Field::ReadinessArg);
    }

    #[test]
    fn one_shot_forces_exit_readiness() {
        let mut form = TaskForm::new(&task());
        form.set_readiness(ReadinessChoice::Port);
        form.toggle_one_shot();
        assert!(form.fields().one_shot);
        assert_eq!(form.fields().readiness, ReadinessChoice::Exit);
        let input = form.to_update_input().expect("valid");
        assert!(matches!(
            input.readiness,
            Some(ReadinessSignal::Exit { code: Some(0) })
        ));
    }

    #[test]
    fn each_readiness_kind_round_trips_through_the_form() {
        for choice in ReadinessChoice::ALL {
            let mut form = TaskForm::new(&task());
            form.set_readiness(choice);
            let input = form.to_update_input().expect("defaults are valid");
            let signal = input.readiness.expect("readiness");
            assert_eq!(ReadinessChoice::from_signal(&signal), choice);
        }
    }

    #[test]
    fn a_core_error_is_attributed_to_the_right_field() {
        let mut form = TaskForm::new(&task());
        form.apply_core_error("name: task name cannot be empty");
        assert_eq!(form.error().map(|e| e.field), Some(Field::Name));
        form.apply_core_error("Task \"x\" depends on unknown task id: ghost");
        assert_eq!(form.error().map(|e| e.field), Some(form.focused()));
    }

    #[test]
    fn a_refresh_does_not_overwrite_typing() {
        let mut form = TaskForm::new(&task());
        form.focus(Field::Command);
        form.apply_key("!", Some("!"));
        let mut external = task();
        external.name = "renamed-by-mcp".into();
        let outcome = form.apply_external(&external);
        assert_eq!(outcome, ExternalOutcome::PreservedLocal);
        assert!(form.fields().command.ends_with('!'));
        assert_eq!(
            form.conflict().map(|c| c.name.as_str()),
            Some("renamed-by-mcp")
        );
    }

    #[test]
    fn a_clean_form_follows_refreshes() {
        let mut form = TaskForm::new(&task());
        let mut external = task();
        external.name = "renamed".into();
        assert_eq!(form.apply_external(&external), ExternalOutcome::Applied);
        assert_eq!(form.fields().name, "renamed");
    }

    #[test]
    fn acknowledging_a_save_clears_the_dirty_flag() {
        let mut form = TaskForm::new(&task());
        form.focus(Field::Name);
        form.apply_key("z", Some("z"));
        let save = form.take_save().expect("pending");
        let mut stored = task();
        stored.name = save.payload.name.clone();
        form.ack(&stored, save.revision);
        assert!(!form.is_dirty());
    }
}
