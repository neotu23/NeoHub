//! The view model: a pure projection of core snapshots/events, plus navigation state.
//!
//! Nothing here touches GPUI, the database or the process table — it takes a `CoreSnapshot`
//! and `CoreEvent`s and answers "what should the sidebar/dashboard show". That keeps the
//! expensive logic testable without a window, and keeps rendering a pure function of state.

use crate::state::session::{AppSort, GroupBy, Session};
use neohub_core::environment::ImportCandidate;
use neohub_core::events::{CoreEvent, CoreEventKind};
use neohub_core::snapshot::CoreSnapshot;
use neohub_domain::{App, AppId, ProcessState, RunHistoryRow, Settings, Task, TaskId};
use std::collections::BTreeMap;

/// Cap for the in-memory log tail per task shown in the detail pane.
pub const LOG_TAIL_BYTES: usize = 256 * 1024;

/// One row in the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarApp {
    pub app_id: AppId,
    pub name: String,
    pub color: String,
    pub state: ProcessState,
}

/// One collapsible section (a folder, a tag, or the catch-all group).
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarSection {
    /// Display title: folder name, tag, "(Ungrouped)" or "(Untagged)".
    pub title: String,
    /// True for the catch-all section, which always sorts last.
    pub is_catch_all: bool,
    pub apps: Vec<SidebarApp>,
}

/// One dashboard card.
#[derive(Clone, Debug, PartialEq)]
pub struct AppCard {
    pub app_id: AppId,
    pub name: String,
    pub color: String,
    pub state: ProcessState,
    pub running_tasks: usize,
    pub total_tasks: usize,
    pub cpu_percent: f32,
    pub mem_mb: f64,
    pub ports: Vec<u16>,
    pub folder: Option<String>,
    pub tags: Vec<String>,
}

/// Header totals for the dashboard.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DashboardTotals {
    pub apps: usize,
    pub running: usize,
    pub cpu_percent: f32,
    pub mem_mb: f64,
    pub ports: usize,
}

/// State of the "import a folder of repos" panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportState {
    pub open: bool,
    /// Folder being scanned (shown as the panel's subtitle).
    pub dir: String,
    /// A scan or an import is in flight.
    pub busy: bool,
    /// Last error, shown in the panel and dismissable.
    pub error: Option<String>,
    pub candidates: Vec<ImportCandidate>,
}

impl ImportState {
    /// Candidates that can still be added.
    pub fn addable(&self) -> impl Iterator<Item = &ImportCandidate> {
        self.candidates
            .iter()
            .filter(|candidate| !candidate.already_registered)
    }
}

/// Everything the views read.
pub struct UiStore {
    snapshot: Option<CoreSnapshot>,
    session: Session,
    /// Tail of each task's raw output for the detail pane.
    logs: BTreeMap<TaskId, Vec<u8>>,
    /// Sidebar filter text (`⌘P` search box).
    filter: String,
    /// Active tag filter on the dashboard.
    tag_filter: Option<String>,
    import: ImportState,
    /// Run history per app, as loaded by the detail pane.
    history: BTreeMap<AppId, Vec<RunHistoryRow>>,
    /// Parsed terminal text per task (readable, escapes already consumed).
    terminal_text: BTreeMap<TaskId, String>,
}

impl UiStore {
    pub fn new(session: Session) -> Self {
        UiStore {
            snapshot: None,
            session,
            logs: BTreeMap::new(),
            filter: String::new(),
            tag_filter: None,
            import: ImportState::default(),
            history: BTreeMap::new(),
            terminal_text: BTreeMap::new(),
        }
    }

    pub fn snapshot(&self) -> Option<&CoreSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Adopt settings that were just stored (or arrived from another writer).
    pub fn set_settings(&mut self, settings: Settings) {
        if let Some(snapshot) = self.snapshot.as_mut() {
            snapshot.settings = settings;
        }
    }

    pub fn settings(&self) -> Settings {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.settings.clone())
            .unwrap_or_default()
    }

    /// Replace one task in place (after a successful save) so the tabs show the new name.
    pub fn upsert_task(&mut self, task: Task) {
        let Some(snapshot) = self.snapshot.as_mut() else {
            return;
        };
        match snapshot.tasks_by_app.get_mut(&task.app_id) {
            Some(tasks) => match tasks.iter_mut().find(|existing| existing.id == task.id) {
                Some(existing) => *existing = task,
                None => tasks.push(task),
            },
            None => {
                snapshot
                    .tasks_by_app
                    .insert(task.app_id.clone(), vec![task]);
            }
        }
    }

    pub fn set_terminal_text(&mut self, task_id: TaskId, text: String) {
        self.terminal_text.insert(task_id, text);
    }

    pub fn terminal_text(&self, task_id: &TaskId) -> Option<&str> {
        self.terminal_text.get(task_id).map(String::as_str)
    }

    pub fn set_history(&mut self, app_id: AppId, rows: Vec<RunHistoryRow>) {
        self.history.insert(app_id, rows);
    }

    pub fn history_of(&self, app_id: &AppId) -> &[RunHistoryRow] {
        self.history.get(app_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn import(&self) -> &ImportState {
        &self.import
    }

    /// Open the import panel for `dir` and mark it busy (a scan is about to start).
    pub fn open_import(&mut self, dir: impl Into<String>) {
        self.import = ImportState {
            open: true,
            dir: dir.into(),
            busy: true,
            error: None,
            candidates: Vec::new(),
        };
    }

    pub fn close_import(&mut self) {
        self.import = ImportState::default();
    }

    pub fn set_import_candidates(&mut self, candidates: Vec<ImportCandidate>) {
        self.import.busy = false;
        self.import.error = None;
        self.import.candidates = candidates;
    }

    pub fn set_import_error(&mut self, message: impl Into<String>) {
        self.import.busy = false;
        self.import.error = Some(message.into());
    }

    pub fn set_import_busy(&mut self, busy: bool) {
        self.import.busy = busy;
    }

    /// True when the dashboard has nothing to show (first run).
    pub fn is_empty(&self) -> bool {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.apps.is_empty())
            .unwrap_or(true)
    }

    pub fn set_tag_filter(&mut self, tag: Option<String>) {
        self.tag_filter = tag;
    }

    pub fn tag_filter(&self) -> Option<&str> {
        self.tag_filter.as_deref()
    }

    /// Replace state with a fresh snapshot, then repair dangling selections.
    pub fn apply_snapshot(&mut self, snapshot: CoreSnapshot) {
        self.snapshot = Some(snapshot);
        if let Some(snapshot) = self.snapshot.as_ref() {
            let apps = snapshot.apps.clone();
            let tasks = snapshot.tasks_by_app.clone();
            self.session.reconcile(&apps, &tasks);
        }
    }

    /// Apply one event. Returns true when something the views render has changed.
    pub fn apply_event(&mut self, event: CoreEvent) -> bool {
        let Some(snapshot) = self.snapshot.as_mut() else {
            return false;
        };
        if event.revision <= snapshot.revision && event.revision != 0 {
            return false; // already reflected in the snapshot we hold
        }
        match event.kind {
            CoreEventKind::TaskStatus {
                task_id,
                state,
                pid,
                run_id,
                ..
            } => {
                match snapshot
                    .running_tasks
                    .iter_mut()
                    .find(|info| info.task_id == task_id)
                {
                    Some(info) => {
                        info.state = state;
                        if let Some(pid) = pid {
                            info.pid = pid;
                        }
                        info.run_id = run_id;
                        if !state.is_live() {
                            snapshot.running_tasks.retain(|i| i.task_id != task_id);
                        }
                    }
                    None if state.is_live() => {
                        // A task started outside this snapshot (e.g. via MCP).
                        return true;
                    }
                    None => {}
                }
                snapshot.revision = event.revision;
                true
            }
            CoreEventKind::TaskStats {
                task_id,
                cpu_percent,
                mem_mb,
                tree_cpu_percent,
                tree_mem_mb,
                process_count,
                ..
            } => {
                snapshot.task_stats.insert(
                    task_id,
                    neohub_core::monitor::stats::StatsSample {
                        cpu_percent,
                        mem_mb,
                        tree_cpu_percent,
                        tree_mem_mb,
                        process_count,
                    },
                );
                snapshot.revision = event.revision;
                true
            }
            CoreEventKind::TaskPorts {
                task_id,
                confirmed,
                hinted,
                ..
            } => {
                snapshot.task_ports.insert(
                    task_id,
                    neohub_core::monitor::ports::PortSample { confirmed, hinted },
                );
                snapshot.revision = event.revision;
                true
            }
            CoreEventKind::Log { task_id, bytes, .. } => {
                let buffer = self.logs.entry(task_id).or_default();
                buffer.extend_from_slice(&bytes);
                if buffer.len() > LOG_TAIL_BYTES {
                    let excess = buffer.len() - LOG_TAIL_BYTES;
                    buffer.drain(..excess);
                }
                true
            }
            CoreEventKind::SettingsChanged(settings) => {
                snapshot.settings = *settings;
                snapshot.revision = event.revision;
                true
            }
            CoreEventKind::ResyncRequired { .. } => true,
            CoreEventKind::CatalogChanged => true,
        }
    }

    pub fn logs_of(&self, task_id: &TaskId) -> String {
        self.logs
            .get(task_id)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default()
    }

    pub fn clear_logs(&mut self, task_id: &TaskId) {
        self.logs.remove(task_id);
    }

    /// Apps after the sidebar filter (name, tags and path all match, mirroring the old ⌘P).
    pub fn filtered_apps(&self) -> Vec<&App> {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return Vec::new();
        };
        let needle = self.filter.trim().to_lowercase();
        let mut apps: Vec<&App> = snapshot
            .apps
            .iter()
            .filter(|app| {
                if needle.is_empty() {
                    return true;
                }
                app.name.to_lowercase().contains(&needle)
                    || app.path.to_lowercase().contains(&needle)
                    || app
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&needle))
            })
            .collect();
        self.sort_apps(&mut apps);
        apps
    }

    fn sort_apps(&self, apps: &mut Vec<&App>) {
        match self.session.sort {
            AppSort::Name => apps.sort_by(|a, b| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then_with(|| a.id.as_str().cmp(b.id.as_str()))
            }),
            AppSort::Recent => apps.sort_by(|a, b| {
                b.last_started_at
                    .unwrap_or(0)
                    .cmp(&a.last_started_at.unwrap_or(0))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
            AppSort::Running => apps.sort_by(|a, b| {
                let a_running = self.app_state(&a.id) == ProcessState::Running;
                let b_running = self.app_state(&b.id) == ProcessState::Running;
                b_running
                    .cmp(&a_running)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
        }
    }

    pub fn app_state(&self, app_id: &AppId) -> ProcessState {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.app_states.get(app_id).copied())
            .unwrap_or(ProcessState::Idle)
    }

    /// Sidebar sections for the current grouping mode.
    pub fn sidebar_sections(&self) -> Vec<SidebarSection> {
        let apps = self.filtered_apps();
        match self.session.group_by {
            GroupBy::Folder => self.group_by_folder(&apps),
            GroupBy::Tag => self.group_by_tag(&apps),
        }
    }

    fn group_by_folder(&self, apps: &[&App]) -> Vec<SidebarSection> {
        let mut sections: BTreeMap<String, Vec<SidebarApp>> = BTreeMap::new();
        let mut ungrouped: Vec<SidebarApp> = Vec::new();
        for app in apps {
            let row = self.sidebar_app(app);
            match app.folder.as_deref().filter(|f| !f.trim().is_empty()) {
                Some(folder) => sections.entry(folder.to_string()).or_default().push(row),
                None => ungrouped.push(row),
            }
        }
        let mut out: Vec<SidebarSection> = Vec::new();
        // Case-insensitive folder order, like the legacy Sidebar.
        let mut names: Vec<String> = sections.keys().cloned().collect();
        names.sort_by_key(|name| name.to_lowercase());
        for name in names {
            let mut rows = sections.remove(&name).unwrap_or_default();
            rows.sort_by_key(|a| a.name.to_lowercase());
            out.push(SidebarSection {
                title: name,
                is_catch_all: false,
                apps: rows,
            });
        }
        if !ungrouped.is_empty() {
            ungrouped.sort_by_key(|a| a.name.to_lowercase());
            out.push(SidebarSection {
                title: "(Ungrouped)".to_string(),
                is_catch_all: true,
                apps: ungrouped,
            });
        }
        out
    }

    fn group_by_tag(&self, apps: &[&App]) -> Vec<SidebarSection> {
        let mut sections: BTreeMap<String, Vec<SidebarApp>> = BTreeMap::new();
        let mut untagged: Vec<SidebarApp> = Vec::new();
        for app in apps {
            let row = self.sidebar_app(app);
            if app.tags.is_empty() {
                untagged.push(row);
            } else {
                for tag in &app.tags {
                    sections.entry(tag.clone()).or_default().push(row.clone());
                }
            }
        }
        let mut out: Vec<SidebarSection> = Vec::new();
        let mut names: Vec<String> = sections.keys().cloned().collect();
        names.sort_by_key(|name| name.to_lowercase());
        for name in names {
            let mut rows = sections.remove(&name).unwrap_or_default();
            rows.sort_by_key(|a| a.name.to_lowercase());
            out.push(SidebarSection {
                title: name,
                is_catch_all: false,
                apps: rows,
            });
        }
        if !untagged.is_empty() {
            untagged.sort_by_key(|a| a.name.to_lowercase());
            out.push(SidebarSection {
                title: "(Untagged)".to_string(),
                is_catch_all: true,
                apps: untagged,
            });
        }
        out
    }

    fn sidebar_app(&self, app: &App) -> SidebarApp {
        SidebarApp {
            app_id: app.id.clone(),
            name: app.name.clone(),
            color: app.color.clone(),
            state: self.app_state(&app.id),
        }
    }

    /// Dashboard cards (tag-filtered), in the current sort order.
    pub fn dashboard_cards(&self) -> Vec<AppCard> {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return Vec::new();
        };
        let apps: Vec<&App> = self
            .filtered_apps()
            .into_iter()
            .filter(|app| match self.tag_filter.as_deref() {
                Some(tag) => app.tags.iter().any(|t| t == tag),
                None => true,
            })
            .collect();

        apps.iter()
            .map(|app| {
                let tasks = snapshot.tasks_by_app.get(&app.id);
                let total_tasks = tasks.map(|t| t.len()).unwrap_or(0);
                let running_tasks = tasks
                    .map(|tasks| {
                        tasks
                            .iter()
                            .filter(|task| {
                                snapshot
                                    .running_tasks
                                    .iter()
                                    .any(|info| info.task_id == task.id && info.state.is_live())
                            })
                            .count()
                    })
                    .unwrap_or(0);
                let task_ids: Vec<&TaskId> = tasks
                    .map(|tasks| tasks.iter().map(|task| &task.id).collect())
                    .unwrap_or_default();
                let mut cpu = 0.0f32;
                let mut mem = 0.0f64;
                let mut ports: Vec<u16> = Vec::new();
                for task_id in task_ids {
                    if let Some(stats) = snapshot.task_stats.get(task_id) {
                        cpu += stats.cpu_percent;
                        mem += stats.mem_mb;
                    }
                    if let Some(sample) = snapshot.task_ports.get(task_id) {
                        ports.extend(sample.confirmed.iter().copied());
                    }
                }
                ports.sort_unstable();
                ports.dedup();
                AppCard {
                    app_id: app.id.clone(),
                    name: app.name.clone(),
                    color: app.color.clone(),
                    state: self.app_state(&app.id),
                    running_tasks,
                    total_tasks,
                    cpu_percent: cpu,
                    mem_mb: mem,
                    ports,
                    folder: app.folder.clone(),
                    tags: app.tags.clone(),
                }
            })
            .collect()
    }

    /// Header totals across the (tag-filtered) dashboard.
    pub fn totals(&self) -> DashboardTotals {
        let cards = self.dashboard_cards();
        let mut totals = DashboardTotals {
            apps: cards.len(),
            ..Default::default()
        };
        let mut ports: Vec<u16> = Vec::new();
        for card in &cards {
            if card.state.is_live() {
                totals.running += 1;
            }
            totals.cpu_percent += card.cpu_percent;
            totals.mem_mb += card.mem_mb;
            ports.extend(card.ports.iter().copied());
        }
        ports.sort_unstable();
        ports.dedup();
        totals.ports = ports.len();
        totals
    }

    /// Tasks of the selected app, in position order.
    pub fn tasks_of_selected_app(&self) -> Vec<Task> {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return Vec::new();
        };
        let Some(app_id) = self.session.selected_app_id.as_ref() else {
            return Vec::new();
        };
        snapshot
            .tasks_by_app
            .get(app_id)
            .cloned()
            .unwrap_or_default()
    }

    /// The selected task tab, falling back to the first enabled task.
    pub fn selected_task(&self) -> Option<Task> {
        let tasks = self.tasks_of_selected_app();
        let selected = self
            .session
            .selected_app_id
            .as_ref()
            .and_then(|app_id| self.session.selected_task_by_app.get(app_id));
        if let Some(selected) = selected
            && let Some(task) = tasks.iter().find(|task| &task.id == selected)
        {
            return Some(task.clone());
        }
        tasks
            .iter()
            .find(|task| task.enabled)
            .or_else(|| tasks.first())
            .cloned()
    }

    pub fn selected_app(&self) -> Option<&App> {
        let snapshot = self.snapshot.as_ref()?;
        let app_id = self.session.selected_app_id.as_ref()?;
        snapshot.apps.iter().find(|app| &app.id == app_id)
    }

    /// All tags in use, for the dashboard filter dropdown.
    pub fn all_tags(&self) -> Vec<String> {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return Vec::new();
        };
        let mut tags: Vec<String> = snapshot
            .apps
            .iter()
            .flat_map(|app| app.tags.iter().cloned())
            .collect();
        tags.sort_by_key(|tag| tag.to_lowercase());
        tags.dedup();
        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::session::{AppSort, GroupBy, Session, View};
    use neohub_core::monitor::ports::PortSample;
    use neohub_core::monitor::stats::StatsSample;
    use neohub_core::process::runner::RunningTaskInfo;
    use neohub_domain::{CommandKind, NodeVersionPref, ReadinessSignal, RunId, Settings};

    fn app(id: &str, name: &str, folder: Option<&str>, tags: &[&str], started: Option<i64>) -> App {
        App {
            id: AppId::from_existing(id),
            name: name.into(),
            path: format!("/tmp/{name}"),
            color: "#7aa2f7".into(),
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
            tags: tags.iter().map(|t| t.to_string()).collect(),
            folder: folder.map(str::to_string),
            last_started_at: started,
            last_exit_code: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn task(app_id: &str, id: &str, position: i64) -> Task {
        Task {
            id: TaskId::from_existing(id),
            app_id: AppId::from_existing(app_id),
            name: id.into(),
            position,
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
            created_at: position,
            updated_at: position,
        }
    }

    fn snapshot(apps: Vec<App>, tasks: Vec<Task>, running: Vec<RunningTaskInfo>) -> CoreSnapshot {
        let mut tasks_by_app: BTreeMap<AppId, Vec<Task>> = BTreeMap::new();
        for task in tasks {
            tasks_by_app
                .entry(task.app_id.clone())
                .or_default()
                .push(task);
        }
        let mut app_states = BTreeMap::new();
        for app in &apps {
            let running_here = running
                .iter()
                .any(|info| info.app_id == app.id && info.state.is_live());
            app_states.insert(
                app.id.clone(),
                if running_here {
                    ProcessState::Running
                } else {
                    ProcessState::Idle
                },
            );
        }
        CoreSnapshot {
            revision: 1,
            apps,
            tasks_by_app,
            running_tasks: running,
            app_states,
            settings: Settings::default(),
            task_stats: BTreeMap::new(),
            task_ports: BTreeMap::new(),
        }
    }

    fn running(app_id: &str, task_id: &str) -> RunningTaskInfo {
        RunningTaskInfo {
            task_id: TaskId::from_existing(task_id),
            app_id: AppId::from_existing(app_id),
            pid: 4242,
            pgid: 4242,
            run_id: RunId::from_existing("run1"),
            state: ProcessState::Running,
            started_at: 0,
            command: "pnpm run dev".into(),
        }
    }

    fn store_with(apps: Vec<App>, tasks: Vec<Task>, running: Vec<RunningTaskInfo>) -> UiStore {
        let mut store = UiStore::new(Session::default());
        store.apply_snapshot(snapshot(apps, tasks, running));
        store
    }

    #[test]
    fn folders_group_alphabetically_with_ungrouped_last() {
        let store = store_with(
            vec![
                app("a1", "zeta", Some("Work"), &[], None),
                app("a2", "alpha", Some("Apps"), &[], None),
                app("a3", "loose", None, &[], None),
                app("a4", "beta", Some("Work"), &[], None),
            ],
            vec![],
            vec![],
        );
        let sections = store.sidebar_sections();
        let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, vec!["Apps", "Work", "(Ungrouped)"]);
        assert!(sections[2].is_catch_all);
        let work: Vec<&str> = sections[1].apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            work,
            vec!["beta", "zeta"],
            "apps inside a folder are alphabetical"
        );
    }

    #[test]
    fn tag_grouping_lists_an_app_under_each_of_its_tags() {
        let mut store = store_with(
            vec![
                app("a1", "web", None, &["frontend", "demo"], None),
                app("a2", "api", None, &["backend"], None),
                app("a3", "loose", None, &[], None),
            ],
            vec![],
            vec![],
        );
        store.session_mut().group_by = GroupBy::Tag;
        let sections = store.sidebar_sections();
        let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, vec!["backend", "demo", "frontend", "(Untagged)"]);
        let frontend = &sections[2];
        assert_eq!(frontend.apps.len(), 1);
        assert_eq!(frontend.apps[0].name, "web");
    }

    #[test]
    fn the_three_sort_orders_behave_as_labelled() {
        let apps = vec![
            app("a1", "beta", None, &[], Some(10)),
            app("a2", "alpha", None, &[], Some(30)),
            app("a3", "gamma", None, &[], None),
        ];
        let mut store = store_with(apps.clone(), vec![], vec![running("a3", "t3")]);
        let names = |store: &UiStore| -> Vec<String> {
            store
                .filtered_apps()
                .iter()
                .map(|a| a.name.clone())
                .collect()
        };
        assert_eq!(names(&store), vec!["alpha", "beta", "gamma"], "name sort");

        store.session_mut().sort = AppSort::Recent;
        assert_eq!(
            names(&store),
            vec!["alpha", "beta", "gamma"],
            "recent first"
        );

        store.session_mut().sort = AppSort::Running;
        assert_eq!(
            names(&store),
            vec!["gamma", "alpha", "beta"],
            "running first"
        );
    }

    #[test]
    fn the_filter_matches_name_path_and_tags() {
        let mut store = store_with(
            vec![
                app("a1", "demo-web", None, &["frontend"], None),
                app("a2", "worker", Some("Jobs"), &["queue"], None),
            ],
            vec![],
            vec![],
        );
        store.set_filter("front");
        assert_eq!(store.filtered_apps().len(), 1);
        store.set_filter("jobs/");
        assert_eq!(
            store.filtered_apps().len(),
            0,
            "path is /tmp/worker, not jobs/"
        );
        store.set_filter("queue");
        assert_eq!(store.filtered_apps().len(), 1);
        store.set_filter("");
        assert_eq!(store.filtered_apps().len(), 2);
    }

    #[test]
    fn cards_aggregate_stats_ports_and_task_counts() {
        let mut snap = snapshot(
            vec![app("a1", "demo", None, &[], None)],
            vec![task("a1", "t1", 0), task("a1", "t2", 1)],
            vec![running("a1", "t1")],
        );
        snap.task_stats.insert(
            TaskId::from_existing("t1"),
            StatsSample {
                cpu_percent: 12.5,
                mem_mb: 64.0,
                tree_cpu_percent: 20.0,
                tree_mem_mb: 100.0,
                process_count: 3,
            },
        );
        snap.task_ports.insert(
            TaskId::from_existing("t1"),
            PortSample {
                confirmed: vec![4319],
                hinted: vec![4999],
            },
        );
        let mut store = UiStore::new(Session::default());
        store.apply_snapshot(snap);

        let cards = store.dashboard_cards();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].total_tasks, 2);
        assert_eq!(cards[0].running_tasks, 1);
        assert_eq!(
            cards[0].ports,
            vec![4319],
            "only confirmed ports show as chips"
        );
        assert!((cards[0].cpu_percent - 12.5).abs() < 0.001);
        assert!((cards[0].mem_mb - 64.0).abs() < 0.001);
        assert_eq!(cards[0].state, ProcessState::Running);

        let totals = store.totals();
        assert_eq!(totals.apps, 1);
        assert_eq!(totals.running, 1);
        assert_eq!(totals.ports, 1);
    }

    #[test]
    fn events_update_running_state_stats_and_ports() {
        let mut store = store_with(
            vec![app("a1", "demo", None, &[], None)],
            vec![task("a1", "t1", 0)],
            vec![],
        );
        // A status event starts showing the task as running.
        let mut snap = store.snapshot().unwrap().clone();
        snap.running_tasks.push(running("a1", "t1"));
        store.apply_snapshot(snap);

        let changed = store.apply_event(CoreEvent {
            revision: 2,
            kind: CoreEventKind::TaskStats {
                task_id: TaskId::from_existing("t1"),
                app_id: AppId::from_existing("a1"),
                cpu_percent: 3.0,
                mem_mb: 32.0,
                tree_cpu_percent: 4.0,
                tree_mem_mb: 40.0,
                process_count: 2,
            },
        });
        assert!(changed);
        assert_eq!(store.dashboard_cards()[0].cpu_percent, 3.0);

        store.apply_event(CoreEvent {
            revision: 3,
            kind: CoreEventKind::TaskPorts {
                task_id: TaskId::from_existing("t1"),
                app_id: AppId::from_existing("a1"),
                confirmed: vec![5173],
                hinted: vec![],
            },
        });
        assert_eq!(store.dashboard_cards()[0].ports, vec![5173]);

        // A terminal status removes it from the running list.
        store.apply_event(CoreEvent {
            revision: 4,
            kind: CoreEventKind::TaskStatus {
                task_id: TaskId::from_existing("t1"),
                app_id: AppId::from_existing("a1"),
                state: ProcessState::Exited,
                pid: None,
                run_id: RunId::from_existing("run1"),
                exit: None,
            },
        });
        assert!(store.snapshot().unwrap().running_tasks.is_empty());
    }

    #[test]
    fn stale_events_are_ignored() {
        let mut store = store_with(vec![app("a1", "demo", None, &[], None)], vec![], vec![]);
        let changed = store.apply_event(CoreEvent {
            revision: 1,
            kind: CoreEventKind::CatalogChanged,
        });
        assert!(
            !changed,
            "an event at the snapshot revision is already applied"
        );
    }

    #[test]
    fn log_events_are_appended_and_bounded() {
        let mut store = store_with(vec![], vec![], vec![]);
        let task_id = TaskId::from_existing("t1");
        store.apply_event(CoreEvent {
            revision: 2,
            kind: CoreEventKind::Log {
                task_id: task_id.clone(),
                run_id: RunId::from_existing("run1"),
                seq: 1,
                bytes: b"hello ".to_vec(),
            },
        });
        store.apply_event(CoreEvent {
            revision: 3,
            kind: CoreEventKind::Log {
                task_id: task_id.clone(),
                run_id: RunId::from_existing("run1"),
                seq: 2,
                bytes: b"world".to_vec(),
            },
        });
        assert_eq!(store.logs_of(&task_id), "hello world");

        // Push more than the cap and confirm the tail is kept, not the head.
        let big = vec![b'x'; LOG_TAIL_BYTES + 1024];
        store.apply_event(CoreEvent {
            revision: 4,
            kind: CoreEventKind::Log {
                task_id: task_id.clone(),
                run_id: RunId::from_existing("run1"),
                seq: 3,
                bytes: big,
            },
        });
        assert_eq!(store.logs_of(&task_id).len(), LOG_TAIL_BYTES);
    }

    fn candidate(path: &str, registered: bool) -> ImportCandidate {
        ImportCandidate {
            path: path.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            package_manager: None,
            suggested_script: Some("dev".into()),
            scripts: vec!["dev".into()],
            already_registered: registered,
        }
    }

    #[test]
    fn the_import_panel_tracks_scan_progress_and_errors() {
        let mut store = UiStore::new(Session::default());
        assert!(!store.import().open);

        store.open_import("/tmp/synth");
        assert!(store.import().open);
        assert!(store.import().busy, "a scan starts as busy");
        assert_eq!(store.import().dir, "/tmp/synth");

        store.set_import_candidates(vec![
            candidate("/tmp/synth/a", false),
            candidate("/tmp/synth/b", true),
        ]);
        assert!(!store.import().busy);
        assert_eq!(store.import().candidates.len(), 2);
        assert_eq!(
            store.import().addable().count(),
            1,
            "an already-registered project is not offerable"
        );

        store.set_import_busy(true);
        store.set_import_error("import a failed: duplicate path");
        assert!(!store.import().busy);
        assert_eq!(
            store.import().error.as_deref(),
            Some("import a failed: duplicate path")
        );

        store.close_import();
        assert!(!store.import().open);
        assert!(store.import().candidates.is_empty());
        assert!(store.import().error.is_none());
    }

    #[test]
    fn emptiness_drives_the_first_run_screen() {
        let mut store = UiStore::new(Session::default());
        assert!(store.is_empty(), "no snapshot yet is empty");
        store.apply_snapshot(snapshot(vec![], vec![], vec![]));
        assert!(store.is_empty(), "an empty registry is empty");
        store.apply_snapshot(snapshot(
            vec![app("a1", "demo", None, &[], None)],
            vec![],
            vec![],
        ));
        assert!(!store.is_empty());
    }

    #[test]
    fn a_snapshot_that_drops_the_selected_app_returns_to_the_dashboard() {
        let mut store = store_with(
            vec![app("a1", "demo", None, &[], None)],
            vec![task("a1", "t1", 0)],
            vec![],
        );
        store.session_mut().select_app(
            AppId::from_existing("a1"),
            Some(TaskId::from_existing("t1")),
        );
        assert_eq!(store.session().view, View::App);
        assert_eq!(store.tasks_of_selected_app().len(), 1);
        assert_eq!(
            store.selected_task().map(|t| t.name),
            Some("t1".to_string())
        );

        // The app is removed (e.g. via MCP) and a new snapshot arrives.
        store.apply_snapshot(snapshot(vec![], vec![], vec![]));
        assert_eq!(store.session().view, View::Dashboard);
        assert!(store.selected_app().is_none());
        assert!(store.selected_task().is_none());
    }
}
