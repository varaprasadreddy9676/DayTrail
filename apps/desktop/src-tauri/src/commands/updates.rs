use std::time::Duration;

use serde::Serialize;

use crate::error::CommandError;

const RELEASES_API: &str =
    "https://api.github.com/repos/varaprasadreddy9676/DayTrail/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/varaprasadreddy9676/DayTrail/releases/latest";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub release_url: String,
    /// Set when the check could not complete (e.g. offline). UI shows this
    /// instead of a misleading "up to date".
    pub error: Option<String>,
}

/// The running app version. No network — used to show the version label.
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// On-demand update check. Asks GitHub for the latest release tag and compares
/// it to the running version. Runs only when the user clicks "Check for
/// updates" — DayTrail never phones home on its own.
#[tauri::command]
pub fn check_for_updates() -> Result<UpdateInfo, CommandError> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let mut info = UpdateInfo {
        current_version: current.clone(),
        latest_version: None,
        update_available: false,
        release_url: RELEASES_PAGE.to_string(),
        error: None,
    };

    match fetch_latest_tag() {
        Ok(latest) => {
            let normalized = latest.trim_start_matches('v').to_string();
            info.update_available = is_newer(&normalized, &current);
            info.latest_version = Some(normalized);
        }
        Err(message) => {
            info.error = Some(message);
        }
    }
    Ok(info)
}

fn fetch_latest_tag() -> Result<String, String> {
    let response = ureq::get(RELEASES_API)
        .timeout(Duration::from_secs(10))
        .set("User-Agent", "DayTrail-Updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| format!("Could not reach GitHub: {error}"))?;
    let json: serde_json::Value = response
        .into_json()
        .map_err(|error| format!("Unexpected response from GitHub: {error}"))?;
    json.get("tag_name")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| "No published release found yet.".to_string())
}

/// True when `candidate` is a strictly newer semantic version than `current`.
/// Falls back to a plain inequality if either side isn't dotted-numeric.
fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_semver(candidate), parse_semver(current)) {
        (Some(a), Some(b)) => a > b,
        _ => candidate != current,
    }
}

fn parse_semver(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    // Drop any pre-release/build suffix on the patch component (e.g. "2-rc1").
    let patch_raw = parts.next().unwrap_or("0");
    let patch = patch_raw
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::{is_newer, parse_semver};

    #[test]
    fn detects_newer_versions_numerically() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.0")); // not lexical
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn parses_versions_with_suffixes() {
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_semver("2.0"), Some((2, 0, 0)));
    }
}
