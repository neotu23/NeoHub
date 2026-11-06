//! The four editing guarantees the plan calls out, exercised against a real core/database so
//! the revisions and errors are the real ones (not mocks).
//!
//! * `close_flushes_pending_edits`
//! * `save_failure_keeps_window_and_input`
//! * `mcp_refresh_preserves_active_edit`
//! * `stale_save_does_not_overwrite_newer_revision`

use neohub_core::catalog::{AddTaskInput, CreateAppInput, CreateTaskSpec, UpdateTaskInput};
use neohub_core::service::CoreService;
use neohub_domain::{AppId, CommandKind, Task, TaskId};
use neohub_storage::DataDir;
use neohub_ui::state::edit_session::{CloseOutcome, Draft, ExternalOutcome, SaveBarrier};
use std::sync::Arc;

/// A small harness: a real core with one synthetic app and one task.
struct Harness {
    _dir: tempfile::TempDir,
    /// Keeps the reactor alive: `CoreService::open` spawns its pumps.
    _runtime: tokio::runtime::Runtime,
    service: Arc<CoreService>,
    app: AppId,
    task: TaskId,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let service = {
            let _enter = runtime.enter();
            CoreService::open(DataDir::new(dir.path())).expect("service")
        };
        let project = dir.path().join("synthetic-project");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.to_string_lossy().into_owned();
        let (app, task) = service
            .storage_call(move |storage| {
                let catalog = neohub_core::AppCatalog::new(storage);
                let app = catalog
                    .create_app(CreateAppInput {
                        path,
                        name: Some("demo".into()),
                        node_version_pref: None,
                        package_manager: None,
                        default_script: None,
                        first_task: Some(CreateTaskSpec {
                            name: "dev".into(),
                            command_kind: Some(CommandKind::Custom),
                            script: None,
                            custom_command: Some("echo dev".into()),
                            working_dir_override: None,
                        }),
                        tasks: vec![],
                        env_vars: vec![],
                    })
                    .expect("create app");
                let task = catalog.list_tasks(&app.id).expect("tasks").remove(0).id;
                (app.id, task)
            })
            .expect("storage");
        Harness {
            _dir: dir,
            _runtime: runtime,
            service,
            app,
            task,
        }
    }

    fn task_snapshot(&self, task: &TaskId) -> Task {
        self.service
            .storage_call(|storage| {
                neohub_core::AppCatalog::new(storage)
                    .list_tasks(&self.app)
                    .expect("tasks")
                    .into_iter()
                    .find(|candidate| &candidate.id == task)
                    .expect("task")
            })
            .expect("storage")
    }

    /// What the UI's save path does: apply the draft to the core, returning the stored row.
    fn save_task(&self, task: &TaskId, name: &str, command: &str) -> Result<Task, String> {
        self.service
            .storage_call(|storage| {
                let catalog = neohub_core::AppCatalog::new(storage);
                catalog.update_task(
                    task,
                    UpdateTaskInput {
                        name: Some(name.to_string()),
                        custom_command: Some(Some(command.to_string())),
                        ..Default::default()
                    },
                )
            })
            .expect("storage")
            .map_err(|err| err.to_string())
    }

    /// Simulate an external mutation (MCP) that changes the same task.
    fn external_edit(&self, task: &TaskId, name: &str) -> Task {
        self.service
            .storage_call(|storage| {
                let catalog = neohub_core::AppCatalog::new(storage);
                catalog.list_tasks(&self.app).expect("tasks");
                catalog
                    .add_task(
                        &self.app,
                        AddTaskInput {
                            name: "from-mcp".into(),
                            command_kind: Some(CommandKind::Custom),
                            custom_command: Some("echo mcp".into()),
                            ..Default::default()
                        },
                    )
                    .expect("add task");
                catalog
                    .update_task(
                        task,
                        UpdateTaskInput {
                            name: Some(name.to_string()),
                            ..Default::default()
                        },
                    )
                    .expect("external update")
            })
            .expect("storage")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TaskDraft {
    name: String,
    command: String,
}

impl TaskDraft {
    fn from_task(task: &Task) -> Self {
        TaskDraft {
            name: task.name.clone(),
            command: task.custom_command.clone().unwrap_or_default(),
        }
    }
}

#[test]
fn close_flushes_pending_edits() {
    let harness = Harness::new();
    let task = harness.task_snapshot(&harness.task);
    let mut draft = Draft::new(harness.task.to_string(), TaskDraft::from_task(&task));
    draft.edit(|fields| fields.command = "echo changed".into());

    let barrier = SaveBarrier::new();
    let pending = match barrier.prepare_close(&[&draft]) {
        CloseOutcome::Pending(saves) => saves,
        _ => panic!("an edited draft must produce a pending save"),
    };
    assert_eq!(pending.len(), 1);

    // The window's save path runs, then acknowledges with the revision it sent.
    for save in &pending {
        let stored = harness
            .save_task(&harness.task, &save.payload.name, &save.payload.command)
            .expect("save");
        draft.ack(TaskDraft::from_task(&stored), save.revision);
    }
    assert!(
        !draft.is_dirty(),
        "a completed flush leaves the draft clean"
    );
    assert!(matches!(
        barrier.prepare_close(&[&draft]),
        CloseOutcome::Ready
    ));
    assert_eq!(
        harness
            .task_snapshot(&harness.task)
            .custom_command
            .as_deref(),
        Some("echo changed"),
        "the edit reached the database before closing"
    );
}

#[test]
fn save_failure_keeps_window_and_input() {
    let harness = Harness::new();
    let task = harness.task_snapshot(&harness.task);
    let mut draft = Draft::new(harness.task.to_string(), TaskDraft::from_task(&task));
    // An empty task name is rejected by the core (field-level validation).
    draft.edit(|fields| {
        fields.name = "   ".into();
        fields.command = "echo keep-me".into();
    });

    let mut barrier = SaveBarrier::new();
    let pending = match barrier.prepare_close(&[&draft]) {
        CloseOutcome::Pending(saves) => saves,
        _ => panic!("pending"),
    };
    let error = harness
        .save_task(
            &harness.task,
            &pending[0].payload.name,
            &pending[0].payload.command,
        )
        .expect_err("the core must reject a blank name");
    draft.reject(&error);
    barrier.record_failure(harness.task.to_string(), &error);

    match barrier.prepare_close(&[&draft]) {
        CloseOutcome::Blocked(reasons) => assert!(!reasons.is_empty()),
        _ => panic!("a failed save must keep the window open"),
    }
    assert!(draft.is_dirty(), "the draft stays dirty");
    assert_eq!(draft.local().command, "echo keep-me", "the text survives");
    assert_eq!(draft.local().name, "   ");
    assert!(draft.error().is_some());
    assert_eq!(
        harness.task_snapshot(&harness.task).name,
        "dev",
        "the database was not touched by the failed save"
    );
}

#[test]
fn mcp_refresh_preserves_active_edit() {
    let harness = Harness::new();
    let task = harness.task_snapshot(&harness.task);
    let mut draft = Draft::new(harness.task.to_string(), TaskDraft::from_task(&task));

    // The user starts typing.
    draft.edit(|fields| fields.command = "echo my-edit".into());
    // Meanwhile MCP renames the task; a refreshed snapshot carries the new row.
    let external = harness.external_edit(&harness.task, "renamed-by-mcp");
    let outcome = draft.apply_external(TaskDraft::from_task(&external));

    assert_eq!(outcome, ExternalOutcome::PreservedLocal);
    assert_eq!(
        draft.local().command,
        "echo my-edit",
        "the in-flight edit is not overwritten by the snapshot"
    );
    assert_eq!(
        draft.conflict().map(|conflict| conflict.name.as_str()),
        Some("renamed-by-mcp"),
        "the external value is offered as a conflict instead of being lost"
    );
    assert!(draft.is_dirty());
}

#[test]
fn stale_save_does_not_overwrite_newer_revision() {
    let harness = Harness::new();
    let task = harness.task_snapshot(&harness.task);
    let mut draft = Draft::new(harness.task.to_string(), TaskDraft::from_task(&task));

    draft.edit(|fields| fields.command = "echo first".into());
    let first_save = match SaveBarrier::new().prepare_close(&[&draft]) {
        CloseOutcome::Pending(mut saves) => saves.remove(0),
        _ => panic!("pending"),
    };

    // The user keeps typing before the save lands.
    draft.edit(|fields| fields.command = "echo second".into());

    // The first save completes and is acknowledged with its (now stale) revision.
    let stored = harness
        .save_task(
            &harness.task,
            &first_save.payload.name,
            &first_save.payload.command,
        )
        .expect("stale save still succeeds");
    draft.ack(TaskDraft::from_task(&stored), first_save.revision);

    assert!(
        draft.is_dirty(),
        "the newer edit must still be pending after the stale save"
    );
    assert_eq!(draft.local().command, "echo second");

    // The follow-up flush sends the newer text.
    let next = match SaveBarrier::new().prepare_close(&[&draft]) {
        CloseOutcome::Pending(mut saves) => saves.remove(0),
        _ => panic!("a follow-up save is required"),
    };
    assert_eq!(next.payload.command, "echo second");
    assert!(next.revision > first_save.revision);

    let stored = harness
        .save_task(&harness.task, &next.payload.name, &next.payload.command)
        .expect("follow-up save");
    draft.ack(TaskDraft::from_task(&stored), next.revision);
    assert!(!draft.is_dirty());
    assert_eq!(
        harness
            .task_snapshot(&harness.task)
            .custom_command
            .as_deref(),
        Some("echo second"),
        "the newest text is what the database holds"
    );
}
