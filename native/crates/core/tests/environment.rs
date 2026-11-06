//! Node resolution and environment construction against a fake home directory — no real
//! Node installation, no real project, and no modification of the user's machine.

use neohub_core::environment::env_builder::build_env;
use neohub_core::environment::node_resolver::{NodeResolver, NodeSource};
use neohub_core::environment::{BuildEnvInput, PathProbe};
use neohub_domain::{AppId, EnvVar, EnvVarId, NodeVersionPref};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fake_node(home: &Path, rel: &str, version: &str) {
    let bin = home.join(rel);
    std::fs::create_dir_all(&bin).unwrap();
    let node = bin.join("node");
    std::fs::write(&node, format!("#!/bin/sh\necho v{version}\n")).unwrap();
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn var(key: &str, value: &str, scope_app: Option<&str>) -> EnvVar {
    EnvVar {
        id: EnvVarId::new(),
        app_id: scope_app.map(AppId::from_existing),
        task_id: None,
        key: key.into(),
        value: value.into(),
        enabled: true,
        is_secret: false,
        note: None,
        created_at: 0,
        updated_at: 0,
    }
}

#[test]
fn a_project_pinning_node_resolves_from_a_version_manager_layout() {
    let dir = tempfile::tempdir().unwrap();
    fake_node(dir.path(), ".nvm/versions/node/v20.11.0/bin", "20.11.0");
    fake_node(dir.path(), ".nvm/versions/node/v22.22.3/bin", "22.22.3");
    let project = dir.path().join("demo-worker");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(".nvmrc"), "20\n").unwrap();

    let resolver = NodeResolver::with_env(dir.path(), None, "");
    let chosen = resolver
        .resolve(&NodeVersionPref::Auto, &project)
        .expect("resolve");
    assert_eq!(chosen.version, "20.11.0");
    assert_eq!(chosen.source, NodeSource::Nvm);
    assert_eq!(
        chosen.bin_dir,
        dir.path().join(".nvm/versions/node/v20.11.0/bin")
    );
}

#[test]
fn building_the_task_environment_layers_every_scope_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("demo-web");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(".env"),
        "FROM_FILE=file\nSHARED=file\nPATH=/evil\n",
    )
    .unwrap();

    let node_bin = dir.path().join(".nvm/versions/node/v22.22.3/bin");
    let mut os_env = BTreeMap::new();
    os_env.insert("HOME".to_string(), "/Users/tester".to_string());
    os_env.insert("UNRELATED".to_string(), "must-not-leak".to_string());

    let global = vec![
        var("SHARED", "global", None),
        var("GLOBAL_ONLY", "yes", None),
    ];
    let app = vec![var("SHARED", "app", Some("app_1"))];
    let task = vec![
        var("SHARED", "task", Some("app_1")),
        var("EMPTY", "", Some("app_1")),
    ];

    let env = build_env(&BuildEnvInput {
        os_env: &os_env,
        node_bin_dir: &node_bin,
        login_path: "/opt/homebrew/bin:/usr/bin",
        project_dir: &project,
        global: &global,
        app: &app,
        task: &task,
    });

    assert_eq!(env.get("HOME").unwrap(), "/Users/tester");
    assert!(
        !env.contains_key("UNRELATED"),
        "only whitelisted OS vars pass through"
    );
    assert_eq!(
        env.get("PATH").unwrap(),
        &format!("{}:/opt/homebrew/bin:/usr/bin", node_bin.display()),
        "the resolved Node bin dir is prepended to the login PATH"
    );
    assert!(
        !env.get("PATH").unwrap().contains("/evil"),
        "a project .env cannot hijack PATH"
    );
    assert_eq!(env.get("FROM_FILE").unwrap(), "file");
    assert_eq!(env.get("GLOBAL_ONLY").unwrap(), "yes");
    assert_eq!(
        env.get("SHARED").unwrap(),
        "task",
        "task scope wins over app and global"
    );
    assert_eq!(
        env.get("EMPTY").unwrap(),
        "",
        "an empty value is meaningful"
    );
    assert_eq!(env.get("FORCE_COLOR").unwrap(), "1");
    assert_eq!(env.get("TERM").unwrap(), "xterm-256color");
}

#[test]
fn a_secret_from_a_locked_store_cannot_reach_the_environment() {
    use neohub_storage::secrets::{FakeSecretStore, SecretStore, read_secret_value};

    let locked = FakeSecretStore::locked();
    let err = read_secret_value(&locked, "nhk1:fake:6869").expect_err("locked store");
    assert_eq!(err, neohub_storage::secrets::SecretError::Locked);

    let working = FakeSecretStore::new();
    let cipher = working.encrypt("hunter2").unwrap();
    assert_eq!(read_secret_value(&working, &cipher).unwrap(), "hunter2");
    assert!(
        !cipher.contains("hunter2"),
        "ciphertext must not embed the plaintext"
    );
}

#[test]
fn the_path_probe_is_bounded_and_falls_back_cleanly() {
    // A shell that never answers must not stall the caller.
    let dir = tempfile::tempdir().unwrap();
    let shell = dir.path().join("slow-shell.sh");
    std::fs::write(&shell, "#!/bin/sh\nsleep 30\n").unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();

    let probe = PathProbe::with_shell(
        shell.to_string_lossy().into_owned(),
        Duration::from_millis(200),
        Some("/fallback:/usr/bin".to_string()),
    );
    let started = std::time::Instant::now();
    assert_eq!(probe.get(), "/fallback:/usr/bin");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_probed_path_is_reused_for_later_builds() {
    let dir = tempfile::tempdir().unwrap();
    let shell = dir.path().join("good-shell.sh");
    let counter: PathBuf = dir.path().join("calls.log");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\necho x >> {}\nprintf '__DH_PATH_BEGIN__%s__DH_PATH_END__' '/probe/bin:/usr/bin'\n",
            counter.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();

    let probe = PathProbe::with_shell(
        shell.to_string_lossy().into_owned(),
        Duration::from_secs(5),
        None,
    );
    assert_eq!(probe.get(), "/probe/bin:/usr/bin");
    assert_eq!(probe.get(), "/probe/bin:/usr/bin");
    let calls = std::fs::read_to_string(&counter).unwrap();
    assert_eq!(calls.lines().count(), 1, "probing must not repeat per use");
}
