//! Navigation behaviour a re-opened window relies on: the selection round-trips through
//! `session.json`, a stale selection falls back to the dashboard, and the sidebar stays
//! correct with a large project count.

use neohub_core::monitor::ports::PortSample;
use neohub_core::monitor::stats::StatsSample;
use neohub_core::process::runner::RunningTaskInfo;
use neohub_core::snapshot::CoreSnapshot;
use neohub_domain::{
    App, AppId, CommandKind, NodeVersionPref, ProcessState, ReadinessSignal, RunId, Settings, Task,
    TaskId,
};
use neohub_ui::state::session::{AppSort, GroupBy, Session, SessionStore, View};
use neohub_ui::state::store::UiStore;
use std::collections::BTreeMap;

fn app(id: &str, name: &str, folder: Option<&str>, tags: &[&str]) -> App {
    App {
        id: AppId::from_existing(id),
        name: name.into(),
        path: format!("/tmp/neohub-synth/{name}"),
        color: "#7aa2f7".into(),
        icon: None,
        node_version_pref: NodeVersionPref::Auto,
        package_manager: None,
        default_script: Some("dev".into()),
        custom_command: None,
        working_dir: format!("/tmp/neohub-synth/{name}"),
        auto_restart_on_change: false,
        auto_start: false,
        watch_globs: vec![],
        port_hint: None,
        tags: tags.iter().map(|t| t.to_string()).collect(),
        folder: folder.map(str::to_string),
        last_started_at: None,
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
        let live = running
            .iter()
            .any(|info| info.app_id == app.id && info.state.is_live());
        app_states.insert(
            app.id.clone(),
            if live {
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

fn one_running(app_id: &str, task_id: &str) -> RunningTaskInfo {
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

#[test]
fn a_selection_survives_the_window_being_destroyed() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path());

    // First window: pick an app and one of its tasks.
    let mut ui = UiStore::new(store.load());
    ui.apply_snapshot(snapshot(
        vec![app("a1", "demo-web", Some("Synthetic"), &["web"])],
        vec![task("a1", "t1", 0), task("a1", "t2", 1)],
        vec![one_running("a1", "t1")],
    ));
    ui.session_mut().select_app(
        AppId::from_existing("a1"),
        Some(TaskId::from_existing("t2")),
    );
    store.save(ui.session()).expect("save session");

    // The window is destroyed and a new one is created: same core, fresh view.
    let mut reopened = UiStore::new(store.load());
    reopened.apply_snapshot(snapshot(
        vec![app("a1", "demo-web", Some("Synthetic"), &["web"])],
        vec![task("a1", "t1", 0), task("a1", "t2", 1)],
        vec![one_running("a1", "t1")],
    ));

    assert_eq!(reopened.session().view, View::App);
    assert_eq!(
        reopened
            .session()
            .selected_app_id
            .as_ref()
            .map(|a| a.as_str()),
        Some("a1")
    );
    assert_eq!(
        reopened.selected_task().map(|task| task.name),
        Some("t2".to_string()),
        "the task tab is restored, not reset to the first task"
    );
    assert_eq!(
        reopened.selected_app().map(|a| a.name.as_str()),
        Some("demo-web")
    );
}

#[test]
fn a_selection_for_a_removed_app_falls_back_to_the_dashboard() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path());
    let mut session = Session::default();
    session.select_app(AppId::from_existing("gone"), None);
    store.save(&session).unwrap();

    let mut ui = UiStore::new(store.load());
    ui.apply_snapshot(snapshot(
        vec![app("a1", "still-here", None, &[])],
        vec![task("a1", "t1", 0)],
        vec![],
    ));
    assert_eq!(ui.session().view, View::Dashboard);
    assert!(ui.selected_app().is_none());
    assert_eq!(ui.dashboard_cards().len(), 1);
}

#[test]
fn grouping_sorting_and_filtering_stay_consistent_with_a_hundred_projects() {
    let mut apps = Vec::new();
    let mut tasks = Vec::new();
    for index in 0..100 {
        let folder = match index % 3 {
            0 => Some("Apps"),
            1 => Some("Services"),
            _ => None,
        };
        let tags = if index % 2 == 0 {
            vec!["web"]
        } else {
            vec!["worker"]
        };
        apps.push(app(
            &format!("a{index}"),
            &format!("project-{index:03}"),
            folder,
            &tags,
        ));
        tasks.push(task(&format!("a{index}"), &format!("t{index}"), 0));
    }
    let mut ui = UiStore::new(Session::default());
    ui.apply_snapshot(snapshot(apps, tasks, vec![]));

    let sections = ui.sidebar_sections();
    let total: usize = sections.iter().map(|section| section.apps.len()).sum();
    assert_eq!(
        total, 100,
        "every project appears exactly once when grouping by folder"
    );
    assert_eq!(
        sections.last().map(|section| section.is_catch_all),
        Some(true),
        "the catch-all group is last"
    );

    ui.session_mut().group_by = GroupBy::Tag;
    let tagged: usize = ui
        .sidebar_sections()
        .iter()
        .filter(|section| !section.is_catch_all)
        .map(|section| section.apps.len())
        .sum();
    assert_eq!(tagged, 100, "every project is under one of its tags");

    ui.session_mut().group_by = GroupBy::Folder;
    ui.set_filter("project-04");
    assert_eq!(ui.filtered_apps().len(), 10, "prefix matches 040..049");
    assert_eq!(ui.dashboard_cards().len(), 10);

    ui.set_filter("");
    ui.session_mut().sort = AppSort::Recent;
    let names: Vec<String> = ui
        .filtered_apps()
        .iter()
        .map(|app| app.name.clone())
        .collect();
    assert_eq!(names.len(), 100);
    assert!(
        names.windows(2).all(|pair| pair[0] <= pair[1]),
        "name tie-break keeps it stable"
    );
}

#[test]
fn the_dashboard_shows_confirmed_ports_and_aggregated_stats() {
    let mut snap = snapshot(
        vec![app("a1", "demo", None, &["web"])],
        vec![task("a1", "t1", 0), task("a1", "t2", 1)],
        vec![one_running("a1", "t1")],
    );
    snap.task_stats.insert(
        TaskId::from_existing("t1"),
        StatsSample {
            cpu_percent: 4.0,
            mem_mb: 40.0,
            tree_cpu_percent: 6.0,
            tree_mem_mb: 60.0,
            process_count: 2,
        },
    );
    snap.task_stats.insert(
        TaskId::from_existing("t2"),
        StatsSample {
            cpu_percent: 1.0,
            mem_mb: 10.0,
            tree_cpu_percent: 1.0,
            tree_mem_mb: 10.0,
            process_count: 1,
        },
    );
    snap.task_ports.insert(
        TaskId::from_existing("t1"),
        PortSample {
            confirmed: vec![4319],
            hinted: vec![5000],
        },
    );

    let mut ui = UiStore::new(Session::default());
    ui.apply_snapshot(snap);
    let card = &ui.dashboard_cards()[0];
    assert_eq!(card.ports, vec![4319], "hinted ports are not chips");
    assert!(
        (card.cpu_percent - 5.0).abs() < 0.001,
        "cpu sums across tasks"
    );
    assert!(
        (card.mem_mb - 50.0).abs() < 0.001,
        "memory sums across tasks"
    );
    assert_eq!(card.running_tasks, 1);
    assert_eq!(card.total_tasks, 2);

    let totals = ui.totals();
    assert_eq!(totals.apps, 1);
    assert_eq!(totals.running, 1);
    assert_eq!(totals.ports, 1);
}
