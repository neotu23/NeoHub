//! Navigation session: what the window was showing, persisted across window destruction.
//!
//! The Electron panel kept this in `localStorage` (`neohub:panel-session:v1`). The Rust build
//! keeps the same information in `session.json` inside the data directory — the window can be
//! destroyed and recreated while the core keeps running, so the session must outlive it.
//!
//! Rules (all tested): a corrupt file falls back to the default, a selection pointing at a
//! deleted app falls back to the dashboard, and a selected task that no longer exists is
//! dropped instead of rendering an empty detail pane.

use neohub_domain::{App, AppId, Task, TaskId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Which top-level view is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    Dashboard,
    App,
}

/// Sidebar/dashboard sort order (mirrors `sortApps.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppSort {
    Name,
    Recent,
    Running,
}

/// Sidebar grouping mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupBy {
    Folder,
    Tag,
}

/// The persisted navigation state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub view: View,
    pub selected_app_id: Option<AppId>,
    /// Last selected task tab per app.
    pub selected_task_by_app: BTreeMap<AppId, TaskId>,
    pub sort: AppSort,
    pub group_by: GroupBy,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            view: View::Dashboard,
            selected_app_id: None,
            selected_task_by_app: BTreeMap::new(),
            sort: AppSort::Name,
            group_by: GroupBy::Folder,
        }
    }
}

impl Session {
    /// Drop references that no longer resolve. Called after every snapshot.
    pub fn reconcile(&mut self, apps: &[App], tasks_by_app: &BTreeMap<AppId, Vec<Task>>) {
        if let Some(selected) = self.selected_app_id.clone()
            && !apps.iter().any(|app| app.id == selected)
        {
            self.selected_app_id = None;
            self.view = View::Dashboard;
        }
        if self.view == View::App && self.selected_app_id.is_none() {
            self.view = View::Dashboard;
        }
        // Forget task selections for apps that are gone, and tasks that no longer exist.
        self.selected_task_by_app
            .retain(|app_id, task_id| match tasks_by_app.get(app_id) {
                Some(tasks) => tasks.iter().any(|task| &task.id == task_id),
                None => false,
            });
    }

    /// Select an app (and remember the task tab chosen for it, if any).
    pub fn select_app(&mut self, app_id: AppId, task_id: Option<TaskId>) {
        self.selected_app_id = Some(app_id.clone());
        self.view = View::App;
        if let Some(task_id) = task_id {
            self.selected_task_by_app.insert(app_id, task_id);
        }
    }

    pub fn go_dashboard(&mut self) {
        self.view = View::Dashboard;
    }
}

/// File-backed session with atomic writes.
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        SessionStore {
            path: data_dir.as_ref().join("session.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load, falling back to the default on any problem (missing, unreadable, corrupt).
    pub fn load(&self) -> Session {
        let Ok(raw) = std::fs::read_to_string(&self.path) else {
            return Session::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    /// Write via a temporary file + rename so a crash cannot leave a half-written session.
    pub fn save(&self, session: &Session) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(session)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, json)?;
        std::fs::rename(&temporary, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_domain::{CommandKind, NodeVersionPref, ReadinessSignal};

    fn app(id: &str, name: &str, folder: Option<&str>) -> App {
        App {
            id: AppId::from_existing(id),
            name: name.into(),
            path: format!("/tmp/{name}"),
            color: "#fff".into(),
            icon: None,
            node_version_pref: NodeVersionPref::Auto,
            package_manager: None,
            default_script: Some("dev".into()),
            custom_command: None,
            working_dir: format!("/tmp/{name}"),
            auto_restart_on_change: false,
            auto_start: false,
            watch_globs: vec![],
            port_hint: None,
            tags: vec![],
            folder: folder.map(str::to_string),
            last_started_at: None,
            last_exit_code: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn task(app: &str, id: &str) -> Task {
        Task {
            id: TaskId::from_existing(id),
            app_id: AppId::from_existing(app),
            name: id.into(),
            position: 0,
            command_kind: CommandKind::Script,
            script: Some("dev".into()),
            custom_command: None,
            working_dir_override: None,
            package_manager_override: None,
            node_version_pref_override: None,
            depends_on: vec![],
            readiness: ReadinessSignal::None,
            one_shot: false,
            enabled: true,
            env_overrides: Default::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn a_session_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path());
        let mut session = Session::default();
        session.select_app(
            AppId::from_existing("app1"),
            Some(TaskId::from_existing("task1")),
        );
        session.sort = AppSort::Recent;
        session.group_by = GroupBy::Tag;
        store.save(&session).expect("save");
        assert_eq!(store.load(), session);
    }

    #[test]
    fn a_corrupt_or_missing_file_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path());
        assert_eq!(store.load(), Session::default(), "missing file");
        std::fs::write(store.path(), "{not json").unwrap();
        assert_eq!(store.load(), Session::default(), "corrupt file");
    }

    #[test]
    fn a_deleted_app_drops_the_selection_and_returns_to_the_dashboard() {
        let mut session = Session::default();
        session.select_app(AppId::from_existing("app1"), None);
        let apps = vec![app("app2", "other", None)];
        session.reconcile(&apps, &BTreeMap::new());
        assert_eq!(session.selected_app_id, None);
        assert_eq!(session.view, View::Dashboard);
    }

    #[test]
    fn a_deleted_task_is_forgotten_but_the_app_selection_survives() {
        let app_id = AppId::from_existing("app1");
        let mut session = Session::default();
        session.select_app(app_id.clone(), Some(TaskId::from_existing("gone")));
        let apps = vec![app("app1", "demo", None)];
        let mut tasks = BTreeMap::new();
        tasks.insert(app_id.clone(), vec![task("app1", "still-here")]);
        session.reconcile(&apps, &tasks);
        assert_eq!(session.selected_app_id, Some(app_id.clone()));
        assert!(!session.selected_task_by_app.contains_key(&app_id));
        assert_eq!(session.view, View::App);
    }

    #[test]
    fn a_restored_selection_keeps_the_task_tab() {
        let app_id = AppId::from_existing("app1");
        let mut session = Session::default();
        session.select_app(app_id.clone(), Some(TaskId::from_existing("task1")));
        let apps = vec![app("app1", "demo", None)];
        let mut tasks = BTreeMap::new();
        tasks.insert(app_id.clone(), vec![task("app1", "task1")]);
        session.reconcile(&apps, &tasks);
        assert_eq!(
            session
                .selected_task_by_app
                .get(&app_id)
                .map(|t| t.as_str()),
            Some("task1")
        );
        assert_eq!(session.view, View::App);
    }

    #[test]
    fn going_back_to_the_dashboard_keeps_the_selection_for_next_time() {
        let app_id = AppId::from_existing("app1");
        let mut session = Session::default();
        session.select_app(app_id.clone(), None);
        session.go_dashboard();
        assert_eq!(session.view, View::Dashboard);
        assert_eq!(session.selected_app_id, Some(app_id));
    }
}
