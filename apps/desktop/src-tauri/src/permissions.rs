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
        "macos" => macos_permission_checks(accessibility_granted),
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
                "Browser URLs still require the DayTrail browser extension/native bridge."
                    .to_string(),
                "Editor folders and terminal commands require their bridge integrations."
                    .to_string(),
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

fn macos_permission_checks(accessibility_granted: bool) -> Vec<CapturePermissionCheck> {
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
        CapturePermissionCheck {
            id: "browser-automation".to_string(),
            label: "Browser automation".to_string(),
            required: false,
            status: "user_prompt".to_string(),
            detail: "macOS asks once when DayTrail reads a supported browser's active tab URL."
                .to_string(),
            settings_label: Some("Privacy & Security > Automation".to_string()),
            settings_url: Some(AUTOMATION_URL.to_string()),
            action_label: Some("Open Automation Settings".to_string()),
        },
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
    use super::{app_bundle_path_from_executable, capture_permission_summary_for_platform};

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
