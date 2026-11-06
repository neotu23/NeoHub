//! End-to-end detection against real directories on disk: a plain app, a monorepo, a
//! folder without `package.json`, a symlinked path and a corrupt manifest.

use neohub_core::environment::detect::{
    detect_project, detect_workspaces, find_env_files, pick_suggested,
};
use neohub_domain::PackageManager;

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

#[test]
fn a_synthetic_single_package_project_is_fully_described() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("demo-web");
    write(
        &project.join("package.json"),
        r#"{"name":"demo-web","scripts":{"dev":"node server.js","build":"node build.js"},"engines":{"node":">=20"}}"#,
    );
    write(&project.join("pnpm-lock.yaml"), "lockfileVersion: 9\n");
    write(&project.join(".env"), "DEMO_FLAG=synthetic\n");
    write(&project.join(".env.local"), "DEMO_LOCAL=1\n");

    let result = detect_project(&project);

    assert!(result.has_package_json);
    assert_eq!(result.package_manager, Some(PackageManager::Pnpm));
    assert_eq!(result.scripts.len(), 2);
    assert_eq!(result.suggested_default_script.as_deref(), Some("dev"));
    assert_eq!(result.node_version_from_project.as_deref(), Some(">=20"));
    assert!(result.has_env_file);
    assert_eq!(
        result.env_files,
        vec![".env".to_string(), ".env.local".to_string()]
    );
    assert!(result.workspaces.is_empty());
}

#[test]
fn a_monorepo_offers_one_candidate_per_workspace_package() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("mono");
    write(
        &root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'apps/*'\n",
    );
    write(
        &root.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"dev":"vite"}}"#,
    );
    write(
        &root.join("apps/api/package.json"),
        r#"{"name":"api","scripts":{"start":"node ."}}"#,
    );

    let result = detect_project(&root);
    assert_eq!(result.workspaces.len(), 2);
    assert_eq!(result.workspaces[0].rel_path, "apps/api");
    assert_eq!(
        result.workspaces[0].suggested_script.as_deref(),
        Some("start")
    );
    assert_eq!(result.workspaces[1].rel_path, "apps/web");
    assert_eq!(detect_workspaces(&root), result.workspaces);
}

#[test]
fn a_folder_without_a_manifest_is_reported_but_not_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("plain-folder");
    std::fs::create_dir_all(&folder).unwrap();
    let result = detect_project(&folder);
    assert!(!result.has_package_json);
    assert!(result.scripts.is_empty());
    assert!(result.package_manager.is_none());
    assert!(result.env_files.is_empty());
    assert!(result.suggested_default_script.is_none());
}

#[test]
fn a_symlinked_project_resolves_to_the_same_detection() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real-app");
    write(&real.join("package.json"), r#"{"scripts":{"dev":"x"}}"#);
    let link = dir.path().join("linked-app");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let from_link = detect_project(&link);
    let from_real = detect_project(&real);
    assert_eq!(from_link.scripts, from_real.scripts);
    assert_eq!(from_link.has_package_json, from_real.has_package_json);
}

#[test]
fn a_corrupt_manifest_reports_no_package_json_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("package.json"), "{\"scripts\": {");
    let result = detect_project(dir.path());
    assert!(!result.has_package_json);
    assert!(result.scripts.is_empty());
}

#[test]
fn env_file_listing_and_script_priority_hold_for_odd_names() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("项目 with space");
    write(
        &project.join("package.json"),
        r#"{"scripts":{"test":"vitest","develop":"x"}}"#,
    );
    write(&project.join(".env.production"), "A=1\n");
    let result = detect_project(&project);
    assert_eq!(result.suggested_default_script.as_deref(), Some("develop"));
    assert_eq!(
        find_env_files(&project),
        vec![".env.production".to_string()]
    );
    assert_eq!(
        pick_suggested(vec!["serve".into(), "build".into()]),
        Some("serve".to_string())
    );
}
