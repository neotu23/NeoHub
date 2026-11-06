//! "Launch at login" via a LaunchAgent.
//!
//! macOS has three ways to do this: `SMAppService` (macOS 13+, needs a signed/notarized
//! bundle), a `LoginItems` AppleScript (`osascript`, needs automation permission), or a
//! LaunchAgent plist the app writes itself. The plist is the only one that works for an
//! unsigned development build, so that is what this does — and the plist is written from
//! scratch every time, pointing at the currently running executable, so moving the bundle
//! cannot leave a stale path behind.

use std::path::{Path, PathBuf};

/// Label of the agent; also the file name.
pub const LABEL: &str = "app.neohub.desktop";

/// `~/Library/LaunchAgents/app.neohub.desktop.plist`
pub fn agent_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        Path::new(&home)
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    })
}

/// The LaunchAgent plist for one executable path.
///
/// `KeepAlive` is deliberately false: the agent's only job is to start the app once at login.
/// A crash loop is the app's own business (it has a single-instance lock and a crash log),
/// not something launchd should restart aggressively.
pub fn render_plist(executable: &Path) -> String {
    let path = xml_escape(&executable.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{path}</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
  <key>ProcessType</key><string>Interactive</string>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
</dict>
</plist>
"#
    )
}

/// Escape the five XML entities. A path with `&` or `<` in it would otherwise produce a plist
/// launchd refuses to parse — and the failure shows up as "login item silently missing".
fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Is the agent file present?
pub fn is_installed() -> bool {
    agent_path().map(|path| path.exists()).unwrap_or(false)
}

/// Write the plist and load it. Idempotent: an existing agent is replaced.
pub fn install(executable: &Path) -> Result<PathBuf, String> {
    let path = agent_path().ok_or_else(|| "HOME is not set".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, render_plist(executable)).map_err(|error| error.to_string())?;
    // Best effort: a load failure (e.g. running inside a test sandbox) must not lose the file.
    let _ = launchctl(&[
        "bootstrap",
        &format!("gui/{}", user_id()),
        &path.to_string_lossy(),
    ]);
    Ok(path)
}

/// Remove the plist and unload it. Safe to call when nothing is installed.
pub fn uninstall() -> Result<(), String> {
    let Some(path) = agent_path() else {
        return Err("HOME is not set".to_string());
    };
    let _ = launchctl(&["bootout", &format!("gui/{}/{LABEL}", user_id())]);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn user_id() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

fn launchctl(args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new("/bin/launchctl")
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if status.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&status.stderr).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plist_names_the_agent_and_the_executable() {
        let plist = render_plist(Path::new(
            "/Applications/NeoHub.app/Contents/MacOS/neohub-desktop",
        ));
        assert!(plist.contains(&format!("<string>{LABEL}</string>")));
        assert!(
            plist.contains("/Applications/NeoHub.app/Contents/MacOS/neohub-desktop"),
            "the executable path must be the one we were given"
        );
        assert!(plist.contains("<key>RunAtLoad</key><true/>"));
        assert!(
            plist.contains("<key>KeepAlive</key><false/>"),
            "keepalive would fight the app's own single-instance lock"
        );
        assert!(plist.contains("<string>Aqua</string>"), "GUI session only");
        assert!(plist.trim_end().ends_with("</plist>"));
    }

    #[test]
    fn a_path_with_xml_characters_is_escaped() {
        let plist = render_plist(Path::new("/tmp/a&b<c>/NeoHub"));
        assert!(plist.contains("/tmp/a&amp;b&lt;c&gt;/NeoHub"));
        assert!(
            !plist.contains("<c>"),
            "raw angle brackets would break the plist"
        );
    }

    #[test]
    fn the_agent_lives_in_the_user_launch_agents_directory() {
        let Some(path) = agent_path() else {
            return;
        };
        assert!(path.ends_with(format!("Library/LaunchAgents/{LABEL}.plist")));
    }

    /// Real round trip against launchd. Ignored by default because it writes to
    /// `~/Library/LaunchAgents` and talks to `launchctl`.
    ///
    /// It installs `/usr/bin/true` rather than the app: `RunAtLoad` would otherwise launch a
    /// second NeoHub instance during the test. That still proves the part that matters — launchd
    /// accepts the generated plist, the job becomes visible, and `uninstall` removes it.
    #[test]
    #[ignore = "writes to ~/Library/LaunchAgents and calls launchctl"]
    fn launchctl_round_trip_accepts_the_generated_plist() {
        if is_installed() {
            eprintln!("skipping: a real agent is already installed");
            return;
        }
        let path = install(Path::new("/usr/bin/true")).expect("install");
        assert!(path.exists(), "the plist is written");
        let listed = launchctl(&["print", &format!("gui/{}/{LABEL}", user_id())]);
        assert!(listed.is_ok(), "launchd should accept the job: {listed:?}");

        uninstall().expect("uninstall");
        assert!(!path.exists(), "the plist is gone");
        let after = launchctl(&["print", &format!("gui/{}/{LABEL}", user_id())]);
        assert!(after.is_err(), "the job is no longer bootstrapped");
    }

    #[test]
    fn uninstalling_when_absent_is_not_an_error() {
        // No plist is written here: this only asserts the "already absent" path is silent,
        // which is what the settings toggle relies on when switching it off repeatedly.
        if is_installed() {
            return; // never touch a real installation from a test
        }
        assert!(uninstall().is_ok());
    }
}
