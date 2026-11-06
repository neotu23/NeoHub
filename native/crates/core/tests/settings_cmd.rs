//! Settings through the command surface: read, patch, persist, and announce.

use neohub_core::commands::{CoreCommand, CoreReply};
use neohub_core::events::CoreEventKind;
use neohub_core::service::CoreService;
use neohub_domain::{LanguageSetting, Settings, SettingsPatch, ThemeSetting};
use neohub_storage::DataDir;
use std::time::{Duration, Instant};

#[tokio::test]
async fn settings_round_trip_through_commands_and_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let handle = service.handle();

    let defaults = match handle.request(CoreCommand::GetSettings).await.expect("get") {
        CoreReply::Settings(settings) => *settings,
        other => panic!("unexpected reply {other:?}"),
    };
    assert_eq!(defaults, Settings::default());

    let mut subscription = handle.subscribe();
    let stored = match handle
        .request(CoreCommand::UpdateSettings {
            patch: SettingsPatch {
                theme: Some(ThemeSetting::Light),
                language: Some(LanguageSetting::En),
                dashboard_refresh_ms: Some(500),
                mcp_port: Some(7100),
                tray_enabled: Some(false),
                ..Default::default()
            },
        })
        .await
        .expect("update")
    {
        CoreReply::Settings(settings) => *settings,
        other => panic!("unexpected reply {other:?}"),
    };
    assert_eq!(stored.theme, ThemeSetting::Light);
    assert_eq!(stored.language, LanguageSetting::En);
    assert_eq!(stored.dashboard_refresh_ms, 500);
    assert_eq!(stored.mcp_port, 7100);
    assert!(!stored.tray_enabled);
    assert_eq!(
        stored.notify_on_crash, defaults.notify_on_crash,
        "fields the patch did not name keep their value"
    );

    // Subscribers learn about it without polling.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let Some(event) = tokio::time::timeout(Duration::from_millis(300), subscription.recv())
            .await
            .ok()
            .flatten()
        else {
            assert!(
                Instant::now() < deadline,
                "no SettingsChanged event arrived"
            );
            continue;
        };
        if let CoreEventKind::SettingsChanged(settings) = event.kind {
            assert_eq!(settings.mcp_port, 7100);
            break;
        }
    }

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");

    // A fresh core on the same data directory reads the stored values back.
    let reopened = CoreService::open(DataDir::new(dir.path())).expect("reopen");
    let handle = reopened.handle();
    match handle.request(CoreCommand::GetSettings).await.expect("get") {
        CoreReply::Settings(settings) => {
            assert_eq!(settings.theme, ThemeSetting::Light);
            assert_eq!(settings.mcp_port, 7100);
            assert_eq!(settings.dashboard_refresh_ms, 500);
            assert!(!settings.tray_enabled);
        }
        other => panic!("unexpected reply {other:?}"),
    }
    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[tokio::test]
async fn an_out_of_range_value_is_clamped_by_the_core_not_just_the_ui() {
    let dir = tempfile::tempdir().unwrap();
    let service = CoreService::open(DataDir::new(dir.path())).expect("service");
    let handle = service.handle();

    let stored = match handle
        .request(CoreCommand::UpdateSettings {
            patch: SettingsPatch {
                dashboard_refresh_ms: Some(1),
                readiness_timeout_ms: Some(10),
                // A privileged port must not be accepted even if a client sends it directly.
                mcp_port: Some(80),
                ..Default::default()
            },
        })
        .await
        .expect("update")
    {
        CoreReply::Settings(settings) => *settings,
        other => panic!("unexpected reply {other:?}"),
    };
    assert_eq!(stored.dashboard_refresh_ms, 200);
    assert_eq!(stored.readiness_timeout_ms, 1_000);
    assert_eq!(stored.mcp_port, 6872, "privileged ports are refused");

    handle
        .request(CoreCommand::Shutdown)
        .await
        .expect("shutdown");
}

#[test]
fn unknown_settings_fields_are_a_parse_error_not_a_silent_no_op() {
    // The patch type denies unknown fields, so a typo from any client is rejected loudly.
    let parsed: Result<SettingsPatch, _> =
        serde_json::from_str(r#"{"theme":"light","them":"dark"}"#);
    assert!(parsed.is_err(), "unknown keys must fail: {parsed:?}");
    let ok: SettingsPatch = serde_json::from_str(r#"{"theme":"light"}"#).expect("valid patch");
    assert_eq!(ok.theme, Some(ThemeSetting::Light));
}
