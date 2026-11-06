//! The app's own log file: `~/Library/Logs/NeoHub/neohub.log`.
//!
//! Why a file at all when the UI shows logs: those are the *projects'* logs. When NeoHub itself
//! misbehaves — a panic inside the host, a window that fails to open — the process may die
//! before anything is on screen, and Console.app does not index a log the app writes itself.
//! A plain file next to the other app logs is what a user can attach to a bug report.
//!
//! Rotation is size based and happens on write: one file, and a single `.1` backup. More than
//! that is unnecessary for a desktop app and makes "open the log directory" confusing.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Directory name under `~/Library/Logs`.
const APP_DIR: &str = "NeoHub";
/// File name inside that directory.
const FILE_NAME: &str = "neohub.log";
/// Rotate once this many bytes are on disk (5 MiB, same as a task's log ring).
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// `~/Library/Logs/NeoHub`, or `None` when `HOME` is missing.
pub fn log_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| Path::new(&home).join("Library/Logs").join(APP_DIR))
}

/// Full path of the log file.
pub fn log_path() -> Option<PathBuf> {
    log_dir().map(|dir| dir.join(FILE_NAME))
}

/// One timestamped line, ready to write: `2026-11-07T13:00:00Z [host] message`.
pub fn format_line(level: &str, message: &str) -> String {
    let stamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    format!("{stamp} [{level}] {message}\n")
}

/// Append one line, rotating first when the file is full.
///
/// Rotation is best effort: a failure to rotate must not lose the line being written, so the
/// error is swallowed and the write is attempted anyway.
pub fn append(dir: &Path, level: &str, message: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    if fs::metadata(&path)
        .map(|meta| meta.len() > MAX_BYTES)
        .unwrap_or(false)
    {
        let backup = dir.join(format!("{FILE_NAME}.1"));
        let _ = fs::remove_file(&backup);
        let _ = fs::rename(&path, &backup);
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(format_line(level, message).as_bytes())
}

/// The process-wide log file, opened lazily so a missing `HOME` cannot panic at startup.
static SINK: OnceLock<Option<PathBuf>> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

/// Point the sink at a directory (defaults to [`log_dir`]).
pub fn init(dir: Option<PathBuf>) {
    let _ = SINK.set(dir.or_else(log_dir));
}

/// Write one line to the process log. Never panics and never blocks the caller on IO errors.
pub fn log(level: &str, message: &str) {
    let Some(dir) = SINK.get().and_then(|dir| dir.as_ref()) else {
        return;
    };
    let _guard = LOCK.lock();
    if let Err(error) = append(dir, level, message) {
        // Nowhere left to report this: stderr is the last resort.
        eprintln!("[neohub] could not write the log file: {error}");
    }
}

/// Install a panic hook that records the panic, then lets the default hook print it.
///
/// Without this a panic inside the host leaves only a stderr message that is lost when the app
/// was launched from Finder.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| (*text).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_string());
        log("panic", &format!("{payload} (at {location})"));
        default(info);
    }));
}

/// Open the log directory in Finder. Creates it first so the reveal always has a target.
pub fn reveal(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    std::process::Command::new("/usr/bin/open")
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_lives_under_the_user_logs_directory() {
        let Some(dir) = log_dir() else { return };
        assert!(dir.ends_with(format!("Library/Logs/{APP_DIR}")));
        let path = log_path().expect("path");
        assert!(path.ends_with(FILE_NAME));
    }

    #[test]
    fn a_line_carries_a_utc_timestamp_and_its_level() {
        let line = format_line("host", "window opened");
        assert!(line.ends_with("[host] window opened\n"), "{line}");
        let stamp = line.split(' ').next().expect("timestamp");
        assert_eq!(stamp.len(), 20, "2026-11-07T13:00:00Z");
        assert!(stamp.ends_with('Z'));
        assert_eq!(&stamp[4..5], "-", "date is ISO-8601");
    }

    #[test]
    fn lines_accumulate_in_order() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), "info", "first").expect("write");
        append(dir.path(), "warn", "second").expect("write");
        let text = std::fs::read_to_string(dir.path().join(FILE_NAME)).expect("read");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("[info] first"));
        assert!(lines[1].ends_with("[warn] second"));
    }

    #[test]
    fn the_file_rotates_once_when_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        // Pretend the log is already over the limit.
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        append(dir.path(), "info", "after rotation").expect("write");

        let backup = dir.path().join(format!("{FILE_NAME}.1"));
        assert!(backup.exists(), "the full file is kept as .1");
        let fresh = std::fs::read_to_string(&path).unwrap();
        assert!(fresh.contains("after rotation"), "the new line landed");
        assert!(
            fs::metadata(&path).unwrap().len() < MAX_BYTES,
            "the live file starts over"
        );

        // A second rotation replaces the backup rather than piling up .2, .3 …
        std::fs::write(&path, vec![b'y'; MAX_BYTES as usize + 1]).unwrap();
        append(dir.path(), "info", "again").expect("write");
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.iter().filter(|name| name.contains(".log")).count(), 2);
    }

    #[test]
    fn a_panic_is_written_before_the_default_hook_runs() {
        let dir = tempfile::tempdir().unwrap();
        init(Some(dir.path().to_path_buf()));
        install_panic_hook();

        // A panic we catch ourselves: the hook runs, the test continues.
        let result = std::panic::catch_unwind(|| panic!("boom for the log"));
        assert!(result.is_err());

        let text = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap_or_default();
        assert!(
            text.contains("boom for the log"),
            "the panic must reach the file: {text}"
        );
        assert!(text.contains("[panic]"));
    }

    #[test]
    fn logging_without_an_initialised_sink_is_silent() {
        // No `init` here: the sink is unset, and this must be a no-op rather than a panic.
        log("info", "nobody is listening");
    }
}
