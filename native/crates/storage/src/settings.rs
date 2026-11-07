//! `settings` persistence: one key/value row per setting, exactly like the legacy table.
//!
//! Values are stored the way the Electron build wrote them — booleans as `"0"`/`"1"`,
//! numbers in decimal, `theme`/`language` as their string names — so a database written by
//! either build reads identically. Unknown rows are ignored and unreadable rows fall back
//! to the field default: a corrupt cell must not reset the user's whole settings set.

use crate::Result;
use neohub_domain::{LanguageSetting, Settings, ThemeSetting};
use rusqlite::{Connection, params};

/// Row-level access to `settings`.
pub struct SettingsStore<'c> {
    conn: &'c Connection,
}

impl<'c> SettingsStore<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        SettingsStore { conn }
    }

    /// Read every known setting, falling back per field (never wholesale).
    pub fn load(&self) -> Result<Settings> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (key, value) = row?;
            map.insert(key, value);
        }

        let defaults = Settings::default();
        let num = |key: &str, fallback: u64| -> u64 {
            map.get(key)
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(fallback)
        };
        let boolean = |key: &str, fallback: bool| -> bool {
            match map.get(key).map(|v| v.as_str()) {
                Some("1") | Some("true") => true,
                Some("0") | Some("false") => false,
                _ => fallback,
            }
        };

        Ok(Settings {
            log_ring_size: num("log_ring_size", defaults.log_ring_size as u64) as u32,
            kill_grace_ms: num("kill_grace_ms", defaults.kill_grace_ms),
            auto_update: boolean("auto_update", defaults.auto_update),
            theme: map
                .get("theme")
                .and_then(|v| ThemeSetting::parse(v))
                .unwrap_or(defaults.theme),
            language: map
                .get("language")
                .and_then(|v| LanguageSetting::parse(v))
                .unwrap_or(defaults.language),
            dashboard_refresh_ms: num("dashboard_refresh_ms", defaults.dashboard_refresh_ms),
            notify_on_crash: boolean("notify_on_crash", defaults.notify_on_crash),
            notify_on_ready: boolean("notify_on_ready", defaults.notify_on_ready),
            launch_at_login: boolean("launch_at_login", defaults.launch_at_login),
            tray_enabled: boolean("tray_enabled", defaults.tray_enabled),
            run_history_limit: num("run_history_limit", defaults.run_history_limit as u64) as u32,
            readiness_timeout_ms: num("readiness_timeout_ms", defaults.readiness_timeout_ms),
            mcp_enabled: boolean("mcp_enabled", defaults.mcp_enabled),
            mcp_port: num("mcp_port", defaults.mcp_port as u64) as u16,
            mcp_require_auth: boolean("mcp_require_auth", defaults.mcp_require_auth),
            mcp_https: boolean("mcp_https", defaults.mcp_https),
            skipped_update_version: map
                .get("skipped_update_version")
                .filter(|value| !value.is_empty())
                .cloned()
                .or_else(|| defaults.skipped_update_version.clone()),
        })
    }

    /// Write every setting (idempotent upsert). Runs in one transaction so a crash mid-save
    /// cannot leave a half-updated settings set.
    pub fn save(&self, settings: &Settings) -> Result<()> {
        if !self.conn.is_autocommit() {
            return write_settings(self.conn, settings);
        }
        let tx = self.conn.unchecked_transaction()?;
        write_settings(&tx, settings)?;
        tx.commit()?;
        Ok(())
    }
}

fn write_settings(conn: &Connection, settings: &Settings) -> Result<()> {
    let mut pairs: Vec<(&str, String)> = vec![
        ("log_ring_size", settings.log_ring_size.to_string()),
        ("kill_grace_ms", settings.kill_grace_ms.to_string()),
        ("auto_update", bool_str(settings.auto_update)),
        ("theme", settings.theme.as_str().to_string()),
        ("language", settings.language.as_str().to_string()),
        (
            "dashboard_refresh_ms",
            settings.dashboard_refresh_ms.to_string(),
        ),
        ("notify_on_crash", bool_str(settings.notify_on_crash)),
        ("notify_on_ready", bool_str(settings.notify_on_ready)),
        ("launch_at_login", bool_str(settings.launch_at_login)),
        ("tray_enabled", bool_str(settings.tray_enabled)),
        ("run_history_limit", settings.run_history_limit.to_string()),
        (
            "readiness_timeout_ms",
            settings.readiness_timeout_ms.to_string(),
        ),
        ("mcp_enabled", bool_str(settings.mcp_enabled)),
        ("mcp_port", settings.mcp_port.to_string()),
        ("mcp_require_auth", bool_str(settings.mcp_require_auth)),
        ("mcp_https", bool_str(settings.mcp_https)),
    ];
    // The skip is optional: an empty string means "nothing skipped", which reads back as None.
    pairs.push((
        "skipped_update_version",
        settings.skipped_update_version.clone().unwrap_or_default(),
    ));
    for (key, value) in pairs {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
    }
    Ok(())
}

fn bool_str(value: bool) -> String {
    if value { "1".into() } else { "0".into() }
}
