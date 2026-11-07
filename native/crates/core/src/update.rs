//! Update check.
//!
//! Scope is deliberate and matches what a self-built desktop app can honestly do:
//!
//! * **Check** the latest published release and tell the user a newer one exists.
//! * **Open** the release page so they can download it.
//!
//! It does **not** download or install: doing that safely needs a signed, notarized bundle and
//! an installer step, and an update path that can install an unsigned build would be a security
//! regression, not a feature. The Electron build had the same gap (its updater was never wired to
//! a live feed), so this is parity plus a real check, not a shortfall hidden behind a button.
//!
//! Everything except the HTTP call is a pure function, so the interesting parts — version
//! comparison, JSON shape, failure handling — are testable without the network.

use serde::Deserialize;
use std::time::Duration;

/// Where releases live.
pub const RELEASES_API: &str = "https://api.github.com/repos/neotu23/NeoHub/releases/latest";
/// Human page for the same release.
pub const RELEASES_PAGE: &str = "https://github.com/neotu23/NeoHub/releases/latest";

/// What the checker found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Running the newest published version.
    UpToDate { current: String },
    /// A newer version exists.
    Available {
        current: String,
        latest: String,
        notes: Option<String>,
        url: String,
    },
}

impl UpdateStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, UpdateStatus::Available { .. })
    }

    /// One line for the UI to show.
    pub fn summary(&self) -> String {
        match self {
            UpdateStatus::UpToDate { current } => format!("已是最新版本（{current}）"),
            UpdateStatus::Available { latest, .. } => format!("有新版本 {latest} 可下载"),
        }
    }
}

/// The subset of the GitHub release payload we use.
#[derive(Debug, Deserialize)]
struct ReleasePayload {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
}

/// Normalise a release tag (`v1.4.0`, `1.4.0`, `release-1.4.0`) to a semver string.
pub fn normalise_version(tag: &str) -> Option<String> {
    let trimmed = tag.trim();
    let candidate = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    // Take the trailing `x.y.z` run so `release-1.4.0` still parses; require all three parts so a
    // stray number is not mistaken for a version.
    let tail: String = candidate
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .collect();
    if tail.is_empty() {
        return None;
    }
    let version = semver::Version::parse(&tail).ok()?;
    Some(version.to_string())
}

/// Apply the user's "skip this version" choice to a check result.
///
/// A skipped release is reported as up to date: showing a banner the user already dismissed is
/// how update prompts get ignored for good. A *newer* release still shows, because the skip is
/// about one version, not about updates.
pub fn respect_skip(status: UpdateStatus, skipped: Option<&str>) -> UpdateStatus {
    let (UpdateStatus::Available { latest, .. }, Some(skipped)) = (&status, skipped) else {
        return status;
    };
    let Some(skipped) = normalise_version(skipped) else {
        return status;
    };
    if normalise_version(latest).as_deref() == Some(skipped.as_str()) {
        UpdateStatus::UpToDate {
            current: match &status {
                UpdateStatus::Available { current, .. } => current.clone(),
                UpdateStatus::UpToDate { current } => current.clone(),
            },
        }
    } else {
        status
    }
}

/// Compare the running version with the newest published one.
pub fn compare(
    current: &str,
    latest_tag: &str,
    notes: Option<String>,
    url: Option<String>,
) -> UpdateStatus {
    let current_version = normalise_version(current);
    let latest_version = normalise_version(latest_tag);
    match (current_version, latest_version) {
        (Some(current), Some(latest)) => {
            let current_parsed = semver::Version::parse(&current).expect("normalised");
            let latest_parsed = semver::Version::parse(&latest).expect("normalised");
            if latest_parsed > current_parsed {
                UpdateStatus::Available {
                    current,
                    latest,
                    notes,
                    url: url.unwrap_or_else(|| RELEASES_PAGE.to_string()),
                }
            } else {
                UpdateStatus::UpToDate { current }
            }
        }
        // An unparsable version on either side means "cannot tell"; reporting an update we cannot
        // substantiate would be worse than saying nothing.
        _ => UpdateStatus::UpToDate {
            current: current.to_string(),
        },
    }
}

/// Ask GitHub for the latest release.
///
/// Runs blocking: it is called from a background task, and a short timeout keeps a flaky network
/// from holding a thread for long.
pub fn check_blocking(current: &str) -> Result<UpdateStatus, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("NeoHub/{current}"))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(RELEASES_API)
        .send()
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("release lookup failed: HTTP {}", response.status()));
    }
    let payload: ReleasePayload = response.json().map_err(|error| error.to_string())?;
    let notes = payload
        .body
        .filter(|body| !body.trim().is_empty())
        .or(payload.name);
    Ok(compare(current, &payload.tag_name, notes, payload.html_url))
}

/// Open the releases page so the user can download the new build themselves.
pub fn open_releases_page() -> Result<(), String> {
    std::process::Command::new("/usr/bin/open")
        .arg(RELEASES_PAGE)
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
    fn tags_are_normalised_to_semver() {
        assert_eq!(normalise_version("v1.4.0").as_deref(), Some("1.4.0"));
        assert_eq!(normalise_version("1.4.0").as_deref(), Some("1.4.0"));
        assert_eq!(normalise_version("V2.0.1").as_deref(), Some("2.0.1"));
        assert_eq!(normalise_version("release-1.4.0").as_deref(), Some("1.4.0"));
        assert_eq!(normalise_version("nope"), None);
        assert_eq!(
            normalise_version("v1.4"),
            None,
            "a partial version is not a version"
        );
    }

    #[test]
    fn a_newer_release_is_reported() {
        let status = compare(
            "1.4.0",
            "v1.5.0",
            Some("notes".into()),
            Some("https://x".into()),
        );
        match status {
            UpdateStatus::Available {
                current,
                latest,
                notes,
                url,
            } => {
                assert_eq!(current, "1.4.0");
                assert_eq!(latest, "1.5.0");
                assert_eq!(notes.as_deref(), Some("notes"));
                assert_eq!(url, "https://x");
            }
            other => panic!("expected an update, got {other:?}"),
        }
    }

    #[test]
    fn the_same_or_an_older_release_is_not_an_update() {
        assert!(!compare("1.4.0", "v1.4.0", None, None).is_available());
        assert!(!compare("1.4.0", "v1.3.9", None, None).is_available());
        // Numeric comparison, not string: 1.10 is newer than 1.9.
        assert!(!compare("1.10.0", "v1.9.9", None, None).is_available());
        assert!(compare("1.9.9", "v1.10.0", None, None).is_available());
    }

    #[test]
    fn an_unparsable_version_reports_nothing_rather_than_guessing() {
        let status = compare("dev", "v1.5.0", None, None);
        assert_eq!(
            status,
            UpdateStatus::UpToDate {
                current: "dev".into()
            }
        );
        let status = compare("1.4.0", "nightly", None, None);
        assert!(!status.is_available());
    }

    #[test]
    fn the_summary_names_the_version() {
        assert!(
            compare("1.4.0", "v1.5.0", None, None)
                .summary()
                .contains("1.5.0")
        );
        assert!(
            compare("1.4.0", "v1.4.0", None, None)
                .summary()
                .contains("1.4.0")
        );
    }

    #[test]
    fn a_skipped_version_is_not_offered_again() {
        let status = compare("1.4.0", "v1.5.0", None, None);
        assert!(status.is_available());
        let skipped = respect_skip(status.clone(), Some("1.5.0"));
        assert!(!skipped.is_available(), "the skipped release stops asking");
        assert!(skipped.summary().contains("1.4.0"));
    }

    #[test]
    fn a_newer_release_still_shows_after_a_skip() {
        let status = compare("1.4.0", "v1.6.0", None, None);
        assert!(
            respect_skip(status, Some("1.5.0")).is_available(),
            "skipping 1.5.0 must not hide 1.6.0"
        );
    }

    #[test]
    fn a_nonsense_skip_value_is_ignored() {
        let status = compare("1.4.0", "v1.5.0", None, None);
        assert!(
            respect_skip(status.clone(), Some("nonsense")).is_available(),
            "an unparsable skip cannot silently hide updates"
        );
        assert!(respect_skip(status, None).is_available());
    }

    #[test]
    fn a_missing_release_url_falls_back_to_the_releases_page() {
        let status = compare("1.4.0", "v9.9.9", None, None);
        match status {
            UpdateStatus::Available { url, .. } => assert_eq!(url, RELEASES_PAGE),
            other => panic!("expected an update, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;

    /// Real request against the public releases API. Ignored by default: CI and offline
    /// machines must not fail because GitHub is unreachable.
    #[test]
    #[ignore = "requires network access to api.github.com"]
    fn the_live_release_lookup_parses() {
        match check_blocking("0.0.1") {
            Ok(status) => {
                // Whatever is published, a 0.0.1 client must see it as newer.
                assert!(
                    status.is_available(),
                    "expected an update from 0.0.1: {status:?}"
                );
            }
            Err(error) => {
                // A rate limit is not a bug in this code; report it instead of failing the run.
                assert!(
                    error.contains("403") || error.contains("404") || error.contains("failed"),
                    "unexpected error shape: {error}"
                );
            }
        }
    }
}
