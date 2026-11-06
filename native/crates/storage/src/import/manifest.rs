//! The migration manifest: what was imported, from where, and how it verified.
//!
//! Written next to the imported database so a later run can tell "this source was already
//! imported" from "this is a different database with the same name". Deliberately contains no
//! project paths, env values or tokens.

use serde::{Deserialize, Serialize};

/// Outcome of one import.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationReport {
    /// SHA-256 of the source database at inspection time.
    pub source_sha256: String,
    /// Schema versions the source carried.
    pub schema_versions: Vec<String>,
    /// Source row counts.
    pub counts: super::legacy_db::TableCounts,
    /// Rows actually written into the new database.
    pub imported: super::legacy_db::TableCounts,
    /// Rows skipped because the target already had them (repeat import).
    pub skipped: super::legacy_db::TableCounts,
    /// Secrets that could not be decrypted (count only — never the values).
    pub secrets_unavailable: i64,
    /// Settings keys this build does not model (kept in the legacy table, not applied).
    pub unknown_settings: Vec<String>,
    /// True when autoStart and MCP were force-disabled for the verification period.
    pub paused_automation: bool,
}

impl MigrationReport {
    /// One-line summary for the UI, no sensitive data.
    pub fn summary(&self) -> String {
        format!(
            "应用 {} · 任务 {} · 环境变量 {} · 历史 {} · 跳过 {} · 未解密 {}",
            self.imported.apps,
            self.imported.tasks,
            self.imported.env_vars,
            self.imported.run_history,
            self.skipped.apps + self.skipped.tasks,
            self.secrets_unavailable
        )
    }

    /// True when this source had already been imported (everything was skipped).
    pub fn is_repeat(&self) -> bool {
        self.imported.apps == 0 && self.imported.tasks == 0 && self.skipped.apps > 0
    }
}

/// Persisted next to the database as `migration.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationManifest {
    pub reports: Vec<MigrationReport>,
}

impl MigrationManifest {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        std::fs::write(path, json)
    }

    /// Has this exact source (by hash) been imported before?
    pub fn contains_source(&self, sha256: &str) -> bool {
        self.reports
            .iter()
            .any(|report| report.source_sha256 == sha256)
    }

    pub fn record(&mut self, report: MigrationReport) {
        self.reports.push(report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::legacy_db::TableCounts;

    #[test]
    fn a_manifest_round_trips_and_recognises_a_known_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration.json");
        let mut manifest = MigrationManifest::load(&path);
        assert!(manifest.reports.is_empty(), "missing file loads empty");

        manifest.record(MigrationReport {
            source_sha256: "abc123".into(),
            schema_versions: vec!["0001_init".into()],
            counts: TableCounts {
                apps: 2,
                ..Default::default()
            },
            imported: TableCounts {
                apps: 2,
                tasks: 3,
                ..Default::default()
            },
            skipped: TableCounts::default(),
            secrets_unavailable: 1,
            unknown_settings: vec!["future_flag".into()],
            paused_automation: true,
        });
        manifest.save(&path).expect("save");

        let reloaded = MigrationManifest::load(&path);
        assert!(reloaded.contains_source("abc123"));
        assert!(!reloaded.contains_source("other"));
        let summary = reloaded.reports[0].summary();
        assert!(summary.contains("应用 2"));
        assert!(!summary.contains("abc123"), "the summary carries no hashes");
    }

    #[test]
    fn a_completely_skipped_import_is_a_repeat() {
        let report = MigrationReport {
            skipped: TableCounts {
                apps: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(report.is_repeat());
        let fresh = MigrationReport {
            imported: TableCounts {
                apps: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!fresh.is_repeat());
    }

    #[test]
    fn a_corrupt_manifest_is_ignored_rather_than_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(MigrationManifest::load(&path).reports.is_empty());
    }
}
