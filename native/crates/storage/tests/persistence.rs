//! Round-trip proof for phase 3's acceptance line: "after a restart the synthetic
//! project, its tasks, its group and the settings are all still there".

use neohub_domain::{
    AppId, CommandKind, LanguageSetting, NodeVersionPref, ReadinessSignal, SettingsPatch, TaskId,
    ThemeSetting,
};
use neohub_storage::catalog::{AppPatch, NewApp, NewTask};
use neohub_storage::{DataDir, Storage};

#[test]
fn a_synthetic_project_survives_a_restart_with_its_tasks_group_and_settings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = DataDir::new(dir.path());
    let app_id = AppId::from_existing("app_persist");
    let task_ids = [
        TaskId::from_existing("task_one"),
        TaskId::from_existing("task_two"),
    ];

    {
        let storage = Storage::open(data_dir.clone()).expect("first open");
        storage
            .catalog()
            .insert_app(&NewApp {
                id: app_id.clone(),
                name: "demo-web".into(),
                path: "/tmp/neohub-synth/demo-web".into(),
                color: "#7aa2f7".into(),
                icon: None,
                node_version_pref: NodeVersionPref::System,
                package_manager: Some(neohub_domain::PackageManager::Npm),
                default_script: Some("dev".into()),
                custom_command: None,
                working_dir: "/tmp/neohub-synth/demo-web".into(),
                auto_restart_on_change: false,
                auto_start: true,
                watch_globs: vec!["src/**/*".into()],
                port_hint: Some(4319),
                tags: vec!["synthetic".into()],
                folder: Some("Synthetic".into()),
                created_at: 100,
                updated_at: 100,
            })
            .expect("insert app");

        for (index, id) in task_ids.iter().enumerate() {
            storage
                .catalog()
                .insert_task(&NewTask {
                    id: id.clone(),
                    app_id: app_id.clone(),
                    name: format!("task{index}"),
                    position: index as i64,
                    command_kind: CommandKind::Script,
                    script: Some("dev".into()),
                    custom_command: None,
                    working_dir_override: None,
                    package_manager_override: None,
                    node_version_pref_override: None,
                    depends_on: if index == 1 {
                        vec![task_ids[0].clone()]
                    } else {
                        vec![]
                    },
                    readiness: if index == 0 {
                        ReadinessSignal::Port {
                            port: 4319,
                            host: None,
                        }
                    } else {
                        ReadinessSignal::None
                    },
                    one_shot: false,
                    enabled: true,
                    env_overrides: Default::default(),
                    created_at: 100,
                    updated_at: 100,
                })
                .expect("insert task");
        }

        let mut settings = storage.settings().load().expect("load settings");
        settings.apply(&SettingsPatch {
            theme: Some(ThemeSetting::Dark),
            language: Some(LanguageSetting::ZhCn),
            tray_enabled: Some(false),
            log_ring_size: Some(4242),
            ..Default::default()
        });
        storage.settings().save(&settings).expect("save settings");
        storage.checkpoint().expect("checkpoint");
    } // Storage dropped: file closed.

    let reopened = Storage::open(data_dir).expect("second open");
    let apps = reopened.catalog().list_apps().expect("list apps");
    assert_eq!(apps.len(), 1, "the project is still registered");
    let app = &apps[0];
    assert_eq!(app.id, app_id);
    assert_eq!(
        app.folder.as_deref(),
        Some("Synthetic"),
        "grouping survived"
    );
    assert_eq!(app.tags, vec!["synthetic".to_string()]);
    assert_eq!(app.port_hint, Some(4319));
    assert!(app.auto_start);
    assert_eq!(app.watch_globs, vec!["src/**/*".to_string()]);

    let tasks = reopened.catalog().list_tasks(&app_id).expect("list tasks");
    assert_eq!(tasks.len(), 2, "both tasks survived");
    assert_eq!(tasks[0].position, 0);
    assert_eq!(tasks[1].depends_on, vec![task_ids[0].clone()]);
    assert_eq!(tasks[0].readiness.kind(), "port");

    let settings = reopened.settings().load().expect("settings");
    assert_eq!(settings.theme, ThemeSetting::Dark);
    assert_eq!(settings.language, LanguageSetting::ZhCn);
    assert!(!settings.tray_enabled);
    assert_eq!(settings.log_ring_size, 4242);
    assert_eq!(
        settings.kill_grace_ms, 5000,
        "fields the patch did not touch keep their stored/default value"
    );
}

#[test]
fn a_corrupt_settings_cell_only_resets_that_field() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = DataDir::new(dir.path());
    {
        let storage = Storage::open(data_dir.clone()).expect("open");
        let mut settings = storage.settings().load().unwrap();
        settings.apply(&SettingsPatch {
            theme: Some(ThemeSetting::Light),
            mcp_port: Some(7000),
            ..Default::default()
        });
        storage.settings().save(&settings).unwrap();
        storage
            .conn()
            .execute(
                "UPDATE settings SET value = 'not-a-theme' WHERE key = 'theme'",
                [],
            )
            .unwrap();
        storage
            .conn()
            .execute(
                "UPDATE settings SET value = 'not-a-number' WHERE key = 'mcp_port'",
                [],
            )
            .unwrap();
    }
    let storage = Storage::open(data_dir).expect("reopen");
    let settings = storage.settings().load().unwrap();
    assert_eq!(
        settings.theme,
        ThemeSetting::System,
        "corrupt theme falls back to its default"
    );
    assert_eq!(
        settings.mcp_port, 6872,
        "corrupt port falls back to its default"
    );
}
