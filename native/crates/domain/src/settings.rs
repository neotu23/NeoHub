//! Application settings.
//!
//! The legacy build stores settings as `settings(key TEXT PRIMARY KEY, value TEXT)`
//! rows keyed by the snake_case names below, with those same defaults in migration
//! `0001_init`. Reading must never reset a user's value: unknown or corrupt rows fall
//! back per field, not wholesale.

use serde::{Deserialize, Serialize};

/// `theme` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeSetting {
    Light,
    Dark,
    System,
}

impl ThemeSetting {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeSetting::Light => "light",
            ThemeSetting::Dark => "dark",
            ThemeSetting::System => "system",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(ThemeSetting::Light),
            "dark" => Some(ThemeSetting::Dark),
            "system" => Some(ThemeSetting::System),
            _ => None,
        }
    }
}

/// `language` setting — `System` follows the OS locale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanguageSetting {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
}

impl LanguageSetting {
    pub fn as_str(self) -> &'static str {
        match self {
            LanguageSetting::System => "system",
            LanguageSetting::En => "en",
            LanguageSetting::ZhCn => "zh-CN",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(LanguageSetting::System),
            "en" => Some(LanguageSetting::En),
            "zh-CN" => Some(LanguageSetting::ZhCn),
            _ => None,
        }
    }
}

/// The complete settings surface. Field-for-field with `SettingsState` in
/// `src/shared/ipc.ts` — nothing silently dropped when ported.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub log_ring_size: u32,
    pub kill_grace_ms: u64,
    pub auto_update: bool,
    pub theme: ThemeSetting,
    pub language: LanguageSetting,
    pub dashboard_refresh_ms: u64,
    pub notify_on_crash: bool,
    pub notify_on_ready: bool,
    pub launch_at_login: bool,
    pub tray_enabled: bool,
    pub run_history_limit: u32,
    pub readiness_timeout_ms: u64,
    pub mcp_enabled: bool,
    pub mcp_port: u16,
    pub mcp_require_auth: bool,
    pub mcp_https: bool,
    /// Version the user asked not to be told about again ("skip this release").
    ///
    /// Stored as the normalised version string, so a re-tagged release with the same number
    /// stays skipped while a genuinely newer one still shows up.
    #[serde(default)]
    pub skipped_update_version: Option<String>,
}

impl Default for Settings {
    /// Defaults copied from `src/main/services/Settings.ts` `DEFAULTS`.
    fn default() -> Self {
        Settings {
            log_ring_size: 10_000,
            kill_grace_ms: 5_000,
            auto_update: true,
            theme: ThemeSetting::System,
            language: LanguageSetting::System,
            dashboard_refresh_ms: 1_000,
            notify_on_crash: true,
            notify_on_ready: false,
            launch_at_login: false,
            tray_enabled: true,
            run_history_limit: 500,
            readiness_timeout_ms: 60_000,
            mcp_enabled: false,
            mcp_port: 6872,
            mcp_require_auth: true,
            mcp_https: false,
            skipped_update_version: None,
        }
    }
}

/// Explicit patch: `None` means "leave unchanged". A free-form map is deliberately
/// not accepted — a typo must be a compile error, not a silent no-op.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsPatch {
    pub log_ring_size: Option<u32>,
    pub kill_grace_ms: Option<u64>,
    pub auto_update: Option<bool>,
    pub theme: Option<ThemeSetting>,
    pub language: Option<LanguageSetting>,
    pub dashboard_refresh_ms: Option<u64>,
    pub notify_on_crash: Option<bool>,
    pub notify_on_ready: Option<bool>,
    pub launch_at_login: Option<bool>,
    pub tray_enabled: Option<bool>,
    pub run_history_limit: Option<u32>,
    pub readiness_timeout_ms: Option<u64>,
    pub mcp_enabled: Option<bool>,
    pub mcp_port: Option<u16>,
    pub mcp_require_auth: Option<bool>,
    pub mcp_https: Option<bool>,
    /// `Some(Some(version))` skips that version, `Some(None)` clears the skip.
    #[serde(default)]
    pub skipped_update_version: Option<Option<String>>,
}

impl Settings {
    /// Apply a patch, clamping values the way the legacy service does on read/write
    /// (a hand-edited row must not make the MCP server bind a privileged port).
    pub fn apply(&mut self, patch: &SettingsPatch) {
        if let Some(v) = patch.log_ring_size {
            self.log_ring_size = v.max(100);
        }
        if let Some(v) = patch.kill_grace_ms {
            self.kill_grace_ms = v.max(100);
        }
        if let Some(v) = patch.auto_update {
            self.auto_update = v;
        }
        if let Some(v) = patch.theme {
            self.theme = v;
        }
        if let Some(v) = patch.language {
            self.language = v;
        }
        if let Some(v) = patch.dashboard_refresh_ms {
            // Legacy lower bound: the stats sampler never runs faster than 200 ms.
            self.dashboard_refresh_ms = v.max(200);
        }
        if let Some(v) = patch.notify_on_crash {
            self.notify_on_crash = v;
        }
        if let Some(v) = patch.notify_on_ready {
            self.notify_on_ready = v;
        }
        if let Some(v) = patch.launch_at_login {
            self.launch_at_login = v;
        }
        if let Some(v) = patch.tray_enabled {
            self.tray_enabled = v;
        }
        if let Some(v) = patch.run_history_limit {
            self.run_history_limit = v.max(1);
        }
        if let Some(v) = patch.readiness_timeout_ms {
            self.readiness_timeout_ms = v.max(1_000);
        }
        if let Some(v) = patch.mcp_enabled {
            self.mcp_enabled = v;
        }
        if let Some(v) = patch.mcp_port {
            // Reject the privileged range and 0 instead of clamping silently to a
            // random port the user never asked for.
            if v >= 1024 {
                self.mcp_port = v;
            }
        }
        if let Some(v) = patch.mcp_require_auth {
            self.mcp_require_auth = v;
        }
        if let Some(v) = patch.mcp_https {
            self.mcp_https = v;
        }
        if let Some(v) = &patch.skipped_update_version {
            // `Some(None)` is an explicit "clear", which is different from "not mentioned".
            self.skipped_update_version = v.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipping_an_update_version_can_also_be_cleared() {
        let mut settings = Settings::default();
        assert!(
            settings.skipped_update_version.is_none(),
            "nothing skipped yet"
        );

        settings.apply(&SettingsPatch {
            skipped_update_version: Some(Some("1.5.0".into())),
            ..Default::default()
        });
        assert_eq!(settings.skipped_update_version.as_deref(), Some("1.5.0"));

        // A patch that does not mention the field leaves it alone.
        settings.apply(&SettingsPatch::default());
        assert_eq!(
            settings.skipped_update_version.as_deref(),
            Some("1.5.0"),
            "an unrelated patch must not forget the skip"
        );

        settings.apply(&SettingsPatch {
            skipped_update_version: Some(None),
            ..Default::default()
        });
        assert!(
            settings.skipped_update_version.is_none(),
            "an explicit clear removes the skip"
        );
    }

    #[test]
    fn defaults_match_the_electron_service() {
        let s = Settings::default();
        assert_eq!(s.log_ring_size, 10_000);
        assert_eq!(s.kill_grace_ms, 5_000);
        assert_eq!(s.run_history_limit, 500);
        assert_eq!(s.readiness_timeout_ms, 60_000);
        assert_eq!(s.mcp_port, 6872);
        assert!(!s.mcp_enabled);
        assert!(s.mcp_require_auth);
        assert!(s.tray_enabled);
        assert_eq!(s.language, LanguageSetting::System);
    }

    #[test]
    fn patch_only_touches_the_fields_it_names() {
        let mut s = Settings::default();
        s.apply(&SettingsPatch {
            theme: Some(ThemeSetting::Dark),
            ..Default::default()
        });
        assert_eq!(s.theme, ThemeSetting::Dark);
        assert_eq!(s.mcp_port, 6872, "untouched fields keep their value");
    }

    #[test]
    fn patch_rejects_a_privileged_mcp_port_instead_of_silently_clamping() {
        let mut s = Settings::default();
        s.apply(&SettingsPatch {
            mcp_port: Some(80),
            ..Default::default()
        });
        assert_eq!(s.mcp_port, 6872);
    }

    #[test]
    fn patch_enforces_the_legacy_sampler_floor() {
        let mut s = Settings::default();
        s.apply(&SettingsPatch {
            dashboard_refresh_ms: Some(10),
            ..Default::default()
        });
        assert_eq!(s.dashboard_refresh_ms, 200);
    }

    #[test]
    fn language_and_theme_strings_round_trip() {
        for value in ["system", "en", "zh-CN"] {
            assert_eq!(
                LanguageSetting::parse(value).unwrap().as_str(),
                value,
                "language {value}"
            );
        }
        for value in ["light", "dark", "system"] {
            assert_eq!(ThemeSetting::parse(value).unwrap().as_str(), value);
        }
        assert_eq!(LanguageSetting::parse("fr"), None);
    }
}
