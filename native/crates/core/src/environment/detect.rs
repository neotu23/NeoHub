//! Project detection: package manager, scripts, `.env` files and monorepo workspaces.
//!
//! Ported from `src/main/services/{PMDetector,DetectionService}.ts`. Everything is
//! read-only and cheap (one directory listing + one `package.json` read), so the add
//! flow can call it on every browse without noticeable delay.
//!
//! Malformed input degrades instead of throwing: a broken `package.json` reports
//! `has_package_json: true` with no scripts, so the UI can warn instead of the whole
//! detection failing.

use neohub_domain::PackageManager;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Scripts tried in order when suggesting a default task command.
pub const SCRIPT_PRIORITY: [&str; 4] = ["dev", "start", "serve", "develop"];

/// One workspace package inside a monorepo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkspaceCandidate {
    pub name: String,
    pub rel_path: String,
    pub scripts: Vec<String>,
    pub suggested_script: Option<String>,
}

/// Everything the add-app / import flows need to know about a directory.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DetectionResult {
    pub package_manager: Option<PackageManager>,
    /// `.nvmrc` / `.node-version` value or `engines.node`, cleaned of a `v` prefix.
    pub node_version_from_project: Option<String>,
    pub scripts: BTreeMap<String, String>,
    pub has_env_file: bool,
    pub env_files: Vec<String>,
    pub suggested_default_script: Option<String>,
    pub has_package_json: bool,
    pub workspaces: Vec<WorkspaceCandidate>,
}

/// Detect a project at `project_path`.
pub fn detect_project(project_path: &Path) -> DetectionResult {
    let package_json = read_json(&project_path.join("package.json"));
    let has_package_json = package_json.is_some();

    let scripts = package_json
        .as_ref()
        .and_then(|value| value.get("scripts"))
        .and_then(|value| value.as_object())
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|text| (key.clone(), text.to_string()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let env_files = find_env_files(project_path);
    let suggested_default_script = pick_suggested(scripts.keys().cloned().collect());

    DetectionResult {
        package_manager: detect_package_manager(project_path, package_json.as_ref()),
        node_version_from_project: read_node_version(project_path, package_json.as_ref()),
        has_env_file: !env_files.is_empty(),
        env_files,
        suggested_default_script,
        scripts,
        has_package_json,
        workspaces: detect_workspaces(project_path),
    }
}

/// Pick a package manager from the `packageManager` field (highest confidence) or the
/// lockfile present.
pub fn detect_package_manager(
    project_path: &Path,
    package_json: Option<&serde_json::Value>,
) -> Option<PackageManager> {
    if let Some(declared) = package_json
        .and_then(|value| value.get("packageManager"))
        .and_then(|value| value.as_str())
        .and_then(|text| text.split('@').next())
        .and_then(PackageManager::parse)
    {
        return Some(declared);
    }
    // pnpm first: a pnpm workspace often also has a package-lock.json left over from a
    // one-off `npm install`, and the pnpm lockfile is the authoritative one.
    for (file, manager) in [
        ("pnpm-lock.yaml", PackageManager::Pnpm),
        ("bun.lockb", PackageManager::Bun),
        ("bun.lock", PackageManager::Bun),
        ("yarn.lock", PackageManager::Yarn),
        ("package-lock.json", PackageManager::Npm),
    ] {
        if project_path.join(file).exists() {
            return Some(manager);
        }
    }
    None
}

/// `.nvmrc` / `.node-version` first, then `engines.node`.
pub fn read_node_version(
    project_path: &Path,
    package_json: Option<&serde_json::Value>,
) -> Option<String> {
    for name in [".nvmrc", ".node-version"] {
        if let Ok(raw) = std::fs::read_to_string(project_path.join(name)) {
            let first = raw.split_whitespace().next().unwrap_or("");
            let cleaned = first.trim().trim_start_matches('v').to_string();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    package_json
        .and_then(|value| value.get("engines"))
        .and_then(|value| value.get("node"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// `.env` and every `.env.*` file in the project root, sorted.
pub fn find_env_files(project_path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(project_path) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| name == ".env" || name.starts_with(".env."))
        .collect();
    out.sort();
    out
}

/// Choose the script a new task should default to.
pub fn pick_suggested(script_names: Vec<String>) -> Option<String> {
    for candidate in SCRIPT_PRIORITY {
        if script_names.iter().any(|name| name == candidate) {
            return Some(candidate.to_string());
        }
    }
    script_names.into_iter().next()
}

/// Monorepo workspaces, from `pnpm-workspace.yaml` and/or `package.json` `workspaces`,
/// expanded one level deep and limited to packages that actually define scripts.
pub fn detect_workspaces(root: &Path) -> Vec<WorkspaceCandidate> {
    let (include, exclude) = workspace_globs(root);
    if include.is_empty() {
        return Vec::new();
    }

    let mut dirs: Vec<PathBuf> = Vec::new();
    for glob in &include {
        for dir in expand_glob(root, glob) {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    for glob in &exclude {
        for dir in expand_glob(root, glob) {
            dirs.retain(|candidate| candidate != &dir);
        }
    }

    let mut out = Vec::new();
    for dir in dirs {
        // Globs such as `packages: ['../*']` must never offer a sibling project: a task
        // created from it would run unrelated code under this app's name. Compare
        // canonical paths — a plain prefix check would accept `repo/../sibling`.
        let (Ok(root_canonical), Ok(dir_canonical)) =
            (std::fs::canonicalize(root), std::fs::canonicalize(&dir))
        else {
            continue;
        };
        let Ok(rel) = dir_canonical.strip_prefix(&root_canonical) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let Some(package_json) = read_json(&dir.join("package.json")) else {
            continue;
        };
        let scripts: Vec<String> = package_json
            .get("scripts")
            .and_then(|value| value.as_object())
            .map(|map| {
                map.iter()
                    .filter(|(_, value)| value.is_string())
                    .map(|(key, _)| key.clone())
                    .collect()
            })
            .unwrap_or_default();
        if scripts.is_empty() {
            continue;
        }
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        let name = package_json
            .get("name")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| rel_path.clone());
        let suggested_script = pick_suggested(scripts.clone());
        out.push(WorkspaceCandidate {
            name,
            rel_path,
            scripts,
            suggested_script,
        });
    }
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    out.truncate(50);
    out
}

/// Workspace globs, including pnpm's negated `!pattern` entries.
pub fn workspace_globs(root: &Path) -> (Vec<String>, Vec<String>) {
    let mut include = Vec::new();
    let mut exclude = Vec::new();
    let mut push = |raw: &str| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        match trimmed.strip_prefix('!') {
            Some(rest) => exclude.push(rest.trim_matches(|c| c == '\'' || c == '"').to_string()),
            None => include.push(trimmed.trim_matches(|c| c == '\'' || c == '"').to_string()),
        }
    };

    if let Ok(yaml) = std::fs::read_to_string(root.join("pnpm-workspace.yaml")) {
        parse_pnpm_workspace(&yaml, &mut push);
    }

    if let Some(package_json) = read_json(&root.join("package.json")) {
        let workspaces = package_json.get("workspaces");
        let list = match workspaces {
            Some(serde_json::Value::Array(items)) => items.clone(),
            Some(serde_json::Value::Object(map)) => map
                .get("packages")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        for item in list {
            if let Some(text) = item.as_str() {
                push(text);
            }
        }
    }

    include.sort();
    include.dedup();
    exclude.sort();
    exclude.dedup();
    (include, exclude)
}

/// Minimal `pnpm-workspace.yaml` reader: block lists and the inline flow form.
fn parse_pnpm_workspace(yaml: &str, push: &mut impl FnMut(&str)) {
    let mut in_packages = false;
    for raw in yaml.lines() {
        let line = raw.trim_end();
        let stripped = line.trim();
        if let Some(rest) = stripped.strip_prefix("packages:") {
            let rest = rest.trim();
            if let Some(flow) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                for part in flow.split(',') {
                    push(part.trim());
                }
                in_packages = false;
            } else if rest.is_empty() {
                in_packages = true;
            }
            continue;
        }
        if in_packages {
            if let Some(item) = stripped.strip_prefix('-') {
                push(item.trim().trim_matches(|c| c == '\'' || c == '"').trim());
            } else if !stripped.is_empty() && !stripped.starts_with('#') {
                // Dedented to a new key: the packages list has ended.
                in_packages = false;
            }
        }
    }
}

/// Expand one workspace glob a single level deep (`packages/*`, `apps/*`, or an exact
/// relative path). Not a full glob engine — `**` patterns resolve their static prefix,
/// same as the Electron implementation.
fn expand_glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let clean = pattern.trim_end_matches('/');
    match clean.find('*') {
        None => {
            let candidate = root.join(clean);
            if candidate.is_dir() {
                vec![candidate]
            } else {
                Vec::new()
            }
        }
        Some(star) => {
            let prefix = clean[..star].trim_end_matches('/');
            let parent = root.join(prefix);
            let Ok(entries) = std::fs::read_dir(&parent) else {
                return Vec::new();
            };
            let mut out: Vec<PathBuf> = entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        return None;
                    }
                    let path = parent.join(&name);
                    if path.is_dir() { Some(path) } else { None }
                })
                .collect();
            out.sort();
            out
        }
    }
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn detects_scripts_and_suggests_by_priority() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("package.json"),
            r#"{"name":"demo","scripts":{"build":"tsc","start":"node .","dev":"vite","test":"vitest"}}"#,
        );
        let result = detect_project(dir.path());
        assert!(result.has_package_json);
        assert_eq!(result.scripts.len(), 4);
        assert_eq!(result.suggested_default_script.as_deref(), Some("dev"));
    }

    #[test]
    fn falls_back_to_the_first_script_when_none_of_the_usual_names_exist() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("package.json"),
            r#"{"scripts":{"lint":"eslint .","build":"tsc"}}"#,
        );
        let result = detect_project(dir.path());
        assert_eq!(
            result.suggested_default_script.as_deref(),
            Some("build"),
            "no priority name exists, so the first script (alphabetical) is used"
        );
    }

    #[test]
    fn a_directory_without_package_json_is_flagged_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "A=1\n").unwrap();
        let result = detect_project(dir.path());
        assert!(!result.has_package_json);
        assert!(result.scripts.is_empty());
        assert_eq!(result.env_files, vec![".env".to_string()]);
        assert!(result.has_env_file);
        assert!(result.suggested_default_script.is_none());
        assert!(result.package_manager.is_none());
    }

    #[test]
    fn corrupt_package_json_does_not_break_detection() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("package.json"), "{ not json ]");
        let result = detect_project(dir.path());
        assert!(
            !result.has_package_json,
            "unparsable JSON reports no package.json"
        );
        assert!(result.scripts.is_empty());
    }

    #[test]
    fn package_manager_comes_from_the_lockfile_or_the_package_manager_field() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("package-lock.json"), "{}");
        assert_eq!(
            detect_project(dir.path()).package_manager,
            Some(PackageManager::Npm)
        );

        // pnpm wins over a leftover package-lock.json.
        write(&dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9\n");
        assert_eq!(
            detect_project(dir.path()).package_manager,
            Some(PackageManager::Pnpm)
        );

        // An explicit packageManager field beats every lockfile.
        write(
            &dir.path().join("package.json"),
            r#"{"packageManager":"yarn@4.1.0"}"#,
        );
        assert_eq!(
            detect_project(dir.path()).package_manager,
            Some(PackageManager::Yarn)
        );
    }

    #[test]
    fn node_version_prefers_nvmrc_then_engines() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("package.json"),
            r#"{"engines":{"node":">=20"}}"#,
        );
        assert_eq!(
            detect_project(dir.path())
                .node_version_from_project
                .as_deref(),
            Some(">=20")
        );
        std::fs::write(dir.path().join(".nvmrc"), "v22.22.3\n").unwrap();
        assert_eq!(
            detect_project(dir.path())
                .node_version_from_project
                .as_deref(),
            Some("22.22.3"),
            ".nvmrc wins over engines.node and loses its v prefix"
        );
    }

    #[test]
    fn env_files_are_listed_sorted() {
        let dir = tempfile::tempdir().unwrap();
        for name in [".env.local", ".env", ".env.development", "notes.txt"] {
            std::fs::write(dir.path().join(name), "X=1\n").unwrap();
        }
        assert_eq!(
            detect_project(dir.path()).env_files,
            vec![
                ".env".to_string(),
                ".env.development".to_string(),
                ".env.local".to_string()
            ]
        );
    }

    #[test]
    fn monorepo_workspaces_are_expanded_one_level_deep() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n  - '!apps/legacy'\n",
        )
        .unwrap();
        write(
            &dir.path().join("apps/web/package.json"),
            r#"{"name":"@demo/web","scripts":{"dev":"vite"}}"#,
        );
        write(
            &dir.path().join("apps/api/package.json"),
            r#"{"name":"@demo/api","scripts":{"start":"node ."}}"#,
        );
        write(
            &dir.path().join("apps/legacy/package.json"),
            r#"{"name":"legacy","scripts":{"dev":"old"}}"#,
        );
        // A package with no scripts is not offered as a task candidate.
        write(
            &dir.path().join("apps/empty/package.json"),
            r#"{"name":"empty"}"#,
        );

        let result = detect_project(dir.path());
        let rel_paths: Vec<&str> = result
            .workspaces
            .iter()
            .map(|w| w.rel_path.as_str())
            .collect();
        assert_eq!(rel_paths, vec!["apps/api", "apps/web"]);
        assert_eq!(result.workspaces[1].name, "@demo/web");
        assert_eq!(
            result.workspaces[1].suggested_script.as_deref(),
            Some("dev")
        );
    }

    #[test]
    fn package_json_workspaces_object_form_is_supported() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("package.json"),
            r#"{"workspaces":{"packages":["libs/*"]}}"#,
        );
        write(
            &dir.path().join("libs/core/package.json"),
            r#"{"name":"core","scripts":{"build":"tsc"}}"#,
        );
        let result = detect_project(dir.path());
        assert_eq!(result.workspaces.len(), 1);
        assert_eq!(result.workspaces[0].rel_path, "libs/core");
    }

    #[test]
    fn workspace_globs_pointing_outside_the_root_are_ignored() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        write(&root.join("package.json"), r#"{"workspaces":["../*"]}"#);
        write(
            &parent.path().join("sibling/package.json"),
            r#"{"name":"sibling","scripts":{"dev":"x"}}"#,
        );
        let result = detect_project(&root);
        assert!(
            result.workspaces.is_empty(),
            "a sibling directory is not part of this repo"
        );
    }

    #[test]
    fn a_project_path_with_spaces_and_chinese_characters_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("我的 应用");
        write(
            &project.join("package.json"),
            r#"{"scripts":{"dev":"vite"}}"#,
        );
        let result = detect_project(&project);
        assert!(result.has_package_json);
        assert_eq!(result.suggested_default_script.as_deref(), Some("dev"));
    }
}
