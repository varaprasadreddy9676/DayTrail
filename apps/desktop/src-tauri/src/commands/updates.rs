use std::time::Duration;

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::error::CommandError;

const RELEASES_API: &str =
    "https://api.github.com/repos/varaprasadreddy9676/DayTrail/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/varaprasadreddy9676/DayTrail/releases/latest";
const SAME_VERSION_REBUILD_GRACE_SECS: i64 = 2 * 60 * 60;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub current_build_unix: Option<i64>,
    pub latest_version: Option<String>,
    pub latest_build_at: Option<String>,
    pub update_available: bool,
    pub release_url: String,
    pub download_url: Option<String>,
    /// Markdown release notes from the GitHub release body, if any.
    pub release_notes: Option<String>,
    /// Set when the check could not complete (e.g. offline). UI shows this
    /// instead of a misleading "up to date".
    pub error: Option<String>,
    /// True when the app was installed via the Homebrew cask. The UI uses this
    /// to show `brew upgrade --cask daytrail` instead of a DMG download link.
    pub homebrew_managed: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: Option<String>,
    published_at: Option<String>,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: Option<String>,
    updated_at: Option<String>,
    created_at: Option<String>,
}

/// The running app version. No network — used to show the version label.
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Update check used by the startup reminder and the manual Settings action.
/// Asks GitHub for the latest release tag and compares it to the running
/// version/build timestamp.
#[tauri::command]
pub fn check_for_updates() -> Result<UpdateInfo, CommandError> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let current_build_unix = current_build_unix();
    let mut info = UpdateInfo {
        current_version: current.clone(),
        current_build_unix,
        latest_version: None,
        latest_build_at: None,
        update_available: false,
        release_url: RELEASES_PAGE.to_string(),
        download_url: None,
        release_notes: None,
        error: None,
        homebrew_managed: is_homebrew_managed(),
    };

    match fetch_latest_release() {
        Ok(release) => {
            let normalized = release.tag_name.trim_start_matches('v').to_string();
            let release_asset = best_download_asset(&release.assets);
            let latest_build_at = release_asset
                .and_then(|asset| asset.updated_at.as_deref().or(asset.created_at.as_deref()))
                .or(release.published_at.as_deref())
                .map(ToString::to_string);

            info.release_url = release.html_url.unwrap_or_else(|| RELEASES_PAGE.to_string());
            info.download_url = release_asset.and_then(|asset| asset.browser_download_url.clone());
            info.latest_build_at = latest_build_at.clone();
            info.release_notes = release
                .body
                .as_deref()
                .map(str::trim)
                .filter(|body| !body.is_empty())
                .map(ToString::to_string);
            info.update_available = is_newer(&normalized, &current)
                || same_version_rebuild_available(
                    &normalized,
                    &current,
                    latest_build_at.as_deref(),
                    current_build_unix,
                );
            info.latest_version = Some(normalized);
        }
        Err(message) => {
            info.error = Some(message);
        }
    }
    Ok(info)
}

fn current_build_unix() -> Option<i64> {
    option_env!("DAYTRAIL_BUILD_UNIX").and_then(|value| value.parse().ok())
}

fn fetch_latest_release() -> Result<GitHubRelease, String> {
    let response = ureq::get(RELEASES_API)
        .timeout(Duration::from_secs(10))
        .set("User-Agent", "DayTrail-Updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| format!("Could not reach GitHub: {error}"))?;
    response
        .into_json()
        .map_err(|error| format!("Unexpected response from GitHub: {error}"))
}

/// True when `candidate` is a strictly newer semantic version than `current`.
/// Falls back to a plain inequality if either side isn't dotted-numeric.
fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_semver(candidate), parse_semver(current)) {
        (Some(a), Some(b)) => a > b,
        _ => candidate != current,
    }
}

fn same_version_rebuild_available(
    candidate: &str,
    current: &str,
    latest_build_at: Option<&str>,
    current_build_unix: Option<i64>,
) -> bool {
    if normalize_version(candidate) != normalize_version(current) {
        return false;
    }

    let Some(latest_unix) = latest_build_at.and_then(parse_rfc3339_unix) else {
        return false;
    };

    match current_build_unix {
        Some(current_unix) if current_unix > 0 => {
            latest_unix > current_unix + SAME_VERSION_REBUILD_GRACE_SECS
        }
        _ => true,
    }
}

fn normalize_version(value: &str) -> String {
    value.trim().trim_start_matches('v').to_string()
}

fn best_download_asset(assets: &[GitHubReleaseAsset]) -> Option<&GitHubReleaseAsset> {
    assets
        .iter()
        .filter(|asset| platform_asset_score(&asset.name) > 0)
        .max_by_key(|asset| {
            (
                platform_asset_score(&asset.name),
                asset
                    .updated_at
                    .as_deref()
                    .or(asset.created_at.as_deref())
                    .and_then(parse_rfc3339_unix)
                    .unwrap_or(0),
            )
        })
}

#[cfg(target_os = "macos")]
fn platform_asset_score(name: &str) -> u8 {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".dmg") {
        30
    } else if name.ends_with(".app.tar.gz") || name.ends_with(".app.tar") {
        20
    } else {
        0
    }
}

#[cfg(target_os = "windows")]
fn platform_asset_score(name: &str) -> u8 {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".exe") {
        30
    } else if name.ends_with(".msi") {
        20
    } else {
        0
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_asset_score(name: &str) -> u8 {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".appimage") || name.ends_with(".deb") || name.ends_with(".rpm") {
        10
    } else {
        0
    }
}

fn parse_rfc3339_unix(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|datetime| datetime.timestamp())
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

/// Run `brew upgrade --cask daytrail` in the background and return the combined
/// stdout+stderr so the UI can show progress. Errors are surfaced as a
/// CommandError so the UI can display them inline.
#[tauri::command]
pub fn brew_upgrade_daytrail() -> Result<String, CommandError> {
    let brew = find_brew_binary()
        .ok_or_else(|| CommandError::from(anyhow::anyhow!("brew binary not found")))?;

    let output = std::process::Command::new(&brew)
        .args(["upgrade", "--cask", "daytrail"])
        .output()
        .map_err(|e| CommandError::from(anyhow::anyhow!("failed to launch brew: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}{stderr}").trim().to_string();

    if output.status.success() {
        Ok(combined)
    } else {
        let msg = if combined.is_empty() {
            "brew upgrade failed".to_string()
        } else {
            combined
        };
        Err(CommandError::from(anyhow::anyhow!("{msg}")))
    }
}

#[cfg(target_os = "macos")]
fn find_brew_binary() -> Option<std::path::PathBuf> {
    for path in ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"] {
        let p = std::path::Path::new(path);
        if p.exists() {
            return Some(p.to_owned());
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn find_brew_binary() -> Option<std::path::PathBuf> {
    None
}

/// True when a Homebrew Caskroom receipt exists for daytrail, meaning the app
/// was installed (and should be updated) via `brew upgrade --cask daytrail`.
fn is_homebrew_managed() -> bool {
    // Homebrew creates a versioned receipt directory under Caskroom when a
    // cask is installed. Check both Apple Silicon and Intel prefixes.
    #[cfg(target_os = "macos")]
    {
        let prefixes = ["/opt/homebrew/Caskroom/daytrail", "/usr/local/Caskroom/daytrail"];
        prefixes.iter().any(|p| std::path::Path::new(p).exists())
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{
        best_download_asset, is_newer, parse_semver, same_version_rebuild_available,
        GitHubReleaseAsset,
    };

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

    #[test]
    fn detects_same_version_rebuilt_installer() {
        assert!(same_version_rebuild_available(
            "0.1.1",
            "0.1.1",
            Some("2026-06-02T12:00:00Z"),
            Some(1_780_000_000),
        ));
        assert!(!same_version_rebuild_available(
            "0.1.1",
            "0.1.1",
            Some("2026-06-02T12:00:00Z"),
            Some(1_780_000_000_000),
        ));
        assert!(!same_version_rebuild_available(
            "0.1.2",
            "0.1.1",
            Some("2026-06-02T12:00:00Z"),
            Some(1),
        ));
    }

    #[test]
    fn treats_unknown_build_age_as_updateable_for_same_version_releases() {
        assert!(same_version_rebuild_available(
            "0.1.1",
            "v0.1.1",
            Some("2026-06-02T12:00:00Z"),
            None,
        ));
    }

    #[test]
    fn chooses_platform_installer_asset() {
        let assets = vec![
            GitHubReleaseAsset {
                name: "daytrail.txt".into(),
                browser_download_url: Some("https://example.com/readme".into()),
                updated_at: Some("2026-06-02T12:00:00Z".into()),
                created_at: None,
            },
            GitHubReleaseAsset {
                #[cfg(target_os = "windows")]
                name: "DayTrail_0.1.1_x64-setup.exe".into(),
                #[cfg(target_os = "macos")]
                name: "DayTrail_0.1.1_aarch64.dmg".into(),
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                name: "DayTrail_0.1.1.AppImage".into(),
                browser_download_url: Some("https://example.com/download".into()),
                updated_at: Some("2026-06-02T13:00:00Z".into()),
                created_at: None,
            },
        ];

        assert_eq!(
            best_download_asset(&assets).and_then(|asset| asset.browser_download_url.as_deref()),
            Some("https://example.com/download"),
        );
    }
}
