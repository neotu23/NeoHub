//! Catalog business rules: path normalisation, atomic app+task+env creation, task
//! validation (unique names, one-shot/readiness pairing, dependency cycles).

mod graph;

pub use graph::find_cycle;

use neohub_domain::{
    App, AppId, CommandKind, DomainError, EnvVar, NodeVersionPref, PackageManager, ReadinessSignal,
    Task, TaskId,
};
use neohub_storage::{AppPatch, NewApp, NewTask, Storage, StorageError, TaskPatch};

pub type CoreResult<T> = std::result::Result<T, CoreError>;

/// Core-level error: domain rules plus the storage failures worth surfacing verbatim.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// One task to create as part of the add-app flow.
#[derive(Clone, Debug, Default)]
pub struct CreateTaskSpec {
    pub name: String,
    pub command_kind: Option<CommandKind>,
    pub script: Option<String>,
    pub custom_command: Option<String>,
    pub working_dir_override: Option<String>,
}

/// Atomic add-app payload: app + first task + extra tasks + env vars, one transaction.
#[derive(Clone, Debug)]
pub struct CreateAppInput {
    pub path: String,
    pub name: Option<String>,
    pub node_version_pref: Option<NodeVersionPref>,
    pub package_manager: Option<PackageManager>,
    pub default_script: Option<String>,
    pub first_task: Option<CreateTaskSpec>,
    pub tasks: Vec<CreateTaskSpec>,
    pub env_vars: Vec<(String, String, bool)>,
}

/// A new task inside an existing app.
#[derive(Clone, Debug, Default)]
pub struct AddTaskInput {
    pub name: String,
    pub command_kind: Option<CommandKind>,
    pub script: Option<String>,
    pub custom_command: Option<String>,
    pub working_dir_override: Option<String>,
    pub package_manager_override: Option<PackageManager>,
    pub node_version_pref_override: Option<NodeVersionPref>,
    pub depends_on: Vec<TaskId>,
    pub readiness: Option<ReadinessSignal>,
    pub one_shot: bool,
    pub enabled: Option<bool>,
}

/// A partial task update; `None` fields keep their current value.
#[derive(Clone, Debug, Default)]
pub struct UpdateTaskInput {
    pub name: Option<String>,
    pub command_kind: Option<CommandKind>,
    pub script: Option<Option<String>>,
    pub custom_command: Option<Option<String>>,
    pub working_dir_override: Option<Option<String>>,
    pub package_manager_override: Option<Option<PackageManager>>,
    pub node_version_pref_override: Option<Option<NodeVersionPref>>,
    pub depends_on: Option<Vec<TaskId>>,
    pub readiness: Option<ReadinessSignal>,
    pub one_shot: Option<bool>,
    pub enabled: Option<bool>,
}

/// Partial app update. Identity fields are deliberately unrepresentable.
#[derive(Clone, Debug, Default)]
pub struct UpdateAppInput {
    pub name: Option<String>,
    pub color: Option<String>,
    pub icon: Option<Option<String>>,
    pub node_version_pref: Option<NodeVersionPref>,
    pub package_manager: Option<Option<PackageManager>>,
    pub default_script: Option<Option<String>>,
    pub custom_command: Option<Option<String>>,
    pub working_dir: Option<String>,
    pub auto_restart_on_change: Option<bool>,
    pub auto_start: Option<bool>,
    pub watch_globs: Option<Vec<String>>,
    pub port_hint: Option<Option<u16>>,
    pub tags: Option<Vec<String>>,
    pub folder: Option<Option<String>>,
}

/// Catalog facade over one open `Storage`.
pub struct AppCatalog<'s> {
    storage: &'s Storage,
}

impl<'s> AppCatalog<'s> {
    pub fn new(storage: &'s Storage) -> Self {
        AppCatalog { storage }
    }

    pub fn storage(&self) -> &'s Storage {
        self.storage
    }

    // ---- apps ---------------------------------------------------------------

    pub fn list_apps(&self) -> CoreResult<Vec<App>> {
        Ok(self.storage.catalog().list_apps()?)
    }

    pub fn get_app(&self, id: &AppId) -> CoreResult<App> {
        self.storage.catalog().get_app(id)?.ok_or_else(|| {
            DomainError::NotFound {
                entity: "app",
                id: id.to_string(),
            }
            .into()
        })
    }

    pub fn get_app_by_path(&self, path: &str) -> CoreResult<Option<App>> {
        Ok(self.storage.catalog().get_app_by_path(path)?)
    }

    /// Create an app with its tasks and environment variables in ONE transaction.
    ///
    /// Any failure (bad path, duplicate path, duplicate task name, dependency cycle,
    /// env key clash) rolls the whole thing back — a half-created app is never
    /// observable, matching the legacy `apps:create` contract.
    pub fn create_app(&self, input: CreateAppInput) -> CoreResult<App> {
        let path = canonicalise_dir(&input.path, "path")?;
        if let Some(existing) = self.storage.catalog().get_app_by_path(&path)? {
            return Err(DomainError::DuplicatePath {
                path,
                existing: existing.id,
            }
            .into());
        }

        let now = neohub_storage::db::now_ms();
        let app_id = AppId::new();
        let name = input
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| default_app_name(&path));
        let color = next_color(self.storage.catalog().list_apps()?.len());

        let specs: Vec<CreateTaskSpec> = input.first_task.into_iter().chain(input.tasks).collect();

        let tx = self
            .storage
            .conn()
            .unchecked_transaction()
            .map_err(StorageError::from)?;
        {
            let cat = neohub_storage::Catalog::new(&tx);
            cat.insert_app(&NewApp {
                id: app_id.clone(),
                name,
                path: path.clone(),
                color,
                icon: None,
                node_version_pref: input.node_version_pref.unwrap_or_default(),
                package_manager: input.package_manager,
                default_script: input.default_script,
                custom_command: None,
                working_dir: path.clone(),
                auto_restart_on_change: false,
                auto_start: false,
                watch_globs: Vec::new(),
                port_hint: None,
                tags: Vec::new(),
                folder: None,
                created_at: now,
                updated_at: now,
            })?;

            let mut drafts: Vec<Task> = Vec::new();
            for (index, spec) in specs.iter().enumerate() {
                let task = build_task(&app_id, spec, index as i64, &drafts, now)?;
                // Same validation as add_task, so an atomic create cannot smuggle in a
                // duplicate name, a self-dependency or a one-shot task without exit readiness.
                validate_task(&task, &drafts, None)?;
                cat.insert_task(&to_new_task(&task))?;
                drafts.push(task);
            }

            if !input.env_vars.is_empty() {
                let vars: Vec<EnvVar> = input
                    .env_vars
                    .iter()
                    .filter(|(key, _, _)| !key.trim().is_empty())
                    .map(|(key, value, is_secret)| {
                        neohub_storage::env::new_var(
                            Some(app_id.clone()),
                            None,
                            key,
                            value,
                            *is_secret,
                        )
                    })
                    .collect();
                let mut env = neohub_storage::EnvStore::new(&tx);
                env.set_app(&app_id, &vars, now)?;
            }
        }
        tx.commit().map_err(StorageError::from)?;

        self.get_app(&app_id)
    }

    /// Patch an app. Identity (`id`, `path`, `created_at`) is immutable by construction.
    pub fn update_app(&self, id: &AppId, patch: UpdateAppInput) -> CoreResult<App> {
        let current = self.get_app(id)?;
        let working_dir = match patch.working_dir {
            Some(ref dir) if dir != &current.working_dir => canonicalise_dir(dir, "workingDir")?,
            Some(dir) => dir,
            None => current.working_dir.clone(),
        };

        let updated = AppPatch {
            name: patch
                .name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_owned)
                .unwrap_or(current.name),
            color: patch.color.unwrap_or(current.color),
            icon: patch.icon.unwrap_or(current.icon),
            node_version_pref: patch.node_version_pref.unwrap_or(current.node_version_pref),
            package_manager: patch.package_manager.unwrap_or(current.package_manager),
            default_script: patch.default_script.unwrap_or(current.default_script),
            custom_command: patch.custom_command.unwrap_or(current.custom_command),
            working_dir,
            auto_restart_on_change: patch
                .auto_restart_on_change
                .unwrap_or(current.auto_restart_on_change),
            auto_start: patch.auto_start.unwrap_or(current.auto_start),
            watch_globs: patch.watch_globs.unwrap_or(current.watch_globs),
            port_hint: patch.port_hint.unwrap_or(current.port_hint),
            tags: patch.tags.unwrap_or(current.tags),
            folder: normalise_folder(patch.folder.unwrap_or(current.folder)),
            last_started_at: current.last_started_at,
            last_exit_code: current.last_exit_code,
            updated_at: neohub_storage::db::now_ms(),
        };
        Ok(self.storage.catalog().update_app(id, &updated)?)
    }

    /// Remove an app registration. **Never** touches the project directory on disk.
    /// Callers that know about running processes (the core service) must refuse while the
    /// app is live; that guard lives in phase 5.
    pub fn remove_app(&self, id: &AppId) -> CoreResult<()> {
        Ok(self.storage.catalog().remove_app(id)?)
    }

    // ---- tasks ---------------------------------------------------------------

    pub fn list_tasks(&self, app_id: &AppId) -> CoreResult<Vec<Task>> {
        Ok(self.storage.catalog().list_tasks(app_id)?)
    }

    pub fn list_all_tasks(&self) -> CoreResult<Vec<Task>> {
        Ok(self.storage.catalog().list_all_tasks()?)
    }

    pub fn add_task(&self, app_id: &AppId, input: AddTaskInput) -> CoreResult<Task> {
        let existing = self.storage.catalog().list_tasks(app_id)?;
        let now = neohub_storage::db::now_ms();
        let spec = CreateTaskSpec {
            name: input.name,
            command_kind: input.command_kind,
            script: input.script,
            custom_command: input.custom_command,
            working_dir_override: input.working_dir_override,
        };
        let mut draft = build_task(app_id, &spec, existing.len() as i64, &existing, now)?;
        draft.package_manager_override = input.package_manager_override;
        draft.node_version_pref_override = input.node_version_pref_override;
        draft.depends_on = input.depends_on;
        draft.readiness = input.readiness.unwrap_or_default();
        draft.one_shot = input.one_shot;
        draft.enabled = input.enabled.unwrap_or(true);

        validate_task(&draft, &existing, None)?;

        let tx = self
            .storage
            .conn()
            .unchecked_transaction()
            .map_err(StorageError::from)?;
        neohub_storage::Catalog::new(&tx).insert_task(&to_new_task(&draft))?;
        tx.commit().map_err(StorageError::from)?;
        Ok(draft)
    }

    pub fn update_task(&self, task_id: &TaskId, input: UpdateTaskInput) -> CoreResult<Task> {
        let current = self.get_task(task_id)?;
        let siblings = self.storage.catalog().list_tasks(&current.app_id)?;

        let mut next = current.clone();
        if let Some(name) = input.name {
            next.name = name;
        }
        if let Some(kind) = input.command_kind {
            next.command_kind = kind;
        }
        if let Some(script) = input.script {
            next.script = script;
        }
        if let Some(command) = input.custom_command {
            next.custom_command = command;
        }
        if let Some(dir) = input.working_dir_override {
            next.working_dir_override = dir;
        }
        if let Some(pm) = input.package_manager_override {
            next.package_manager_override = pm;
        }
        if let Some(pref) = input.node_version_pref_override {
            next.node_version_pref_override = pref;
        }
        if let Some(deps) = input.depends_on {
            next.depends_on = deps;
        }
        if let Some(readiness) = input.readiness {
            next.readiness = readiness;
        }
        if let Some(one_shot) = input.one_shot {
            next.one_shot = one_shot;
        }
        if let Some(enabled) = input.enabled {
            next.enabled = enabled;
        }
        next.updated_at = neohub_storage::db::now_ms();

        validate_task(&next, &siblings, Some(task_id))?;

        let patch = TaskPatch {
            name: next.name.clone(),
            position: next.position,
            command_kind: next.command_kind,
            script: next.script.clone(),
            custom_command: next.custom_command.clone(),
            working_dir_override: next.working_dir_override.clone(),
            package_manager_override: next.package_manager_override,
            node_version_pref_override: next.node_version_pref_override.clone(),
            depends_on: next.depends_on.clone(),
            readiness: next.readiness.clone(),
            one_shot: next.one_shot,
            enabled: next.enabled,
            updated_at: next.updated_at,
        };
        Ok(self.storage.catalog().update_task(task_id, &patch)?)
    }

    pub fn get_task(&self, task_id: &TaskId) -> CoreResult<Task> {
        self.storage.catalog().get_task(task_id)?.ok_or_else(|| {
            DomainError::NotFound {
                entity: "task",
                id: task_id.to_string(),
            }
            .into()
        })
    }

    /// Delete a task unless another task in the same app depends on it. Dependents are
    /// never silently rewritten (that would change start semantics behind the user's back).
    pub fn remove_task(&self, task_id: &TaskId) -> CoreResult<()> {
        let task = self.get_task(task_id)?;
        let dependents = self
            .storage
            .catalog()
            .dependents_of(&task.app_id, task_id)?;
        if !dependents.is_empty() {
            return Err(DomainError::TaskHasDependents {
                name: task.name,
                dependents: dependents.into_iter().map(|t| t.name).collect(),
            }
            .into());
        }
        Ok(self.storage.catalog().remove_task(task_id)?)
    }

    pub fn reorder_tasks(&self, app_id: &AppId, task_ids: &[TaskId]) -> CoreResult<()> {
        let existing: Vec<TaskId> = self
            .storage
            .catalog()
            .list_tasks(app_id)?
            .into_iter()
            .map(|t| t.id)
            .collect();
        let mut sorted = task_ids.to_vec();
        sorted.sort_by_key(|id| id.as_str().to_owned());
        let mut known = existing.clone();
        known.sort_by_key(|id| id.as_str().to_owned());
        if sorted != known {
            return Err(DomainError::InvalidInput {
                field: "taskIds",
                message: "reorder must list every task of the app exactly once".into(),
            }
            .into());
        }
        let tx = self
            .storage
            .conn()
            .unchecked_transaction()
            .map_err(StorageError::from)?;
        neohub_storage::Catalog::new(&tx).reorder_tasks(
            app_id,
            task_ids,
            neohub_storage::db::now_ms(),
        )?;
        tx.commit().map_err(StorageError::from)?;
        Ok(())
    }

    // ---- folders -------------------------------------------------------------

    pub fn list_folders(&self) -> CoreResult<Vec<String>> {
        Ok(self.storage.catalog().list_folders()?)
    }

    pub fn rename_folder(&self, from: &str, to: &str) -> CoreResult<usize> {
        let dest = normalise_folder(Some(to.to_string()));
        Ok(self.storage.catalog().rename_folder(
            from,
            dest.as_deref(),
            neohub_storage::db::now_ms(),
        )?)
    }

    pub fn clear_folder(&self, name: &str) -> CoreResult<usize> {
        Ok(self
            .storage
            .catalog()
            .clear_folder(name, neohub_storage::db::now_ms())?)
    }
}

// ---- helpers ---------------------------------------------------------------

/// Palette used to colour new apps, in the legacy order.
const PALETTE: [&str; 12] = [
    "#7aa2f7", "#bb9af7", "#f7768e", "#e0af68", "#9ece6a", "#73daca", "#7dcfff", "#ff9e64",
    "#c0caf5", "#f4b8e4", "#a6e3a1", "#fab387",
];

fn next_color(existing_apps: usize) -> String {
    PALETTE[existing_apps % PALETTE.len()].to_string()
}

fn default_app_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Resolve to an existing, canonical directory — the same realpath + isDirectory check the
/// legacy registry applied to both `path` and `working_dir`.
fn canonicalise_dir(raw: &str, field: &'static str) -> CoreResult<String> {
    let path = std::path::Path::new(raw);
    let canonical = std::fs::canonicalize(path).map_err(|_| DomainError::PathMissing {
        path: raw.to_string(),
    })?;
    let meta = std::fs::metadata(&canonical).map_err(|_| DomainError::PathMissing {
        path: raw.to_string(),
    })?;
    if !meta.is_dir() {
        return Err(DomainError::InvalidInput {
            field,
            message: format!("not a directory: {raw}"),
        }
        .into());
    }
    Ok(canonical.to_string_lossy().into_owned())
}

/// Trim, cap at 60 chars, treat empty as "(Ungrouped)".
fn normalise_folder(value: Option<String>) -> Option<String> {
    let raw = value?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(60).collect())
}

fn build_task(
    app_id: &AppId,
    spec: &CreateTaskSpec,
    position: i64,
    existing: &[Task],
    now: i64,
) -> CoreResult<Task> {
    let name = spec.name.trim();
    let name = if name.is_empty() {
        format!("task-{}", existing.len() + 1)
    } else {
        name.to_string()
    };
    let command_kind = spec.command_kind.unwrap_or(CommandKind::Script);
    Ok(Task {
        id: TaskId::new(),
        app_id: app_id.clone(),
        name,
        position,
        command_kind,
        script: spec.script.clone(),
        custom_command: spec.custom_command.clone(),
        working_dir_override: spec.working_dir_override.clone(),
        package_manager_override: None,
        node_version_pref_override: None,
        depends_on: Vec::new(),
        readiness: ReadinessSignal::None,
        one_shot: false,
        enabled: true,
        env_overrides: Default::default(),
        created_at: now,
        updated_at: now,
    })
}

/// Validate one task against its siblings (the `replace` task, if any, is excluded from
/// the "existing" set so an update can keep its own name/deps).
fn validate_task(draft: &Task, siblings: &[Task], replace: Option<&TaskId>) -> CoreResult<()> {
    if draft.name.trim().is_empty() {
        return Err(DomainError::InvalidInput {
            field: "name",
            message: "task name cannot be empty".into(),
        }
        .into());
    }
    if let Some(conflict) = siblings
        .iter()
        .find(|t| t.id != draft.id && Some(&t.id) != replace && t.name == draft.name)
    {
        return Err(DomainError::DuplicateTaskName {
            name: conflict.name.clone(),
        }
        .into());
    }
    if draft.one_shot && draft.readiness.kind() != "exit" {
        return Err(DomainError::OneShotRequiresExitReadiness.into());
    }

    let others: Vec<Task> = siblings
        .iter()
        .filter(|t| t.id != draft.id && Some(&t.id) != replace)
        .cloned()
        .collect();
    let known: std::collections::BTreeSet<TaskId> = others.iter().map(|t| t.id.clone()).collect();
    for dep in &draft.depends_on {
        if dep == &draft.id {
            return Err(DomainError::SelfDependency {
                task_id: draft.id.clone(),
            }
            .into());
        }
        if !known.contains(dep) {
            return Err(DomainError::UnknownDependency {
                task_id: draft.id.clone(),
                missing: dep.clone(),
            }
            .into());
        }
    }

    let mut nodes: Vec<TaskId> = others.iter().map(|t| t.id.clone()).collect();
    nodes.push(draft.id.clone());
    let mut deps: std::collections::BTreeMap<TaskId, Vec<TaskId>> = others
        .iter()
        .map(|t| (t.id.clone(), t.depends_on.clone()))
        .collect();
    deps.insert(draft.id.clone(), draft.depends_on.clone());
    if let Some(cycle) = find_cycle(&nodes, &deps) {
        return Err(DomainError::DependencyCycle { tasks: cycle }.into());
    }
    Ok(())
}

fn to_new_task(task: &Task) -> NewTask {
    NewTask {
        id: task.id.clone(),
        app_id: task.app_id.clone(),
        name: task.name.clone(),
        position: task.position,
        command_kind: task.command_kind,
        script: task.script.clone(),
        custom_command: task.custom_command.clone(),
        working_dir_override: task.working_dir_override.clone(),
        package_manager_override: task.package_manager_override,
        node_version_pref_override: task.node_version_pref_override.clone(),
        depends_on: task.depends_on.clone(),
        readiness: task.readiness.clone(),
        one_shot: task.one_shot,
        enabled: task.enabled,
        env_overrides: task.env_overrides.clone(),
        created_at: task.created_at,
        updated_at: task.updated_at,
    }
}
