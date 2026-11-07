//! `neohub-desktop --migrate-legacy [--dry-run]` — moving data over from the Electron build.
//!
//! This is a separate code path from the GUI on purpose: it runs before GPUI starts, prints a
//! plain report and exits, so it works over SSH or in a script and cannot be half-applied by a
//! window opening mid-way.
//!
//! Safety rules, all inherited from `docs/MIGRATION.md`:
//! * the legacy directory is only ever **read** (the copy is what gets inspected),
//! * `--dry-run` inspects and stops — nothing is written to the target,
//! * a target that already holds data is merged with `INSERT OR IGNORE`, never overwritten.

use neohub_storage::import::{
    MigrationManifest, MigrationPreview, copy_source, discover_legacy_dir, import_legacy, inspect,
};
use std::path::{Path, PathBuf};

/// What the command was asked to do.
#[derive(Debug)]
pub struct MigrateRequest {
    pub data_dir: PathBuf,
    pub legacy_dir: Option<PathBuf>,
    pub dry_run: bool,
}

/// Parse `--migrate-legacy` and its companions out of the argument list.
///
/// Returns `None` when the flag is absent, so the caller just starts the GUI as usual.
pub fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<MigrateRequest>, String> {
    let args: Vec<String> = args.collect();
    if !args.iter().any(|arg| arg == "--migrate-legacy") {
        return Ok(None);
    }
    let mut data_dir: Option<PathBuf> = None;
    let mut legacy_dir: Option<PathBuf> = None;
    let mut dry_run = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--dry-run" => dry_run = true,
            "--data-dir" => {
                index += 1;
                data_dir = args.get(index).map(PathBuf::from);
            }
            "--legacy-dir" => {
                index += 1;
                legacy_dir = args.get(index).map(PathBuf::from);
            }
            _ => {}
        }
        index += 1;
    }
    let data_dir = data_dir.ok_or_else(|| {
        "--migrate-legacy needs --data-dir <target> so the destination is never guessed".to_string()
    })?;
    Ok(Some(MigrateRequest {
        data_dir,
        legacy_dir,
        dry_run,
    }))
}

/// Run the migration and print a report. Returns the process exit code.
pub fn run(request: MigrateRequest) -> i32 {
    // 1. Find the source.
    let source = match request.legacy_dir.clone() {
        Some(dir) => dir,
        None => {
            let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
                eprintln!("HOME is not set; pass --legacy-dir");
                return 2;
            };
            let support = home.join("Library/Application Support");
            match discover_legacy_dir(&support) {
                Some(dir) => dir,
                None => {
                    eprintln!("no legacy database found under {}", support.display());
                    return 2;
                }
            }
        }
    };
    if !source.join("devharbor.db").is_file() {
        eprintln!("{} does not contain devharbor.db", source.display());
        return 2;
    }
    // 2. Refuse to write into the source, which would defeat the whole point of copying first.
    if same_path(&source, &request.data_dir) {
        eprintln!(
            "refusing to migrate into the source directory itself; pick a different --data-dir"
        );
        return 2;
    }

    println!("source:  {}", source.display());
    println!("target:  {}", request.data_dir.display());
    println!(
        "mode:    {}",
        if request.dry_run {
            "dry run (nothing written)"
        } else {
            "import"
        }
    );

    // 3. Copy, then inspect the copy: the source is never opened for writing, and never by us.
    let staging = request.data_dir.join(".import-staging");
    let copied = match copy_source(&source, &staging) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("could not copy the legacy database: {error}");
            return 1;
        }
    };
    let had_wal = source.join("devharbor.db-wal").is_file();
    let preview = match inspect(&copied, &source, had_wal) {
        Ok(preview) => preview,
        Err(error) => {
            eprintln!("could not inspect the legacy database: {error}");
            let _ = std::fs::remove_dir_all(&staging);
            return 1;
        }
    };
    print_preview(&preview);

    if request.dry_run {
        let _ = std::fs::remove_dir_all(&staging);
        println!("\ndry run complete — nothing was written to the target.");
        return 0;
    }

    // 4. Import.
    match import_legacy(&copied, &preview, &request.data_dir) {
        Ok(outcome) => {
            println!("\n{}", outcome.report.summary());
            let manifest_path = request.data_dir.join("migration.json");
            let mut manifest = MigrationManifest::load(&manifest_path);
            manifest.record(outcome.report.clone());
            if let Err(error) = manifest.save(&manifest_path) {
                eprintln!(
                    "warning: could not write {}: {error}",
                    manifest_path.display()
                );
            }
            println!("database: {}", outcome.database.display());
            0
        }
        Err(error) => {
            eprintln!("import failed: {error}");
            eprintln!("the target database was not replaced (the import publishes atomically)");
            1
        }
    }
}

fn print_preview(preview: &MigrationPreview) {
    println!("\n-- what was found ------------------------------------------------");
    println!("schema versions : {}", preview.schema_versions.join(", "));
    if !preview.unknown_versions.is_empty() {
        println!(
            "unknown versions: {} (copied verbatim, never dropped)",
            preview.unknown_versions.join(", ")
        );
    }
    let counts = preview.counts;
    println!(
        "rows            : {} apps / {} tasks / {} env vars / {} run history / {} settings",
        counts.apps, counts.tasks, counts.env_vars, counts.run_history, counts.settings
    );
    println!(
        "wal present     : {}",
        if preview.had_wal { "yes" } else { "no" }
    );
    println!("source sha256   : {}", preview.source_sha256);
    if !preview.unknown_settings.is_empty() {
        println!(
            "unknown settings: {} (left in the legacy database, not applied)",
            preview.unknown_settings.join(", ")
        );
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        // A target that does not exist yet cannot be the source.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> std::vec::IntoIter<String> {
        list.iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn without_the_flag_the_gui_starts_normally() {
        assert!(
            parse_args(args(&["neohub-desktop", "--data-dir", "/tmp/x"]))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn the_target_directory_is_required() {
        let error = parse_args(args(&["neohub-desktop", "--migrate-legacy"])).unwrap_err();
        assert!(error.contains("--data-dir"), "{error}");
    }

    #[test]
    fn the_flags_are_read() {
        let request = parse_args(args(&[
            "neohub-desktop",
            "--migrate-legacy",
            "--data-dir",
            "/tmp/target",
            "--legacy-dir",
            "/tmp/legacy",
            "--dry-run",
        ]))
        .unwrap()
        .expect("request");
        assert_eq!(request.data_dir, PathBuf::from("/tmp/target"));
        assert_eq!(request.legacy_dir, Some(PathBuf::from("/tmp/legacy")));
        assert!(request.dry_run);
    }

    #[test]
    fn migrating_into_the_source_is_refused() {
        // The whole design depends on the source staying read-only; pointing the target at it
        // would be the one way to break that.
        let dir = tempfile::tempdir().unwrap();
        assert!(same_path(dir.path(), dir.path()));
        assert!(!same_path(dir.path(), &dir.path().join("other")));
    }
}
