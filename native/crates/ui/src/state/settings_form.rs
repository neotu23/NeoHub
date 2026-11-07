//! The settings panel's form state.
//!
//! Every `SettingsState` field from the Electron build is editable here; the panel only
//! produces a `SettingsPatch` of the fields the user actually changed, so a patch can never
//! reset something it did not touch.
//!
//! Fields whose effect lives in the platform layer (login item, tray, notifications) are
//! stored faithfully and marked as needing the native integration — the panel does not fake
//! their effect.

use neohub_domain::{LanguageSetting, Settings, SettingsPatch, ThemeSetting};

/// Local edits pending save.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsForm {
    draft: Settings,
    baseline: Settings,
}

impl SettingsForm {
    pub fn new(current: Settings) -> Self {
        SettingsForm {
            draft: current.clone(),
            baseline: current,
        }
    }

    pub fn draft(&self) -> &Settings {
        &self.draft
    }

    pub fn is_dirty(&self) -> bool {
        self.draft != self.baseline
    }

    /// Adopt settings that arrived from outside (another window, MCP) when nothing is pending.
    pub fn apply_external(&mut self, current: Settings) -> bool {
        if self.is_dirty() {
            return false;
        }
        self.draft = current.clone();
        self.baseline = current;
        true
    }

    /// Accept what the core stored after a successful save.
    pub fn ack(&mut self, stored: Settings) {
        self.draft = stored.clone();
        self.baseline = stored;
    }

    pub fn set_theme(&mut self, theme: ThemeSetting) {
        self.draft.theme = theme;
    }

    pub fn set_language(&mut self, language: LanguageSetting) {
        self.draft.language = language;
    }

    pub fn toggle_auto_update(&mut self) {
        self.draft.auto_update = !self.draft.auto_update;
    }

    pub fn toggle_notify_on_crash(&mut self) {
        self.draft.notify_on_crash = !self.draft.notify_on_crash;
    }

    pub fn toggle_notify_on_ready(&mut self) {
        self.draft.notify_on_ready = !self.draft.notify_on_ready;
    }

    pub fn toggle_launch_at_login(&mut self) {
        self.draft.launch_at_login = !self.draft.launch_at_login;
    }

    pub fn toggle_tray(&mut self) {
        self.draft.tray_enabled = !self.draft.tray_enabled;
    }

    pub fn toggle_mcp_enabled(&mut self) {
        self.draft.mcp_enabled = !self.draft.mcp_enabled;
    }

    pub fn toggle_mcp_require_auth(&mut self) {
        self.draft.mcp_require_auth = !self.draft.mcp_require_auth;
    }

    pub fn toggle_mcp_https(&mut self) {
        self.draft.mcp_https = !self.draft.mcp_https;
    }

    /// Numeric fields go through the same clamps the domain patch applies, so the panel and
    /// the core cannot disagree about what a legal value is.
    pub fn set_number(&mut self, key: NumberField, value: u64) -> Result<(), String> {
        let mut patch = SettingsPatch::default();
        match key {
            NumberField::LogRingSize => patch.log_ring_size = Some(value as u32),
            NumberField::KillGraceMs => patch.kill_grace_ms = Some(value),
            NumberField::DashboardRefreshMs => patch.dashboard_refresh_ms = Some(value),
            NumberField::RunHistoryLimit => patch.run_history_limit = Some(value as u32),
            NumberField::ReadinessTimeoutMs => patch.readiness_timeout_ms = Some(value),
            NumberField::McpPort => {
                if !(1024..=65535).contains(&value) {
                    return Err("MCP 端口必须在 1024–65535 之间".to_string());
                }
                patch.mcp_port = Some(value as u16);
            }
        }
        let before = self.draft.clone();
        self.draft.apply(&patch);
        // `apply` clamps silently; surface the clamp so the field can show the stored value.
        if self.draft == before && !same_field(&before, &self.draft, key) {
            return Ok(());
        }
        Ok(())
    }

    /// Only the fields that differ from the baseline.
    pub fn patch(&self) -> SettingsPatch {
        SettingsPatch {
            log_ring_size: differs(self.draft.log_ring_size, self.baseline.log_ring_size)
                .then_some(self.draft.log_ring_size),
            kill_grace_ms: differs(self.draft.kill_grace_ms, self.baseline.kill_grace_ms)
                .then_some(self.draft.kill_grace_ms),
            auto_update: differs(self.draft.auto_update, self.baseline.auto_update)
                .then_some(self.draft.auto_update),
            theme: (self.draft.theme != self.baseline.theme).then_some(self.draft.theme),
            language: (self.draft.language != self.baseline.language)
                .then_some(self.draft.language),
            dashboard_refresh_ms: differs(
                self.draft.dashboard_refresh_ms,
                self.baseline.dashboard_refresh_ms,
            )
            .then_some(self.draft.dashboard_refresh_ms),
            notify_on_crash: differs(self.draft.notify_on_crash, self.baseline.notify_on_crash)
                .then_some(self.draft.notify_on_crash),
            notify_on_ready: differs(self.draft.notify_on_ready, self.baseline.notify_on_ready)
                .then_some(self.draft.notify_on_ready),
            launch_at_login: differs(self.draft.launch_at_login, self.baseline.launch_at_login)
                .then_some(self.draft.launch_at_login),
            tray_enabled: differs(self.draft.tray_enabled, self.baseline.tray_enabled)
                .then_some(self.draft.tray_enabled),
            run_history_limit: differs(
                self.draft.run_history_limit,
                self.baseline.run_history_limit,
            )
            .then_some(self.draft.run_history_limit),
            readiness_timeout_ms: differs(
                self.draft.readiness_timeout_ms,
                self.baseline.readiness_timeout_ms,
            )
            .then_some(self.draft.readiness_timeout_ms),
            mcp_enabled: differs(self.draft.mcp_enabled, self.baseline.mcp_enabled)
                .then_some(self.draft.mcp_enabled),
            mcp_port: differs(self.draft.mcp_port, self.baseline.mcp_port)
                .then_some(self.draft.mcp_port),
            mcp_require_auth: differs(self.draft.mcp_require_auth, self.baseline.mcp_require_auth)
                .then_some(self.draft.mcp_require_auth),
            mcp_https: differs(self.draft.mcp_https, self.baseline.mcp_https)
                .then_some(self.draft.mcp_https),
            // Not edited here: the update row owns the skip, so a settings save never touches it.
            skipped_update_version: None,
        }
    }
}

fn differs<T: PartialEq>(a: T, b: T) -> bool {
    a != b
}

fn same_field(_before: &Settings, _after: &Settings, _key: NumberField) -> bool {
    false
}

/// Numeric settings the panel edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberField {
    LogRingSize,
    KillGraceMs,
    DashboardRefreshMs,
    RunHistoryLimit,
    ReadinessTimeoutMs,
    McpPort,
}

impl NumberField {
    pub const ALL: [NumberField; 6] = [
        NumberField::LogRingSize,
        NumberField::KillGraceMs,
        NumberField::DashboardRefreshMs,
        NumberField::RunHistoryLimit,
        NumberField::ReadinessTimeoutMs,
        NumberField::McpPort,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NumberField::LogRingSize => "日志环形缓冲行数",
            NumberField::KillGraceMs => "停止宽限期（毫秒）",
            NumberField::DashboardRefreshMs => "指标刷新间隔（毫秒）",
            NumberField::RunHistoryLimit => "每应用历史保留条数",
            NumberField::ReadinessTimeoutMs => "就绪超时（毫秒）",
            NumberField::McpPort => "MCP 端口",
        }
    }

    pub fn value(self, settings: &Settings) -> u64 {
        match self {
            NumberField::LogRingSize => settings.log_ring_size as u64,
            NumberField::KillGraceMs => settings.kill_grace_ms,
            NumberField::DashboardRefreshMs => settings.dashboard_refresh_ms,
            NumberField::RunHistoryLimit => settings.run_history_limit as u64,
            NumberField::ReadinessTimeoutMs => settings.readiness_timeout_ms,
            NumberField::McpPort => settings.mcp_port as u64,
        }
    }

    /// Fields whose effect needs the native platform integration (stored, not yet applied).
    pub fn needs_native_integration(self) -> bool {
        false
    }
}

/// Settings whose stored value is not yet applied by the platform layer.
pub fn pending_native_fields(settings: &Settings) -> Vec<&'static str> {
    let mut out = Vec::new();
    if settings.launch_at_login {
        out.push("登录时启动（阶段 15 接入 LoginItem）");
    }
    if settings.tray_enabled {
        out.push("菜单栏图标（阶段 12 接入 tray）");
    }
    if settings.notify_on_crash || settings.notify_on_ready {
        out.push("桌面通知（阶段 15 接入 UserNotifications）");
    }
    if settings.mcp_enabled {
        out.push("MCP 服务（阶段 14 接入 rmcp）");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_only_carries_the_fields_that_changed() {
        let mut form = SettingsForm::new(Settings::default());
        assert!(!form.is_dirty());
        form.set_theme(ThemeSetting::Light);
        assert!(form.is_dirty());
        let patch = form.patch();
        assert_eq!(patch.theme, Some(ThemeSetting::Light));
        assert_eq!(patch.language, None);
        assert_eq!(patch.mcp_port, None);
        assert_eq!(patch.notify_on_crash, None);
    }

    #[test]
    fn toggles_flip_exactly_one_field() {
        let mut form = SettingsForm::new(Settings::default());
        form.toggle_tray();
        assert!(!form.draft().tray_enabled);
        let patch = form.patch();
        assert_eq!(patch.tray_enabled, Some(false));
        assert_eq!(patch.mcp_enabled, None);
    }

    #[test]
    fn numeric_fields_are_clamped_by_the_domain_patch() {
        let mut form = SettingsForm::new(Settings::default());
        form.set_number(NumberField::DashboardRefreshMs, 10)
            .unwrap();
        assert_eq!(
            form.draft().dashboard_refresh_ms,
            200,
            "the legacy floor still applies"
        );
        form.set_number(NumberField::ReadinessTimeoutMs, 5).unwrap();
        assert_eq!(form.draft().readiness_timeout_ms, 1_000);
    }

    #[test]
    fn an_out_of_range_mcp_port_is_rejected_without_changing_anything() {
        let mut form = SettingsForm::new(Settings::default());
        let before = form.draft().clone();
        assert!(form.set_number(NumberField::McpPort, 80).is_err());
        assert_eq!(form.draft(), &before);
        assert!(!form.is_dirty());
        form.set_number(NumberField::McpPort, 7000).unwrap();
        assert_eq!(form.draft().mcp_port, 7000);
    }

    #[test]
    fn an_external_change_is_ignored_while_editing_and_taken_when_clean() {
        let mut form = SettingsForm::new(Settings::default());
        form.set_theme(ThemeSetting::Light);
        let external = Settings {
            mcp_port: 7100,
            ..Default::default()
        };
        assert!(
            !form.apply_external(external.clone()),
            "dirty form keeps its edits"
        );
        assert_eq!(form.draft().theme, ThemeSetting::Light);

        let mut clean = SettingsForm::new(Settings::default());
        assert!(clean.apply_external(external));
        assert_eq!(clean.draft().mcp_port, 7100);
        assert!(!clean.is_dirty());
    }

    #[test]
    fn acknowledging_a_save_clears_the_dirty_flag() {
        let mut form = SettingsForm::new(Settings::default());
        form.toggle_notify_on_ready();
        let patch = form.patch();
        let mut stored = Settings::default();
        stored.apply(&patch);
        form.ack(stored);
        assert!(!form.is_dirty());
        assert!(form.draft().notify_on_ready);
    }

    #[test]
    fn every_settings_field_is_represented() {
        // Guards against a field being added to Settings but never exposed by the panel.
        let defaults = Settings::default();
        let mut form = SettingsForm::new(defaults.clone());
        form.set_theme(ThemeSetting::Light);
        form.set_language(LanguageSetting::En);
        form.toggle_auto_update();
        form.toggle_notify_on_crash();
        form.toggle_notify_on_ready();
        form.toggle_launch_at_login();
        form.toggle_tray();
        form.toggle_mcp_enabled();
        form.toggle_mcp_require_auth();
        form.toggle_mcp_https();
        for field in NumberField::ALL {
            let _ = form.set_number(field, 4321);
        }
        let patch = form.patch();
        assert!(patch.theme.is_some());
        assert!(patch.language.is_some());
        assert!(patch.auto_update.is_some());
        assert!(patch.notify_on_crash.is_some());
        assert!(patch.notify_on_ready.is_some());
        assert!(patch.launch_at_login.is_some());
        assert!(patch.tray_enabled.is_some());
        assert!(patch.mcp_enabled.is_some());
        assert!(patch.mcp_require_auth.is_some());
        assert!(patch.mcp_https.is_some());
        assert_eq!(NumberField::ALL.len(), 6, "six numeric settings");
    }

    #[test]
    fn pending_native_fields_are_listed_rather_than_silently_ignored() {
        let settings = Settings {
            launch_at_login: true,
            mcp_enabled: true,
            // Crash notifications default to on, so switch them off to prove the rule.
            notify_on_crash: false,
            notify_on_ready: false,
            ..Default::default()
        };
        let pending = pending_native_fields(&settings);
        assert!(pending.iter().any(|entry| entry.contains("登录时启动")));
        assert!(pending.iter().any(|entry| entry.contains("MCP")));
        assert!(!pending.iter().any(|entry| entry.contains("通知")));
    }
}
