//! Single-instance lock.
//!
//! A second NeoHub must not open the database, start its own monitor loop or fight over the
//! MCP port: it hands its activation request to the running instance and exits. The lock is an
//! exclusive `flock` on a file in the data directory, which the OS releases even if the process
//! is killed — no stale-pid guessing.
//!
//! The activation channel is a small file whose modification time the running instance polls;
//! that keeps it file-based (no port, no socket cleanup) and testable without a GUI.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// Outcome of trying to become the primary instance.
pub enum LockOutcome {
    /// This process owns the lock.
    Acquired(SessionLock),
    /// Another instance is running; its activation file was touched so it can raise its window.
    AlreadyRunning { activation_file: PathBuf },
}

/// Holds the exclusive lock for the process lifetime.
pub struct SessionLock {
    path: PathBuf,
    file: File,
}

impl SessionLock {
    /// Try to take the lock in `data_dir`.
    pub fn acquire(data_dir: &Path) -> std::io::Result<LockOutcome> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join("neohub.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;

        // `flock(LOCK_EX | LOCK_NB)`: fails with EWOULDBLOCK when another process holds it.
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result == 0 {
            let mut lock_file = &file;
            let _ = writeln!(lock_file, "{}", std::process::id());
            return Ok(LockOutcome::Acquired(SessionLock { path, file }));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            let activation_file = activation_path(data_dir);
            touch(&activation_file)?;
            return Ok(LockOutcome::AlreadyRunning { activation_file });
        }
        Err(error)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Keep the handle alive for the process lifetime; dropping releases the lock.
    pub fn file(&self) -> &File {
        &self.file
    }
}

/// Where the running instance watches for "someone tried to launch me again".
pub fn activation_path(data_dir: &Path) -> PathBuf {
    data_dir.join("neohub.activate")
}

fn touch(path: &Path) -> std::io::Result<()> {
    // Truncate: the file only carries 'someone asked to activate'; its content is irrelevant.
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    writeln!(file, "{}", std::process::id())
}

/// Read the activation file's modification time (0 when it does not exist yet).
pub fn activation_stamp(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_instance_acquires_and_the_second_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let first = SessionLock::acquire(dir.path()).expect("first");
        let lock = match first {
            LockOutcome::Acquired(lock) => lock,
            LockOutcome::AlreadyRunning { .. } => panic!("the first instance must win"),
        };
        assert!(lock.path().exists());

        match SessionLock::acquire(dir.path()).expect("second") {
            LockOutcome::AlreadyRunning { activation_file } => {
                assert!(
                    activation_file.exists(),
                    "the second instance signals the first"
                );
            }
            LockOutcome::Acquired(_) => panic!("a second instance must not take the lock"),
        }
    }

    #[test]
    fn releasing_the_lock_lets_the_next_instance_in() {
        let dir = tempfile::tempdir().unwrap();
        {
            let _lock = match SessionLock::acquire(dir.path()).unwrap() {
                LockOutcome::Acquired(lock) => lock,
                LockOutcome::AlreadyRunning { .. } => panic!("should acquire"),
            };
        }
        match SessionLock::acquire(dir.path()).unwrap() {
            LockOutcome::Acquired(_) => {}
            LockOutcome::AlreadyRunning { .. } => panic!("the lock must be released on drop"),
        }
    }

    #[test]
    fn the_activation_stamp_advances_when_a_second_instance_tries() {
        let dir = tempfile::tempdir().unwrap();
        let _lock = match SessionLock::acquire(dir.path()).unwrap() {
            LockOutcome::Acquired(lock) => lock,
            LockOutcome::AlreadyRunning { .. } => panic!("should acquire"),
        };
        let activation = activation_path(dir.path());
        let before = activation_stamp(&activation);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = SessionLock::acquire(dir.path()).unwrap();
        let after = activation_stamp(&activation);
        assert!(after >= before);
        assert!(
            after > 0,
            "the activation file exists after a second launch"
        );
    }
}
