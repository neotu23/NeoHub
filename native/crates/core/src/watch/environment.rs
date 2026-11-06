//! `.env*` file watching.
//!
//! Non-recursive per project directory, filtered to `.env` / `.env.*`, debounced so a single
//! editor save (which can emit create+write+rename) produces one event. Runs on its own
//! thread — closing a window never stops it.

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// Debounce window: events for the same path inside this window collapse into one.
pub const DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvEventKind {
    Add,
    Change,
    Unlink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvFileEvent {
    pub path: PathBuf,
    pub kind: EnvEventKind,
}

/// Live watcher; dropping it stops the watch.
pub struct EnvWatch {
    events: Receiver<EnvFileEvent>,
    _watcher: RecommendedWatcher,
}

impl EnvWatch {
    /// Blocking receive (callers wrap it in a thread or use `try_recv` in a loop).
    pub fn recv(&self) -> Option<EnvFileEvent> {
        self.events.recv().ok()
    }

    pub fn try_recv(&self) -> Option<EnvFileEvent> {
        self.events.try_recv().ok()
    }
}

/// Watch `dir` for `.env*` changes.
pub fn watch_env_files(dir: impl AsRef<Path>) -> notify::Result<EnvWatch> {
    let dir = dir.as_ref().to_path_buf();
    let (tx, rx) = channel();
    let pending: std::sync::Arc<std::sync::Mutex<Vec<EnvFileEvent>>> = Default::default();
    let pending_for_cb = pending.clone();

    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else { return };
            let kind = match event.kind {
                EventKind::Create(_) => EnvEventKind::Add,
                EventKind::Remove(_) => EnvEventKind::Unlink,
                _ => EnvEventKind::Change,
            };
            if let Ok(mut pending) = pending_for_cb.lock() {
                for path in event.paths {
                    if !is_env_file(&path) {
                        continue;
                    }
                    pending.push(EnvFileEvent { path, kind });
                }
            }
        },
        Config::default().with_poll_interval(Duration::from_millis(200)),
    )?;
    watcher.watch(&dir, RecursiveMode::NonRecursive)?;

    // Debounce thread: forwards at most one event per path per window.
    let pending_for_thread = pending.clone();
    std::thread::Builder::new()
        .name(format!("env-watch-{}", dir.display()))
        .spawn(move || {
            let mut last_sent: std::collections::HashMap<PathBuf, Instant> = Default::default();
            loop {
                std::thread::sleep(Duration::from_millis(50));
                let drained: Vec<EnvFileEvent> = match pending_for_thread.lock() {
                    Ok(mut pending) => pending.drain(..).collect(),
                    Err(_) => return,
                };
                for event in drained {
                    let now = Instant::now();
                    let recent = last_sent
                        .get(&event.path)
                        .map(|at| now.duration_since(*at) < DEBOUNCE)
                        .unwrap_or(false);
                    if recent {
                        continue;
                    }
                    last_sent.insert(event.path.clone(), now);
                    if tx.send(event).is_err() {
                        return;
                    }
                }
            }
        })
        .ok();

    Ok(EnvWatch {
        events: rx,
        _watcher: watcher,
    })
}

/// `.env` or `.env.<something>`.
pub fn is_env_file(path: &Path) -> bool {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name == ".env" || name.starts_with(".env."),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn env_file_detection_matches_the_legacy_glob() {
        assert!(is_env_file(Path::new("/p/.env")));
        assert!(is_env_file(Path::new("/p/.env.local")));
        assert!(is_env_file(Path::new("/p/.env.development.local")));
        assert!(!is_env_file(Path::new("/p/.envrc")));
        assert!(!is_env_file(Path::new("/p/package.json")));
        assert!(!is_env_file(Path::new("/p")));
    }

    #[test]
    fn a_written_env_file_is_reported_and_other_files_are_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let watch = watch_env_files(dir.path()).expect("watch");
        std::thread::sleep(Duration::from_millis(300)); // let the watcher arm
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join(".env"), "A=1\n").unwrap();

        let event = wait_for(&watch, Duration::from_secs(5)).expect("an env event");
        assert!(event.path.ends_with(".env"), "got {event:?}");
        assert!(matches!(
            event.kind,
            EnvEventKind::Add | EnvEventKind::Change
        ));
        assert!(
            watch.try_recv().is_none(),
            "package.json must not produce an env event"
        );
    }

    #[test]
    fn an_event_storm_is_debounced() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".env"), "A=0\n").unwrap();
        let watch = watch_env_files(dir.path()).expect("watch");
        std::thread::sleep(Duration::from_millis(300));
        for index in 0..50 {
            std::fs::write(dir.path().join(".env"), format!("A={index}\n")).unwrap();
        }
        // Collect for a moment: a storm must not turn into 50 events.
        let mut count = 0;
        let deadline = Instant::now() + Duration::from_millis(1200);
        while Instant::now() < deadline {
            if watch.try_recv().is_some() {
                count += 1;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(count >= 1, "at least one event must arrive");
        assert!(count <= 5, "the storm must be coalesced, saw {count}");
    }

    fn wait_for(watch: &EnvWatch, timeout: Duration) -> Option<EnvFileEvent> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(event) = watch.try_recv() {
                return Some(event);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        None
    }
}
