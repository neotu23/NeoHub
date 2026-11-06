//! Source-change watching for "restart on change".
//!
//! Watches the project directory recursively, keeps only files matching the task's globs,
//! ignores generated directories, and coalesces bursts so a `git checkout` or a build that
//! touches a hundred files does not restart the app a hundred times.

use glob::Pattern;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// Debounce window for restart triggers (matches the legacy 500 ms).
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// Directories whose changes never mean "the source changed".
pub const IGNORED_DIRS: [&str; 6] = ["node_modules", ".git", ".next", "dist", "build", "target"];

/// Live watcher; dropping it stops the watch.
pub struct RestartWatch {
    events: Receiver<Vec<PathBuf>>,
    _watcher: RecommendedWatcher,
}

impl RestartWatch {
    pub fn try_recv(&self) -> Option<Vec<PathBuf>> {
        self.events.try_recv().ok()
    }

    pub fn recv(&self) -> Option<Vec<PathBuf>> {
        self.events.recv().ok()
    }
}

/// Watch `dir` and emit a coalesced batch of changed files that match `globs`.
pub fn watch_sources(
    dir: impl AsRef<Path>,
    globs: Vec<String>,
    debounce: Duration,
) -> notify::Result<RestartWatch> {
    let dir = dir.as_ref().to_path_buf();
    let (tx, rx) = channel();
    let pending: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>> = Default::default();
    let pending_for_cb = pending.clone();
    let patterns = expand_patterns(&globs);
    let has_globs = !patterns.is_empty();
    let root_for_cb = dir.clone();
    // macOS reports /private/var/... for temp dirs while callers may hold /var/...; try both
    // so relative globs keep working regardless of how the root was spelled.
    let root_canonical = std::fs::canonicalize(&dir).ok();

    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            if let Ok(mut pending) = pending_for_cb.lock() {
                for path in event.paths {
                    if is_ignored(&path) {
                        continue;
                    }
                    // Globs are written relative to the project root (`src/**/*.js`), so match
                    // against the relative path first and fall back to the absolute one.
                    // macOS reports `/private/var/...` for temp dirs while callers may hold
                    // `/var/...`, hence the canonical root is tried as well.
                    let relative = root_canonical
                        .as_deref()
                        .and_then(|root| path.strip_prefix(root).ok())
                        .or_else(|| path.strip_prefix(&root_for_cb).ok())
                        .unwrap_or(&path)
                        .to_path_buf();
                    if has_globs
                        && !matches_any(&patterns, &relative)
                        && !matches_any(&patterns, &path)
                    {
                        // FSEvents sometimes reports a parent directory instead of the changed
                        // file. A directory cannot match a file glob, so a non-ignored directory
                        // event is treated as "something in here changed" — a missed restart is
                        // worse than one extra, and generated dirs are already filtered above.
                        if !path.is_dir() {
                            continue;
                        }
                    }
                    pending.push(path);
                }
            }
        },
        Config::default().with_poll_interval(Duration::from_millis(200)),
    )?;
    watcher.watch(&dir, RecursiveMode::Recursive)?;

    let pending_for_thread = pending.clone();
    std::thread::Builder::new()
        .name(format!("restart-watch-{}", dir.display()))
        .spawn(move || {
            let mut batch: Vec<PathBuf> = Vec::new();
            let mut last_event: Option<Instant> = None;
            loop {
                std::thread::sleep(Duration::from_millis(50));
                let drained: Vec<PathBuf> = match pending_for_thread.lock() {
                    Ok(mut pending) => pending.drain(..).collect(),
                    Err(_) => return,
                };
                if !drained.is_empty() {
                    last_event = Some(Instant::now());
                    for path in drained {
                        if !batch.contains(&path) {
                            batch.push(path);
                        }
                    }
                }
                let quiet = last_event
                    .map(|at| at.elapsed() >= debounce)
                    .unwrap_or(false);
                if quiet && !batch.is_empty() {
                    let ready = std::mem::take(&mut batch);
                    last_event = None;
                    if tx.send(ready).is_err() {
                        return;
                    }
                }
            }
        })
        .ok();

    Ok(RestartWatch {
        events: rx,
        _watcher: watcher,
    })
}

/// True for generated / VCS directories.
pub fn is_ignored(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        IGNORED_DIRS.contains(&name.as_ref())
    })
}

/// Compile user globs, adding the picomatch-equivalent form of `**/`.
///
/// The Electron build used picomatch, where `src/**/*.js` matches `src/app.js` (zero or more
/// directories). The `glob` crate requires at least one directory for `**/`, so the collapsed
/// form (`src/*.js`) is added as a second pattern — same accepted set, no widening.
fn expand_patterns(globs: &[String]) -> Vec<Pattern> {
    let mut out = Vec::new();
    for glob in globs {
        if let Ok(pattern) = Pattern::new(glob) {
            out.push(pattern);
        }
        if glob.contains("**/") {
            let collapsed = glob.replace("**/", "");
            if let Ok(pattern) = Pattern::new(&collapsed) {
                out.push(pattern);
            }
        }
    }
    out
}

fn matches_any(patterns: &[Pattern], path: &Path) -> bool {
    patterns.iter().any(|pattern| {
        pattern.matches_path(path)
            || path
                .file_name()
                .map(|name| pattern.matches(&name.to_string_lossy()))
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_event_is_not_dropped_as_a_glob_miss() {
        let patterns = expand_patterns(&["src/**/*.js".to_string()]);
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("src");
        std::fs::create_dir_all(&sub).unwrap();
        assert!(
            !matches_any(&patterns, &sub),
            "a directory never matches a file glob — that is why callers special-case it"
        );
        assert!(sub.is_dir());
    }

    #[test]
    fn the_picomatch_double_star_semantics_are_preserved() {
        let patterns = expand_patterns(&["src/**/*.js".to_string()]);
        assert!(
            matches_any(&patterns, Path::new("src/app.js")),
            "zero directories"
        );
        assert!(
            matches_any(&patterns, Path::new("src/deep/app.js")),
            "one or more dirs"
        );
        assert!(!matches_any(&patterns, Path::new("lib/app.js")));
    }
    #[test]
    fn generated_directories_are_ignored() {
        assert!(is_ignored(Path::new("/p/node_modules/x/index.js")));
        assert!(is_ignored(Path::new("/p/.git/HEAD")));
        assert!(is_ignored(Path::new("/p/.next/app.js")));
        assert!(is_ignored(Path::new("/p/dist/app.js")));
        assert!(!is_ignored(Path::new("/p/src/app.js")));
    }

    #[test]
    fn only_matching_files_are_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        let watch = watch_sources(
            dir.path(),
            vec!["**/*.js".into()],
            Duration::from_millis(200),
        )
        .expect("watch");
        std::thread::sleep(Duration::from_millis(400));

        std::fs::write(dir.path().join("node_modules/pkg/index.js"), "x").unwrap();
        std::fs::write(dir.path().join("README.md"), "x").unwrap();
        std::fs::write(dir.path().join("src/app.js"), "x").unwrap();

        let batch = wait_batch(&watch, Duration::from_secs(5)).expect("a restart batch");
        assert!(
            batch.iter().any(|p| p.ends_with("src/app.js")),
            "the source file must trigger a restart: {batch:?}"
        );
        assert!(
            !batch
                .iter()
                .any(|p| p.to_string_lossy().contains("node_modules")),
            "generated files must not trigger restarts: {batch:?}"
        );
        assert!(
            !batch.iter().any(|p| p.ends_with("README.md")),
            "non-matching files are filtered by the glob: {batch:?}"
        );
    }

    #[test]
    fn a_burst_of_writes_becomes_one_batch() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let watch = watch_sources(
            dir.path(),
            vec!["**/*.js".into()],
            Duration::from_millis(250),
        )
        .expect("watch");
        std::thread::sleep(Duration::from_millis(400));

        for index in 0..40 {
            std::fs::write(dir.path().join("src").join(format!("f{index}.js")), "x").unwrap();
        }

        let mut batches = 0;
        let deadline = Instant::now() + Duration::from_millis(2000);
        while Instant::now() < deadline {
            if watch.try_recv().is_some() {
                batches += 1;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(batches >= 1, "at least one restart trigger");
        assert!(batches <= 3, "a burst must coalesce, saw {batches} batches");
    }

    fn wait_batch(watch: &RestartWatch, timeout: Duration) -> Option<Vec<PathBuf>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(batch) = watch.try_recv() {
                return Some(batch);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        None
    }
}
