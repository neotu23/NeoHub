//! Login-shell PATH probe.
//!
//! An app launched from Finder/Spotlight inherits a stripped `PATH`: no Homebrew, no
//! pnpm shims, no version-manager bin dirs. Probing the user's login shell once recovers
//! the PATH they actually see in a terminal, and that becomes the base for every spawned
//! task.
//!
//! Two rules carried over from `src/main/services/PathProbe.ts`:
//!
//! * the value is wrapped in unique sentinels so shell banners / `nvm is not compatible…`
//!   noise printed by interactive rc files cannot corrupt the result;
//! * the probe has a hard timeout (3 s) and the successful result is cached for the
//!   process lifetime — a probe must never run per render or per monitor tick.

use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PATH_BEGIN: &str = "__DH_PATH_BEGIN__";
const PATH_END: &str = "__DH_PATH_END__";

/// Default probe timeout, same as the Electron build.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// Recover the PATH from probe stdout, discarding rc noise.
pub fn extract_path(stdout: &str) -> String {
    if let Some(start) = stdout.find(PATH_BEGIN) {
        let from = start + PATH_BEGIN.len();
        if let Some(end) = stdout[from..].find(PATH_END) {
            let value = stdout[from..from + end].trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    // Fallback: the last non-empty line is the most likely bare `echo $PATH` output.
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// Cached login-shell PATH.
pub struct PathProbe {
    cached: Mutex<Option<String>>,
    shell: String,
    timeout: Duration,
    fallback: Option<String>,
}

impl Default for PathProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl PathProbe {
    pub fn new() -> Self {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        PathProbe {
            cached: Mutex::new(None),
            shell,
            timeout: DEFAULT_TIMEOUT,
            fallback: std::env::var("PATH").ok(),
        }
    }

    /// Explicit configuration for tests (no reliance on the ambient shell).
    pub fn with_shell(
        shell: impl Into<String>,
        timeout: Duration,
        fallback: Option<String>,
    ) -> Self {
        PathProbe {
            cached: Mutex::new(None),
            shell: shell.into(),
            timeout,
            fallback,
        }
    }

    /// Return the probed PATH, or the process PATH when probing is impossible.
    pub fn get(&self) -> String {
        if let Ok(guard) = self.cached.lock()
            && let Some(value) = guard.as_ref()
        {
            return value.clone();
        }
        let probed = self.probe();
        let value = match probed {
            Some(path) => path,
            None => self
                .fallback
                .clone()
                .unwrap_or_else(|| "/usr/bin:/bin".to_string()),
        };
        // Cache successes only: a failed probe may succeed later (e.g. the user fixes
        // their rc file), matching the legacy "retry a failed probe" behaviour.
        if value != self.fallback.clone().unwrap_or_default()
            && let Ok(mut guard) = self.cached.lock()
        {
            *guard = Some(value.clone());
        }
        value
    }

    /// Drop the cache (used by tests and by an explicit "refresh environment" action).
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.cached.lock() {
            *guard = None;
        }
    }

    fn probe(&self) -> Option<String> {
        let script = format!("printf '{PATH_BEGIN}%s{PATH_END}' \"$PATH\"");
        let mut child = Command::new(&self.shell)
            .arg("-l")
            .arg("-i")
            .arg("-c")
            .arg(script)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let deadline = Instant::now() + self.timeout;
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
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
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let path = extract_path(&stdout);
        if path.is_empty() { None } else { Some(path) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn sentinels_win_over_shell_banner_noise() {
        let stdout = "nvm is not compatible with the npm config\n__DH_PATH_BEGIN__/opt/homebrew/bin:/usr/bin__DH_PATH_END__\nfnm: switched\n";
        assert_eq!(extract_path(stdout), "/opt/homebrew/bin:/usr/bin");
    }

    #[test]
    fn falls_back_to_the_last_non_empty_line() {
        let stdout = "banner\n\n/usr/local/bin:/usr/bin\n\n";
        assert_eq!(extract_path(stdout), "/usr/local/bin:/usr/bin");
    }

    #[test]
    fn empty_output_is_reported_as_empty() {
        assert_eq!(extract_path(""), "");
    }

    fn fake_shell(dir: &std::path::Path, body: &str) -> String {
        let path = dir.join("fake-shell.sh");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "#!/bin/sh\n{body}").unwrap();
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_successful_probe_is_cached() {
        let dir = tempfile::tempdir().unwrap();
        let counter = dir.path().join("calls");
        let shell = fake_shell(
            dir.path(),
            &format!(
                "echo x >> {}\nprintf '__DH_PATH_BEGIN__%s__DH_PATH_END__' '/probe/bin:/usr/bin'\n",
                counter.display()
            ),
        );
        let probe = PathProbe::with_shell(shell, Duration::from_secs(5), None);
        assert_eq!(probe.get(), "/probe/bin:/usr/bin");
        assert_eq!(probe.get(), "/probe/bin:/usr/bin");
        let calls = std::fs::read_to_string(&counter).unwrap();
        assert_eq!(calls.lines().count(), 1, "the probe must run only once");
    }

    #[test]
    fn a_failing_probe_falls_back_and_can_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(dir.path(), "exit 1\n");
        let probe = PathProbe::with_shell(
            shell,
            Duration::from_secs(5),
            Some("/fallback/bin:/usr/bin".to_string()),
        );
        assert_eq!(probe.get(), "/fallback/bin:/usr/bin");
        probe.invalidate();
        assert_eq!(probe.get(), "/fallback/bin:/usr/bin");
    }

    #[test]
    fn a_hanging_shell_times_out_instead_of_blocking_startup() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(dir.path(), "sleep 30\n");
        let started = Instant::now();
        let probe = PathProbe::with_shell(shell, Duration::from_millis(300), Some("/bin".into()));
        assert_eq!(probe.get(), "/bin");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timeout must bound the probe"
        );
    }
}
