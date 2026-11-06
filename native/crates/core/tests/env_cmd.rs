//! Environment variables through the command surface: secrets are encrypted at rest, never
//! returned as text, and a `None` value keeps the stored secret instead of overwriting it.

use neohub_core::catalog::{CreateAppInput, CreateTaskSpec};
use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::service::{CoreService, EnvScope, EnvVarInput};
use neohub_domain::{AppId, CommandKind, TaskId};
use neohub_storage::DataDir;

struct Harness {
    _dir: tempfile::TempDir,
    service: std::sync::Arc<CoreService>,
    app: AppId,
    task: TaskId,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let service = CoreService::open(DataDir::new(dir.path())).expect("service");
        let project = dir.path().join("env-project");
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
                    .expect("app");
                let task = catalog.list_tasks(&app.id).expect("tasks").remove(0).id;
                (app.id, task)
            })
            .expect("storage");
        Harness {
            _dir: dir,
            service,
            app,
            task,
        }
    }
}

fn input(key: &str, value: Option<&str>, secret: bool) -> EnvVarInput {
    EnvVarInput {
        key: key.into(),
        value: value.map(str::to_string),
        is_secret: secret,
        enabled: true,
        note: None,
    }
}

#[tokio::test]
async fn secrets_are_encrypted_at_rest_and_never_returned_as_text() {
    let harness = Harness::new();
    let handle = harness.service.handle();

    let reply = handle
        .request(CoreCommand::SetEnvVars {
            scope: EnvScope::App(harness.app.clone()),
            vars: vec![
                input("DEMO_FLAG", Some("synthetic"), false),
                input("DB_PASSWORD", Some("hunter2"), true),
            ],
        })
        .await
        .expect("set");
    let rows = match reply {
        CoreReply::EnvVars(rows) => rows,
        other => panic!("unexpected reply {other:?}"),
    };
    let secret = rows
        .iter()
        .find(|row| row.key == "DB_PASSWORD")
        .expect("secret row");
    assert!(secret.is_secret);
    assert_eq!(
        secret.value, None,
        "the core must not send secret text back to a view"
    );
    let plain = rows
        .iter()
        .find(|row| row.key == "DEMO_FLAG")
        .expect("plain");
    assert_eq!(plain.value.as_deref(), Some("synthetic"));

    // At rest the secret is ciphertext with the native prefix, not the plaintext.
    let stored = harness
        .service
        .storage_call(|storage| {
            storage
                .env_store()
                .list_app(&harness.app)
                .expect("rows")
                .into_iter()
                .find(|row| row.key == "DB_PASSWORD")
                .expect("stored secret")
                .value
        })
        .expect("storage");
    assert!(
        stored.starts_with(neohub_storage::secrets::NATIVE_PREFIX),
        "stored: {stored}"
    );
    assert!(
        !stored.contains("hunter2"),
        "plaintext must not be in the database"
    );

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn a_none_value_keeps_the_stored_secret_instead_of_overwriting_it() {
    let harness = Harness::new();
    let handle = harness.service.handle();
    let scope = EnvScope::App(harness.app.clone());

    handle
        .request(CoreCommand::SetEnvVars {
            scope: scope.clone(),
            vars: vec![
                input("TOKEN", Some("first"), true),
                input("KEEP", Some("x"), false),
            ],
        })
        .await
        .expect("first write");
    let before = harness
        .service
        .storage_call(|storage| {
            storage
                .env_store()
                .list_app(&harness.app)
                .expect("rows")
                .into_iter()
                .find(|row| row.key == "TOKEN")
                .expect("token")
                .value
        })
        .expect("storage");

    // The editor re-saves with the secret masked (value None) and edits the plain row.
    handle
        .request(CoreCommand::SetEnvVars {
            scope: scope.clone(),
            vars: vec![input("TOKEN", None, true), input("KEEP", Some("y"), false)],
        })
        .await
        .expect("second write");

    let after = harness
        .service
        .storage_call(|storage| {
            storage
                .env_store()
                .list_app(&harness.app)
                .expect("rows")
                .into_iter()
                .find(|row| row.key == "TOKEN")
                .expect("token")
                .value
        })
        .expect("storage");
    assert_eq!(
        before, after,
        "the stored secret is untouched by a masked re-save"
    );
    let rows = match handle
        .request(CoreCommand::ListEnvVars { scope })
        .await
        .expect("list")
    {
        CoreReply::EnvVars(rows) => rows,
        other => panic!("unexpected reply {other:?}"),
    };
    assert_eq!(
        rows.iter()
            .find(|row| row.key == "KEEP")
            .unwrap()
            .value
            .as_deref(),
        Some("y")
    );
    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn the_three_scopes_are_independent() {
    let harness = Harness::new();
    let handle = harness.service.handle();
    let global = EnvScope::Global;
    let app = EnvScope::App(harness.app.clone());
    let task = EnvScope::Task(harness.task.clone());

    for (scope, value) in [
        (global.clone(), "global"),
        (app.clone(), "app"),
        (task.clone(), "task"),
    ] {
        handle
            .request(CoreCommand::SetEnvVars {
                scope,
                vars: vec![input("API_URL", Some(value), false)],
            })
            .await
            .expect("write");
    }

    for (scope, expected) in [(global, "global"), (app, "app"), (task, "task")] {
        let rows = match handle
            .request(CoreCommand::ListEnvVars { scope })
            .await
            .expect("list")
        {
            CoreReply::EnvVars(rows) => rows,
            other => panic!("unexpected reply {other:?}"),
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value.as_deref(), Some(expected));
    }

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}
