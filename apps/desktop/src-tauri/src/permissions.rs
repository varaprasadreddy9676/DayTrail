use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use crate::models::{CapturePermissionCheck, CapturePermissionSummary};

const ACCESSIBILITY_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const AUTOMATION_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation";

pub fn capture_permission_summary() -> CapturePermissionSummary {
    #[cfg(target_os = "macos")]
    {
        capture_permission_summary_for_platform("macos", macos_accessibility_trusted(false))
    }

    #[cfg(not(target_os = "macos"))]
    {
        capture_permission_summary_for_platform(std::env::consts::OS, true)
    }
}

pub fn capture_permission_summary_for_platform(
    platform: &str,
    accessibility_granted: bool,
) -> CapturePermissionSummary {
    capture_permission_summary_for_platform_with_browser_check(
        platform,
        accessibility_granted,
        None,
    )
}

fn capture_permission_summary_for_platform_with_browser_check(
    platform: &str,
    accessibility_granted: bool,
    browser_automation_check: Option<CapturePermissionCheck>,
) -> CapturePermissionSummary {
    let executable_path = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok().or(Some(path)))
        .map(|path| path.display().to_string());
    let app_path = executable_path
        .as_deref()
        .and_then(app_bundle_path_from_executable);
    let diagnostics = permission_diagnostics(
        platform,
        accessibility_granted,
        app_path.as_deref(),
        executable_path.as_deref(),
    );
    let checks = match platform {
        "macos" => macos_permission_checks(accessibility_granted, browser_automation_check),
        "windows" => windows_permission_checks(),
        _ => default_permission_checks(),
    };

    let setup_required = checks
        .iter()
        .any(|check| check.required && check.status != "granted");
    let all_required_granted = checks
        .iter()
        .filter(|check| check.required)
        .all(|check| check.status == "granted");

    CapturePermissionSummary {
        platform: platform.to_string(),
        setup_required,
        all_required_granted,
        app_path,
        executable_path,
        restart_recommended: setup_required,
        diagnostics,
        checks,
    }
}

fn app_bundle_path_from_executable(executable_path: &str) -> Option<String> {
    let path = Path::new(executable_path);
    let components = path.components().collect::<Vec<_>>();
    let contents_index = components
        .iter()
        .position(|component| component.as_os_str() == "Contents")?;
    let app_component = contents_index.checked_sub(1).and_then(|index| {
        let rendered = components[index].as_os_str().to_string_lossy();
        rendered.ends_with(".app").then_some(index)
    })?;

    let mut app_path = PathBuf::new();
    for component in components.iter().take(app_component + 1) {
        app_path.push(component.as_os_str());
    }
    Some(app_path.display().to_string())
}

fn permission_diagnostics(
    platform: &str,
    accessibility_granted: bool,
    app_path: Option<&str>,
    executable_path: Option<&str>,
) -> Vec<String> {
    if platform != "macos" || accessibility_granted {
        if platform == "windows" {
            return vec![
                "No Windows privacy permission is required for normal active-window tracking."
                    .to_string(),
                "Browser URLs still require the DayTrail browser extension and native host bridge — install these from Settings > Bridges.".to_string(),
                "Editor folders require the VS Code/Cursor extension. Terminal commands require the PowerShell terminal bridge.".to_string(),
            ];
        }
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    if let Some(app_path) = app_path {
        diagnostics.push(format!(
            "Enable Accessibility for this exact app: {app_path}"
        ));
        if !app_path.starts_with("/Applications/") {
            diagnostics.push(
                "This copy is not running from /Applications. macOS can show another DayTrail entry for a different copy.".to_string(),
            );
        }
    } else if let Some(executable_path) = executable_path {
        diagnostics.push(format!(
            "Enable Accessibility for the binary at: {executable_path}"
        ));
    }
    diagnostics.push(
        "If DayTrail is already enabled, quit and reopen the same app copy, then recheck."
            .to_string(),
    );
    diagnostics
}

fn macos_permission_checks(
    accessibility_granted: bool,
    browser_automation_check: Option<CapturePermissionCheck>,
) -> Vec<CapturePermissionCheck> {
    vec![
        CapturePermissionCheck {
            id: "accessibility".to_string(),
            label: "Accessibility".to_string(),
            required: true,
            status: if accessibility_granted {
                "granted".to_string()
            } else {
                "missing".to_string()
            },
            detail: if accessibility_granted {
                "DayTrail can read the active app and focused window title.".to_string()
            } else {
                "Required for accurate app and window-title tracking.".to_string()
            },
            settings_label: Some("Privacy & Security > Accessibility".to_string()),
            settings_url: Some(ACCESSIBILITY_URL.to_string()),
            action_label: Some("Open Accessibility Settings".to_string()),
        },
        browser_automation_check.unwrap_or_else(default_browser_automation_check),
        CapturePermissionCheck {
            id: "screen-recording".to_string(),
            label: "Screen Recording".to_string(),
            required: false,
            status: "not_required".to_string(),
            detail: "Not requested because screenshots are off by default.".to_string(),
            settings_label: None,
            settings_url: None,
            action_label: None,
        },
    ]
}

fn default_browser_automation_check() -> CapturePermissionCheck {
    CapturePermissionCheck {
        id: "browser-automation".to_string(),
        label: "Browser automation".to_string(),
        required: false,
        status: "user_prompt".to_string(),
        detail: "Lets DayTrail read active tab URLs from supported browsers. Click \"Grant now\" while the browser is open, or macOS will ask the first time DayTrail captures a tab.".to_string(),
        settings_label: Some("Privacy & Security > Automation".to_string()),
        settings_url: Some(AUTOMATION_URL.to_string()),
        action_label: Some("Grant now".to_string()),
    }
}

#[derive(Debug, Clone, Copy)]
struct BrowserAutomationTarget {
    app_name: &'static str,
    script_kind: BrowserAutomationScriptKind,
}

#[derive(Debug, Clone, Copy)]
enum BrowserAutomationScriptKind {
    Safari,
    Chromium,
}

#[cfg(target_os = "macos")]
const MACOS_AUTOMATION_BROWSERS: &[BrowserAutomationTarget] = &[
    BrowserAutomationTarget {
        app_name: "Safari",
        script_kind: BrowserAutomationScriptKind::Safari,
    },
    BrowserAutomationTarget {
        app_name: "Google Chrome",
        script_kind: BrowserAutomationScriptKind::Chromium,
    },
    BrowserAutomationTarget {
        app_name: "Google Chrome Canary",
        script_kind: BrowserAutomationScriptKind::Chromium,
    },
    BrowserAutomationTarget {
        app_name: "Microsoft Edge",
        script_kind: BrowserAutomationScriptKind::Chromium,
    },
    BrowserAutomationTarget {
        app_name: "Brave Browser",
        script_kind: BrowserAutomationScriptKind::Chromium,
    },
    BrowserAutomationTarget {
        app_name: "ChatGPT Atlas",
        script_kind: BrowserAutomationScriptKind::Chromium,
    },
    BrowserAutomationTarget {
        app_name: "Arc",
        script_kind: BrowserAutomationScriptKind::Chromium,
    },
    BrowserAutomationTarget {
        app_name: "Chromium",
        script_kind: BrowserAutomationScriptKind::Chromium,
    },
    BrowserAutomationTarget {
        app_name: "Opera",
        script_kind: BrowserAutomationScriptKind::Chromium,
    },
    BrowserAutomationTarget {
        app_name: "Vivaldi",
        script_kind: BrowserAutomationScriptKind::Chromium,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserAutomationProbeStatus {
    Granted,
    Denied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowserAutomationProbeResult {
    browser: String,
    status: BrowserAutomationProbeStatus,
}

fn browser_automation_check_from_probe_results(
    results: &[BrowserAutomationProbeResult],
) -> CapturePermissionCheck {
    let mut granted = Vec::new();
    let mut blocked = Vec::new();

    for result in results {
        match result.status {
            BrowserAutomationProbeStatus::Granted => granted.push(result.browser.as_str()),
            BrowserAutomationProbeStatus::Denied | BrowserAutomationProbeStatus::Failed => {
                blocked.push(result.browser.as_str())
            }
        }
    }

    let (status, detail, action_label) = if results.is_empty() {
        (
            "not_running",
            "Open Safari, Chrome, Brave, Edge, Arc, Chromium, Opera, Vivaldi, or ChatGPT Atlas, then click \"Grant now\" to verify browser automation.".to_string(),
            "Grant now",
        )
    } else if blocked.is_empty() {
        (
            "granted",
            format!(
                "Automation verified for running browsers: {}. DayTrail can read active tab URLs when those browsers are frontmost.",
                granted.join(", ")
            ),
            "Recheck",
        )
    } else if granted.is_empty() {
        (
            "missing",
            format!(
                "macOS did not allow DayTrail to automate {}. Open Automation Settings and enable DayTrail for the browser, then click \"Grant now\" again.",
                blocked.join(", ")
            ),
            "Grant now",
        )
    } else {
        (
            "limited",
            format!(
                "Automation verified for {}. Still needs access for {}.",
                granted.join(", "),
                blocked.join(", ")
            ),
            "Grant now",
        )
    };

    CapturePermissionCheck {
        id: "browser-automation".to_string(),
        label: "Browser automation".to_string(),
        required: false,
        status: status.to_string(),
        detail: detail.to_string(),
        settings_label: Some("Privacy & Security > Automation".to_string()),
        settings_url: Some(AUTOMATION_URL.to_string()),
        action_label: Some(action_label.to_string()),
    }
}

fn windows_permission_checks() -> Vec<CapturePermissionCheck> {
    vec![
        CapturePermissionCheck {
            id: "window-metadata".to_string(),
            label: "Active app metadata".to_string(),
            required: false,
            status: "granted".to_string(),
            detail: "Windows allows normal active app and window-title tracking without a separate privacy grant.".to_string(),
            settings_label: None,
            settings_url: None,
            action_label: None,
        },
        CapturePermissionCheck {
            id: "elevated-apps".to_string(),
            label: "Elevated apps".to_string(),
            required: false,
            status: "limited".to_string(),
            detail: "Apps running as administrator can hide window details from a non-admin DayTrail process.".to_string(),
            settings_label: Some(
                "Run DayTrail as administrator only if you need admin-app capture.".to_string(),
            ),
            settings_url: None,
            action_label: None,
        },
    ]
}

fn default_permission_checks() -> Vec<CapturePermissionCheck> {
    vec![CapturePermissionCheck {
        id: "window-metadata".to_string(),
        label: "Active app metadata".to_string(),
        required: false,
        status: "granted".to_string(),
        detail:
            "This platform does not require a separate OS privacy grant for active-window metadata."
                .to_string(),
        settings_label: None,
        settings_url: None,
        action_label: None,
    }]
}

pub fn open_permission_settings(permission_id: &str) -> Result<CapturePermissionSummary> {
    #[cfg(target_os = "macos")]
    {
        let url = match permission_id {
            "accessibility" => ACCESSIBILITY_URL,
            "browser-automation" => AUTOMATION_URL,
            "automation" => AUTOMATION_URL,
            "screen-recording" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            other => bail!("unknown macOS permission setting: {other}"),
        };

        let status = std::process::Command::new("open")
            .arg(url)
            .status()
            .with_context(|| format!("failed to open macOS settings for {permission_id}"))?;

        if !status.success() {
            return Err(anyhow!(
                "macOS settings failed to open for {permission_id}: {status}"
            ));
        }

        Ok(capture_permission_summary())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = permission_id;
        Ok(capture_permission_summary())
    }
}

pub fn request_capture_permission(permission_id: &str) -> Result<CapturePermissionSummary> {
    #[cfg(target_os = "macos")]
    {
        if permission_id == "accessibility" {
            let _ = macos_accessibility_trusted(true);
        }

        open_permission_settings(permission_id)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = permission_id;
        Ok(capture_permission_summary())
    }
}

/// Proactively trigger the macOS automation TCC prompt for every browser
/// that is currently running.  macOS only shows the dialog when the app
/// actually attempts to script a target, so we fire a harmless
/// `tell application X to get name` for each running browser.
/// Non-running browsers are skipped — osascript would otherwise launch them.
pub fn trigger_browser_automation_prompt() -> Result<CapturePermissionSummary> {
    #[cfg(target_os = "macos")]
    {
        let mut results = Vec::new();

        for browser in MACOS_AUTOMATION_BROWSERS {
            let running = browser_is_running(browser.app_name);

            if running {
                let script = browser_automation_probe_script(*browser);
                let result = std::process::Command::new("osascript")
                    .args(["-e", &script])
                    .output();
                let status = match result {
                    Ok(output) if output.status.success() => BrowserAutomationProbeStatus::Granted,
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
                        if stderr.contains("-1743")
                            || stderr.contains("not authorized")
                            || stderr.contains("not authorised")
                            || stderr.contains("not allowed")
                        {
                            BrowserAutomationProbeStatus::Denied
                        } else {
                            BrowserAutomationProbeStatus::Failed
                        }
                    }
                    Err(_) => BrowserAutomationProbeStatus::Failed,
                };
                results.push(BrowserAutomationProbeResult {
                    browser: browser.app_name.to_string(),
                    status,
                });
            }
        }

        Ok(capture_permission_summary_for_platform_with_browser_check(
            "macos",
            macos_accessibility_trusted(false),
            Some(browser_automation_check_from_probe_results(&results)),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(capture_permission_summary())
    }
}

#[cfg(target_os = "macos")]
fn browser_is_running(app_name: &str) -> bool {
    let script = format!(
        "application \"{}\" is running",
        app_name.replace('"', "\\\"")
    );
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()
        .and_then(|output| {
            output.status.success().then(|| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .eq_ignore_ascii_case("true")
            })
        })
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn browser_automation_probe_script(target: BrowserAutomationTarget) -> String {
    let app_name = target.app_name.replace('"', "\\\"");
    match target.script_kind {
        BrowserAutomationScriptKind::Safari => format!(
            "tell application \"{app_name}\" to if (count of windows) > 0 then get URL of current tab of front window"
        ),
        BrowserAutomationScriptKind::Chromium => format!(
            "tell application \"{app_name}\" to if (count of windows) > 0 then get URL of active tab of front window"
        ),
    }
}

/// Called at app startup: if AX is not yet trusted, open System Settings
/// so the user can grant it for the current binary. Safe to call on every launch.
pub fn ensure_accessibility_permission() {
    #[cfg(target_os = "macos")]
    {
        if !macos_accessibility_trusted(false) {
            let _ = macos_accessibility_trusted(true);
        }
    }
}

pub fn reset_and_request_accessibility() -> Result<CapturePermissionSummary> {
    #[cfg(target_os = "macos")]
    {
        // Reset the stale TCC grant (e.g. after a binary update invalidated the signature).
        // This clears the entry so macOS will prompt fresh on the next trust check.
        let _ = std::process::Command::new("tccutil")
            .args(["reset", "Accessibility", "ai.daytrail.desktop"])
            .status();

        // Trigger the system prompt / open System Settings Accessibility page.
        let _ = macos_accessibility_trusted(true);

        // Open the settings pane so the user can toggle the switch on.
        let _ = std::process::Command::new("open")
            .arg(ACCESSIBILITY_URL)
            .status();

        Ok(capture_permission_summary())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(capture_permission_summary())
    }
}

#[cfg(target_os = "macos")]
fn macos_accessibility_trusted(prompt: bool) -> bool {
    use core_foundation::{
        base::TCFType,
        boolean::CFBoolean,
        dictionary::{CFDictionary, CFDictionaryRef},
        string::{CFString, CFStringRef},
    };
    use core_foundation_sys::base::Boolean;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFStringRef;
        fn AXIsProcessTrusted() -> Boolean;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
    }

    unsafe {
        if !prompt {
            return AXIsProcessTrusted() != 0;
        }

        let prompt_key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let prompt_value = CFBoolean::true_value();
        let options = CFDictionary::from_CFType_pairs(&[(prompt_key, prompt_value)]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::{
        app_bundle_path_from_executable, browser_automation_check_from_probe_results,
        capture_permission_summary_for_platform, BrowserAutomationProbeResult,
        BrowserAutomationProbeStatus,
    };

    #[test]
    fn macos_accessibility_missing_requires_setup() {
        let summary = capture_permission_summary_for_platform("macos", false);

        assert_eq!(summary.platform, "macos");
        assert!(summary.setup_required);
        assert!(!summary.all_required_granted);
        assert!(summary.restart_recommended);
        assert!(!summary.diagnostics.is_empty());
        assert_eq!(summary.checks[0].id, "accessibility");
        assert!(summary.checks[0].required);
        assert_eq!(summary.checks[0].status, "missing");
        assert_eq!(
            summary.checks[0].settings_label.as_deref(),
            Some("Privacy & Security > Accessibility")
        );
    }

    #[test]
    fn macos_accessibility_granted_allows_capture_start() {
        let summary = capture_permission_summary_for_platform("macos", true);

        assert!(!summary.setup_required);
        assert!(summary.all_required_granted);
        assert!(!summary.restart_recommended);
        assert_eq!(summary.checks[0].status, "granted");
        assert_eq!(summary.checks[1].status, "user_prompt");
        assert_eq!(summary.checks[2].status, "not_required");
    }

    #[test]
    fn browser_automation_probe_reports_no_running_browser() {
        let check = browser_automation_check_from_probe_results(&[]);

        assert_eq!(check.id, "browser-automation");
        assert_eq!(check.status, "not_running");
        assert_eq!(check.action_label.as_deref(), Some("Grant now"));
    }

    #[test]
    fn browser_automation_probe_reports_granted_browsers() {
        let check = browser_automation_check_from_probe_results(&[BrowserAutomationProbeResult {
            browser: "Brave Browser".to_string(),
            status: BrowserAutomationProbeStatus::Granted,
        }]);

        assert_eq!(check.status, "granted");
        assert!(check.detail.contains("Brave Browser"));
        assert_eq!(check.action_label.as_deref(), Some("Recheck"));
    }

    #[test]
    fn browser_automation_probe_reports_partial_access() {
        let check = browser_automation_check_from_probe_results(&[
            BrowserAutomationProbeResult {
                browser: "Brave Browser".to_string(),
                status: BrowserAutomationProbeStatus::Granted,
            },
            BrowserAutomationProbeResult {
                browser: "Google Chrome".to_string(),
                status: BrowserAutomationProbeStatus::Denied,
            },
        ]);

        assert_eq!(check.status, "limited");
        assert!(check.detail.contains("Brave Browser"));
        assert!(check.detail.contains("Google Chrome"));
    }

    #[test]
    fn browser_automation_probe_reports_missing_access() {
        let check = browser_automation_check_from_probe_results(&[BrowserAutomationProbeResult {
            browser: "Google Chrome".to_string(),
            status: BrowserAutomationProbeStatus::Denied,
        }]);

        assert_eq!(check.status, "missing");
        assert_eq!(check.action_label.as_deref(), Some("Grant now"));
    }

    #[test]
    fn non_macos_platform_has_no_required_setup() {
        let summary = capture_permission_summary_for_platform("linux", false);

        assert_eq!(summary.platform, "linux");
        assert!(!summary.setup_required);
        assert!(summary.all_required_granted);
        assert_eq!(summary.checks[0].status, "granted");
    }

    #[test]
    fn windows_summary_explains_no_os_permission_and_elevated_app_limit() {
        let summary = capture_permission_summary_for_platform("windows", false);

        assert_eq!(summary.platform, "windows");
        assert!(!summary.setup_required);
        assert!(summary.all_required_granted);
        assert_eq!(summary.checks[0].status, "granted");
        assert_eq!(summary.checks[1].id, "elevated-apps");
        assert_eq!(summary.checks[1].status, "limited");
        assert!(summary
            .diagnostics
            .iter()
            .any(|item| item.contains("No Windows privacy permission")));
    }

    #[test]
    fn derives_app_bundle_path_from_macos_executable_path() {
        assert_eq!(
            app_bundle_path_from_executable("/Applications/DayTrail.app/Contents/MacOS/daytrail")
                .as_deref(),
            Some("/Applications/DayTrail.app")
        );
        assert_eq!(
            app_bundle_path_from_executable("/tmp/DayTrail.app/Contents/MacOS/daytrail").as_deref(),
            Some("/tmp/DayTrail.app")
        );
        assert_eq!(app_bundle_path_from_executable("/usr/bin/daytrail"), None);
    }
}
