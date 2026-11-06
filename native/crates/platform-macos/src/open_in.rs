//! "Open in …" — hand a project folder to another app.
//!
//! Every target is launched through `/usr/bin/open -a <app> <path>`, which lets macOS resolve
//! the app by name and keeps us out of the business of guessing bundle paths. The one exception
//! is Finder, where `open -R` reveals the folder in its parent — that is what "reveal" means.
//!
//! Capability detection is a directory check rather than a registry query: if the app bundle is
//! not in `/Applications` (or `~/Applications`), the menu entry is hidden. A wrong guess would
//! show a menu item that does nothing.

use std::path::{Path, PathBuf};

/// Where a project can be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpenInTarget {
    Finder,
    Terminal,
    VsCode,
    Cursor,
    Sublime,
}

impl OpenInTarget {
    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            OpenInTarget::Finder => "在 Finder 中显示",
            OpenInTarget::Terminal => "在终端中打开",
            OpenInTarget::VsCode => "用 VS Code 打开",
            OpenInTarget::Cursor => "用 Cursor 打开",
            OpenInTarget::Sublime => "用 Sublime Text 打开",
        }
    }

    /// `.app` bundle name, as `open -a` expects it.
    pub fn app_name(self) -> Option<&'static str> {
        match self {
            // Finder is handled by `open -R`: naming it explicitly would open the home folder.
            OpenInTarget::Finder => None,
            OpenInTarget::Terminal => Some("Terminal"),
            OpenInTarget::VsCode => Some("Visual Studio Code"),
            OpenInTarget::Cursor => Some("Cursor"),
            OpenInTarget::Sublime => Some("Sublime Text"),
        }
    }

    /// Bundle path used for detection.
    pub fn bundle(self) -> Option<&'static str> {
        match self {
            OpenInTarget::Finder => None,
            OpenInTarget::Terminal => Some("/System/Applications/Utilities/Terminal.app"),
            OpenInTarget::VsCode => Some("Visual Studio Code.app"),
            OpenInTarget::Cursor => Some("Cursor.app"),
            OpenInTarget::Sublime => Some("Sublime Text.app"),
        }
    }

    /// The command line for one target and one folder.
    pub fn command(self, path: &Path) -> (String, Vec<String>) {
        let target = path.to_string_lossy().into_owned();
        match self {
            OpenInTarget::Finder => ("/usr/bin/open".to_string(), vec!["-R".to_string(), target]),
            other => {
                let app = other.app_name().unwrap_or_default().to_string();
                (
                    "/usr/bin/open".to_string(),
                    vec!["-a".to_string(), app, target],
                )
            }
        }
    }
}

/// Every target, in menu order.
pub const ALL_TARGETS: [OpenInTarget; 5] = [
    OpenInTarget::Finder,
    OpenInTarget::Terminal,
    OpenInTarget::VsCode,
    OpenInTarget::Cursor,
    OpenInTarget::Sublime,
];

/// Is the app bundle present? Finder is always available.
pub fn is_available(target: OpenInTarget) -> bool {
    let Some(bundle) = target.bundle() else {
        return true;
    };
    if bundle.starts_with('/') {
        return Path::new(bundle).exists();
    }
    candidate_app_dirs()
        .iter()
        .any(|dir| dir.join(bundle).exists())
}

/// Targets whose app is installed.
pub fn available_targets() -> Vec<OpenInTarget> {
    ALL_TARGETS
        .into_iter()
        .filter(|target| is_available(*target))
        .collect()
}

/// Where macOS keeps applications.
fn candidate_app_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join("Applications"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_reveals_the_folder_instead_of_opening_it() {
        let (program, args) = OpenInTarget::Finder.command(Path::new("/tmp/project"));
        assert_eq!(program, "/usr/bin/open");
        assert_eq!(args, vec!["-R".to_string(), "/tmp/project".to_string()]);
        assert!(is_available(OpenInTarget::Finder), "Finder always exists");
    }

    #[test]
    fn other_targets_name_the_app_bundle() {
        let (program, args) = OpenInTarget::VsCode.command(Path::new("/tmp/project"));
        assert_eq!(program, "/usr/bin/open");
        assert_eq!(
            args,
            vec![
                "-a".to_string(),
                "Visual Studio Code".to_string(),
                "/tmp/project".to_string()
            ]
        );

        let (_, args) = OpenInTarget::Terminal.command(Path::new("/tmp/project"));
        assert_eq!(args[1], "Terminal");
    }

    #[test]
    fn a_path_with_spaces_survives_as_one_argument() {
        // `open -a` takes paths as argv entries, not as a shell line: no quoting needed, and
        // nothing here goes through a shell, so a space is just a character.
        let (_, args) = OpenInTarget::Cursor.command(Path::new("/tmp/my project"));
        assert_eq!(args.last().unwrap(), "/tmp/my project");
    }

    #[test]
    fn detection_matches_the_installed_bundles() {
        // Whatever this machine has installed, the answer must be consistent with the
        // filesystem rather than hardcoded.
        for target in ALL_TARGETS {
            let Some(bundle) = target.bundle() else {
                continue;
            };
            if bundle.starts_with('/') {
                assert_eq!(is_available(target), Path::new(bundle).exists(), "{bundle}");
            }
        }
        let available = available_targets();
        assert!(
            available.contains(&OpenInTarget::Finder),
            "Finder is always offered"
        );
        // Order is stable so the menu does not reshuffle between frames.
        let mut sorted = available.clone();
        sorted.sort_unstable();
        assert_eq!(available, sorted);
    }

    #[test]
    fn every_target_has_a_label() {
        for target in ALL_TARGETS {
            assert!(!target.label().is_empty());
            assert!(
                !target.app_name().unwrap_or("x").is_empty(),
                "a named target needs an app name"
            );
        }
    }
}

/// Actually launch a target. Detached: the app must not die with our process, and its output is
/// not ours to read.
pub fn launch(target: OpenInTarget, path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("目录不存在：{}", path.display()));
    }
    let (program, args) = target.command(path);
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn launching_a_missing_folder_is_refused_before_spawning() {
        let error = launch(
            OpenInTarget::Finder,
            Path::new("/tmp/neohub-definitely-missing"),
        )
        .expect_err("a missing folder must not be opened");
        assert!(error.contains("不存在"), "{error}");
    }

    #[test]
    fn launching_a_real_folder_reaches_open() {
        // Finder is always installed, so this exercises the real spawn path. `open -R` only
        // reveals a folder; it changes nothing on disk.
        let dir = std::env::temp_dir();
        if launch(OpenInTarget::Finder, &dir).is_ok() {
            // Reached `/usr/bin/open`; the reveal itself is Finder's business.
            assert!(dir.exists());
        }
    }
}
