//! Node runtime discovery and selection.
//!
//! Ported from `src/main/services/NodeResolver.ts`. Filesystem-based on purpose: nvm is a
//! shell function, so shelling out to it would be both slow and fragile. Five sources are
//! scanned — nvm, fnm, Volta, asdf and the system `node` on the login-shell PATH — and the
//! rules for picking one are unchanged:
//!
//! * `explicit` → that exact version, else a hard error (never silently substitute);
//! * `system`   → the `node` found on the PATH;
//! * `auto`     → `.nvmrc` / `.node-version` (exact, or major-only like `20`), then
//!   `engines.node` as a semver range, then the highest installed version.
//!
//! Nothing here installs or upgrades Node.

use super::path_probe::PathProbe;
use neohub_domain::NodeVersionPref;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Where a Node installation came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeSource {
    Nvm,
    Fnm,
    Volta,
    Asdf,
    System,
}

impl NodeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeSource::Nvm => "nvm",
            NodeSource::Fnm => "fnm",
            NodeSource::Volta => "volta",
            NodeSource::Asdf => "asdf",
            NodeSource::System => "system",
        }
    }
}

/// One discovered runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeInstallation {
    pub source: NodeSource,
    /// Version without the leading `v`, e.g. `22.22.3`.
    pub version: String,
    pub bin_dir: PathBuf,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NodeError {
    #[error("Node v{version} is not installed. Install it first (e.g. `nvm install {version}`).")]
    NotInstalled { version: String },

    #[error(
        "No installed Node version satisfies engines.node \"{requirement}\". Installed: {installed}"
    )]
    NoVersionSatisfies {
        requirement: String,
        installed: String,
    },

    #[error(
        "engines.node \"{requirement}\" uses a range form Rust's semver cannot express \
         (OR groups). Narrow it to a single AND group, e.g. \">=20 <22\"."
    )]
    InvalidRequirement { requirement: String },

    #[error("No Node installations found on this machine.")]
    NotFound,

    #[error("No system Node installation found on the login-shell PATH.")]
    NoSystemNode,
}

pub type NodeResult<T> = std::result::Result<T, NodeError>;

/// Discovers and selects Node installations for a given home directory.
pub struct NodeResolver {
    home: PathBuf,
    nvm_dir: Option<PathBuf>,
    /// PATH used for the `system` source; pass the login-shell probe result, not the
    /// stripped PATH a Finder-launched app inherits.
    path_env: String,
    cache: Mutex<Option<Vec<NodeInstallation>>>,
}

impl NodeResolver {
    /// Production constructor: reads `HOME`/`NVM_DIR`/`PATH` from the process.
    pub fn new() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let nvm_dir = std::env::var_os("NVM_DIR").map(PathBuf::from);
        let path_env = PathProbe::new().get();
        NodeResolver {
            home,
            nvm_dir,
            path_env,
            cache: Mutex::new(None),
        }
    }

    /// Explicit configuration for tests and for the app host (which may already have a
    /// probed PATH).
    pub fn with_env(
        home: impl Into<PathBuf>,
        nvm_dir: Option<PathBuf>,
        path_env: impl Into<String>,
    ) -> Self {
        NodeResolver {
            home: home.into(),
            nvm_dir,
            path_env: path_env.into(),
            cache: Mutex::new(None),
        }
    }

    /// Drop the cached scan (Node upgrades appear without restarting the app).
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.cache.lock() {
            *guard = None;
        }
    }

    /// Every installation found, highest version first.
    pub fn list(&self) -> Vec<NodeInstallation> {
        self.list_cached(false)
    }

    /// Rescan (used when a start fails and the user may have just installed a version).
    pub fn list_fresh(&self) -> Vec<NodeInstallation> {
        self.list_cached(true)
    }

    fn list_cached(&self, refresh: bool) -> Vec<NodeInstallation> {
        if !refresh
            && let Ok(guard) = self.cache.lock()
            && let Some(list) = guard.as_ref()
        {
            return list.clone();
        }
        let mut installs = Vec::new();
        installs.extend(self.scan_nvm());
        installs.extend(self.scan_fnm());
        installs.extend(self.scan_volta());
        installs.extend(self.scan_asdf());
        if let Some(system) = self.scan_system() {
            installs.push(system);
        }
        sort_by_version_desc(&mut installs);
        if let Ok(mut guard) = self.cache.lock() {
            *guard = Some(installs.clone());
        }
        installs
    }

    /// Resolve the runtime for one app/task.
    pub fn resolve(
        &self,
        pref: &NodeVersionPref,
        project_dir: &Path,
    ) -> NodeResult<NodeInstallation> {
        let installs = self.list();

        match pref {
            NodeVersionPref::System => installs
                .iter()
                .find(|i| i.source == NodeSource::System)
                .cloned()
                .ok_or(NodeError::NoSystemNode),
            NodeVersionPref::Explicit { version } => {
                let wanted = clean_version(version);
                installs
                    .iter()
                    .find(|i| i.version == wanted)
                    .cloned()
                    .ok_or(NodeError::NotInstalled { version: wanted })
            }
            NodeVersionPref::Auto => {
                if let Some(project_version) = self.read_project_node_version(project_dir) {
                    if let Some(found) = installs.iter().find(|i| i.version == project_version) {
                        return Ok(found.clone());
                    }
                    // Major-only pins (`.nvmrc` containing `20`) accept the highest 20.x.
                    if project_version.chars().all(|c| c.is_ascii_digit())
                        && let Some(found) = installs
                            .iter()
                            .find(|i| major_of(&i.version) == Some(project_version.as_str()))
                    {
                        return Ok(found.clone());
                    }
                    return Err(NodeError::NotInstalled {
                        version: project_version,
                    });
                }

                if let Some(requirement) = self.read_engines_node(project_dir) {
                    let normalized = normalize_npm_range(&requirement)?;
                    let req = semver::VersionReq::parse(&normalized).map_err(|_| {
                        NodeError::NoVersionSatisfies {
                            requirement: requirement.clone(),
                            installed: join_versions(&installs),
                        }
                    })?;
                    if let Some(found) = installs.iter().find(|i| {
                        semver::Version::parse(&i.version)
                            .map(|v| req.matches(&v))
                            .unwrap_or(false)
                    }) {
                        return Ok(found.clone());
                    }
                    return Err(NodeError::NoVersionSatisfies {
                        requirement,
                        installed: join_versions(&installs),
                    });
                }

                match installs.first() {
                    Some(found) => Ok(found.clone()),
                    None => installs
                        .iter()
                        .find(|i| i.source == NodeSource::System)
                        .cloned()
                        .ok_or(NodeError::NotFound),
                }
            }
        }
    }

    /// `.nvmrc` / `.node-version`, cleaned: no `v` prefix, no trailing comment.
    pub fn read_project_node_version(&self, project_dir: &Path) -> Option<String> {
        for name in [".nvmrc", ".node-version"] {
            let path = project_dir.join(name);
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let first_word = trimmed.split_whitespace().next().unwrap_or("");
            let cleaned = clean_version(first_word);
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
        None
    }

    /// `package.json` → `engines.node`, as a semver requirement string.
    pub fn read_engines_node(&self, project_dir: &Path) -> Option<String> {
        let raw = std::fs::read_to_string(project_dir.join("package.json")).ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        value
            .get("engines")
            .and_then(|e| e.get("node"))
            .and_then(|n| n.as_str())
            .map(str::to_string)
    }

    // ---- scanners ----------------------------------------------------------

    fn scan_nvm(&self) -> Vec<NodeInstallation> {
        let root = self
            .nvm_dir
            .clone()
            .unwrap_or_else(|| self.home.join(".nvm"));
        scan_version_dir(
            &root.join("versions").join("node"),
            NodeSource::Nvm,
            |dir| dir.join("bin"),
        )
    }

    fn scan_fnm(&self) -> Vec<NodeInstallation> {
        let roots = [
            self.home
                .join(".local")
                .join("share")
                .join("fnm")
                .join("node-versions"),
            self.home.join(".fnm").join("node-versions"),
            self.home
                .join("Library")
                .join("Application Support")
                .join("fnm")
                .join("node-versions"),
        ];
        let mut out = Vec::new();
        for root in roots {
            out.extend(scan_version_dir(&root, NodeSource::Fnm, |dir| {
                dir.join("installation").join("bin")
            }));
        }
        out
    }

    fn scan_volta(&self) -> Vec<NodeInstallation> {
        let root = self
            .home
            .join(".volta")
            .join("tools")
            .join("image")
            .join("node");
        scan_version_dir(&root, NodeSource::Volta, |dir| dir.join("bin"))
    }

    fn scan_asdf(&self) -> Vec<NodeInstallation> {
        let root = self.home.join(".asdf").join("installs").join("nodejs");
        scan_version_dir(&root, NodeSource::Asdf, |dir| dir.join("bin"))
    }

    /// `node` on the given PATH, asked for its version directly (no shell).
    fn scan_system(&self) -> Option<NodeInstallation> {
        for dir in self.path_env.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = Path::new(dir).join("node");
            if !is_executable(&candidate) {
                continue;
            }
            let version = node_version_of(&candidate)?;
            return Some(NodeInstallation {
                source: NodeSource::System,
                version,
                bin_dir: Path::new(dir).to_path_buf(),
            });
        }
        None
    }
}

impl Default for NodeResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn scan_version_dir(
    root: &Path,
    source: NodeSource,
    bin_dir_for: impl Fn(&Path) -> PathBuf,
) -> Vec<NodeInstallation> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let version = clean_version(name);
        if semver::Version::parse(&version).is_err() {
            continue;
        }
        let bin_dir = bin_dir_for(&path);
        if !bin_dir.join("node").exists() {
            continue;
        }
        out.push(NodeInstallation {
            source,
            version,
            bin_dir,
        });
    }
    out
}

/// Translate the npm/node-semver range syntax used in `engines.node` into what Rust's
/// `semver` crate accepts.
///
/// node-semver joins comparators with whitespace (`">=20 <22"`) and allows `||` groups;
/// Rust's `VersionReq` wants commas and has no OR. Whitespace-joined comparators are the
/// same AND semantics, so normalising them is lossless. OR groups are **not** silently
/// reinterpreted — the caller gets an explicit `InvalidRequirement`.
pub fn normalize_npm_range(raw: &str) -> NodeResult<String> {
    let trimmed = raw.trim();
    if trimmed.contains("||") {
        return Err(NodeError::InvalidRequirement {
            requirement: trimmed.to_string(),
        });
    }
    // Commas are also valid separators in npm ranges; unify on whitespace splitting.
    let unified = trimmed.replace(',', " ");
    let mut tokens: Vec<String> = Vec::new();
    for token in unified.split_whitespace() {
        // An operator separated from its version (`>= 20`) belongs with the next token.
        let is_bare_operator = matches!(token, ">" | ">=" | "<" | "<=" | "=" | "^" | "~");
        match (is_bare_operator, tokens.last_mut()) {
            (true, _) => tokens.push(token.to_string()),
            (false, Some(previous))
                if matches!(previous.as_str(), ">" | ">=" | "<" | "<=" | "=" | "^" | "~") =>
            {
                *previous = format!("{previous}{token}");
            }
            _ => tokens.push(token.to_string()),
        }
    }
    Ok(tokens.join(","))
}

fn clean_version(raw: &str) -> String {
    raw.trim().trim_start_matches('v').trim().to_string()
}

fn major_of(version: &str) -> Option<&str> {
    version.split('.').next()
}

fn join_versions(installs: &[NodeInstallation]) -> String {
    if installs.is_empty() {
        return "(none)".to_string();
    }
    installs
        .iter()
        .map(|i| i.version.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn sort_by_version_desc(installs: &mut Vec<NodeInstallation>) {
    installs.sort_by(|a, b| {
        let va = semver::Version::parse(&a.version);
        let vb = semver::Version::parse(&b.version);
        match (va, vb) {
            (Ok(va), Ok(vb)) => vb.cmp(&va),
            _ => b.version.cmp(&a.version),
        }
    });
    // Deduplicate identical (source, version) pairs produced by overlapping fnm roots.
    let mut seen = BTreeSet::new();
    installs.retain(|i| seen.insert((i.source, i.version.clone())));
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Run `<node> --version` with a bounded wait so a broken shim cannot hang startup.
fn node_version_of(node: &Path) -> Option<String> {
    let mut child = Command::new(node)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(3_000);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
    let output = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let version = clean_version(&text);
    if semver::Version::parse(&version).is_ok() {
        Some(version)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Build a fake home with one installation per manager.
    fn fake_home() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        for (rel, version) in [
            (".nvm/versions/node/v20.11.0/bin", "20.11.0"),
            (".nvm/versions/node/v22.22.3/bin", "22.22.3"),
            (
                ".local/share/fnm/node-versions/v18.19.0/installation/bin",
                "18.19.0",
            ),
            (".volta/tools/image/node/21.7.3/bin", "21.7.3"),
            (".asdf/installs/nodejs/19.9.0/bin", "19.9.0"),
        ] {
            let bin = home.join(rel);
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("node"), format!("#!/bin/sh\necho v{version}\n")).unwrap();
            std::fs::set_permissions(bin.join("node"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        // Junk that must be ignored.
        std::fs::create_dir_all(home.join(".nvm/versions/node/not-a-version/bin")).unwrap();
        std::fs::create_dir_all(home.join(".nvm/versions/node/v23.0.0/bin")).unwrap(); // no node bin
        dir
    }

    fn resolver(dir: &tempfile::TempDir) -> NodeResolver {
        // PATH deliberately empty so no real system Node leaks into the assertions.
        NodeResolver::with_env(dir.path(), None, "")
    }

    fn project(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn discovers_all_managers_and_sorts_highest_first() {
        let dir = fake_home();
        let installs = resolver(&dir).list();
        let versions: Vec<&str> = installs.iter().map(|i| i.version.as_str()).collect();
        assert_eq!(
            versions,
            vec!["22.22.3", "21.7.3", "20.11.0", "19.9.0", "18.19.0"]
        );
        let sources: Vec<&str> = installs.iter().map(|i| i.source.as_str()).collect();
        assert_eq!(sources, vec!["nvm", "volta", "nvm", "asdf", "fnm"]);
    }

    #[test]
    fn a_major_only_pin_picks_the_highest_patch_of_that_major() {
        let dir = fake_home();
        let proj = project(&dir, "major-pin");
        std::fs::write(proj.join(".nvmrc"), "v20\n").unwrap();
        let chosen = resolver(&dir)
            .resolve(&NodeVersionPref::Auto, &proj)
            .expect("resolve");
        assert_eq!(chosen.version, "20.11.0");
    }

    #[test]
    fn node_version_file_is_used_when_there_is_no_nvmrc() {
        let dir = fake_home();
        let proj = project(&dir, "node-version-file");
        std::fs::write(proj.join(".node-version"), "18.19.0\n").unwrap();
        let chosen = resolver(&dir)
            .resolve(&NodeVersionPref::Auto, &proj)
            .expect("resolve");
        assert_eq!(chosen.version, "18.19.0");
        assert_eq!(chosen.source, NodeSource::Fnm);
    }

    #[test]
    fn a_pinned_version_that_is_not_installed_is_a_hard_error() {
        let dir = fake_home();
        let proj = project(&dir, "missing-pin");
        std::fs::write(proj.join(".nvmrc"), "16.20.0\n").unwrap();
        let err = resolver(&dir)
            .resolve(&NodeVersionPref::Auto, &proj)
            .expect_err("must not silently substitute a different version");
        assert_eq!(
            err,
            NodeError::NotInstalled {
                version: "16.20.0".into()
            }
        );
    }

    #[test]
    fn engines_node_selects_the_highest_satisfying_version() {
        let dir = fake_home();
        let proj = project(&dir, "engines");
        std::fs::write(
            proj.join("package.json"),
            r#"{"name":"synthetic","engines":{"node":">=20 <22"}}"#,
        )
        .unwrap();
        let chosen = resolver(&dir)
            .resolve(&NodeVersionPref::Auto, &proj)
            .expect("resolve");
        assert_eq!(
            chosen.version, "21.7.3",
            "21.x satisfies >=20 <22 and beats 20.x"
        );
    }

    #[test]
    fn an_unsatisfiable_engines_range_lists_what_is_installed() {
        let dir = fake_home();
        let proj = project(&dir, "engines-impossible");
        std::fs::write(
            proj.join("package.json"),
            r#"{"engines":{"node":">=99.0.0"}}"#,
        )
        .unwrap();
        match resolver(&dir)
            .resolve(&NodeVersionPref::Auto, &proj)
            .expect_err("no version can satisfy")
        {
            NodeError::NoVersionSatisfies {
                requirement,
                installed,
            } => {
                assert_eq!(requirement, ">=99.0.0");
                assert!(
                    installed.contains("22.22.3"),
                    "message lists installed versions"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn without_pins_the_highest_installed_version_wins() {
        let dir = fake_home();
        let proj = project(&dir, "no-pins");
        let chosen = resolver(&dir)
            .resolve(&NodeVersionPref::Auto, &proj)
            .expect("resolve");
        assert_eq!(chosen.version, "22.22.3");
    }

    #[test]
    fn explicit_preference_only_matches_that_version() {
        let dir = fake_home();
        let proj = project(&dir, "explicit");
        let chosen = resolver(&dir)
            .resolve(
                &NodeVersionPref::Explicit {
                    version: "v18.19.0".into(),
                },
                &proj,
            )
            .expect("resolve");
        assert_eq!(chosen.version, "18.19.0", "the leading v is tolerated");
    }

    #[test]
    fn a_system_installation_is_discovered_through_the_given_path() {
        let dir = fake_home();
        let sys_bin = dir.path().join("usr-local-bin");
        std::fs::create_dir_all(&sys_bin).unwrap();
        std::fs::write(sys_bin.join("node"), "#!/bin/sh\necho v23.1.0\n").unwrap();
        std::fs::set_permissions(sys_bin.join("node"), std::fs::Permissions::from_mode(0o755))
            .unwrap();

        let resolver =
            NodeResolver::with_env(dir.path(), None, sys_bin.to_string_lossy().into_owned());
        let installs = resolver.list();
        let system = installs
            .iter()
            .find(|i| i.source == NodeSource::System)
            .expect("system node");
        assert_eq!(system.version, "23.1.0");
        assert_eq!(system.bin_dir, sys_bin);
        assert_eq!(installs[0].version, "23.1.0", "still sorted highest-first");

        let chosen = resolver
            .resolve(&NodeVersionPref::System, &project(&dir, "sys"))
            .expect("system preference");
        assert_eq!(chosen.source, NodeSource::System);
    }

    #[test]
    fn projects_under_paths_with_spaces_and_chinese_are_handled() {
        let dir = fake_home();
        let proj = project(&dir, "我的 项目 with space");
        std::fs::write(proj.join(".nvmrc"), "  v22.22.3  # pinned\n").unwrap();
        let chosen = resolver(&dir)
            .resolve(&NodeVersionPref::Auto, &proj)
            .expect("resolve");
        assert_eq!(chosen.version, "22.22.3");
    }

    #[test]
    fn npm_style_ranges_are_normalised_for_rust_semver() {
        assert_eq!(normalize_npm_range(">=20 <22").unwrap(), ">=20,<22");
        assert_eq!(normalize_npm_range(">= 20 < 22").unwrap(), ">=20,<22");
        assert_eq!(normalize_npm_range("^18.0.0").unwrap(), "^18.0.0");
        assert_eq!(normalize_npm_range(">=18, <21").unwrap(), ">=18,<21");
        assert_eq!(
            normalize_npm_range("^18 || ^20"),
            Err(NodeError::InvalidRequirement {
                requirement: "^18 || ^20".into()
            }),
            "OR groups must fail loudly instead of silently changing meaning"
        );
    }

    #[test]
    fn an_or_range_is_rejected_with_an_actionable_error() {
        let dir = fake_home();
        let proj = project(&dir, "engines-or");
        std::fs::write(
            proj.join("package.json"),
            r#"{"engines":{"node":"^18 || ^20"}}"#,
        )
        .unwrap();
        match resolver(&dir)
            .resolve(&NodeVersionPref::Auto, &proj)
            .expect_err("OR ranges are unsupported")
        {
            NodeError::InvalidRequirement { requirement } => {
                assert_eq!(requirement, "^18 || ^20");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn the_scan_is_cached_until_invalidated() {
        let dir = fake_home();
        let resolver = resolver(&dir);
        assert_eq!(resolver.list().len(), 5);
        // Install another version behind the resolver's back.
        let bin = dir.path().join(".nvm/versions/node/v24.0.0/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("node"), "#!/bin/sh\necho v24.0.0\n").unwrap();
        std::fs::set_permissions(bin.join("node"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(resolver.list().len(), 5, "cached scan");
        assert_eq!(
            resolver.list_fresh().len(),
            6,
            "rescan sees the new version"
        );
    }
}
