use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    thread,
    time::{Duration, SystemTime},
};

type ContentTitleCache = HashMap<u32, (Option<String>, SystemTime)>;

/// Cache: pid → (enriched_title_or_None, captured_at)
/// Populated by `ax_content_title_cached`. Prevents running a slow AppleScript
/// on every 2-second poll tick — result is reused for 10 seconds.
static CONTENT_TITLE_CACHE: Mutex<Option<ContentTitleCache>> = Mutex::new(None);

#[cfg(target_os = "macos")]
use std::ffi::CStr;
#[cfg(target_os = "windows")]
use std::{ffi::OsString, os::windows::ffi::OsStringExt};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::{
    models::{GitContext, TerminalBridgeMetadata},
    project_detection::{default_project_sources, detect_project_candidates_from_sources},
    store::WorktraceStore,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWindowInfo {
    pub app_name: String,
    pub window_title: Option<String>,
    pub process_id: Option<u32>,
    pub url: Option<String>,
    pub workspace_key: Option<String>,
    pub workspace_candidates: Vec<String>,
    pub ai_tools: Vec<String>,
    pub git_context: Option<GitContext>,
    pub captured_at: String,
}

/// Threshold: skip recording if system has been idle longer than this.
const IDLE_SKIP_THRESHOLD_MS: u64 = 60_000; // 60 seconds

pub fn spawn_active_window_watcher(store: WorktraceStore, app: AppHandle, interval: Duration) {
    thread::spawn(move || {
        // Keep the capture loop out of macOS App Nap. A tray-resident app with
        // its window hidden is a prime App Nap target: macOS throttles/suspends
        // its background timers and threads, which silently stops capture until
        // the user re-foregrounds the app — the "nothing recorded on a new day"
        // report. The assertion still allows normal system (idle/lid) sleep, so
        // it does not drain the battery.
        let _activity = begin_background_activity();
        run_watcher_loop(&store, &app, interval);
    });
}

/// Watcher main loop. Each tick is wrapped in `catch_unwind` so a single bad
/// poll (e.g. a transient AX/objc failure) can never kill capture for the rest
/// of the process lifetime — it logs and continues on the next interval.
fn run_watcher_loop(store: &WorktraceStore, app: &AppHandle, interval: Duration) {
    let interval_ms = duration_to_ms(interval);
    let mut recording = true;
    watcher_log("watcher started");

    loop {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            watcher_tick(store, app, interval, interval_ms, &mut recording)
        }));
        if outcome.is_err() {
            watcher_log("PANIC in watcher tick — recovered, continuing");
        }
        thread::sleep(interval);
    }
}

/// A single poll cycle. Records the active window unless the user is idle.
/// Detects process suspension (sleep / App Nap) via the wall-clock gap since the
/// previous tick — the monotonic clock can pause during system sleep, so we
/// compare real timestamps instead. On a long gap we clear the stale per-PID AX
/// cache and post a "welcome back" notification so the user can classify the
/// away time even if the window was closed.
fn watcher_tick(
    store: &WorktraceStore,
    app: &AppHandle,
    interval: Duration,
    interval_ms: u64,
    recording: &mut bool,
) {
    let _ = store.ingest_local_bridge_files();

    let now_epoch = epoch_ms();
    // Wall-clock gap since the previous tick (0 on the very first tick).
    let wall_gap_ms = watcher_heartbeat()
        .map(|hb| now_epoch.saturating_sub(hb.last_tick_at_ms).max(0))
        .unwrap_or(0);

    let idle_ms = system_idle_ms().unwrap_or(0);
    let trusted = accessibility_trusted();
    // Heartbeat: published every tick so the UI can tell "alive" from "dead".
    update_heartbeat(|hb| {
        hb.last_tick_at_ms = now_epoch;
        hb.last_idle_ms = idle_ms;
        hb.accessibility_trusted = trusted;
    });

    if is_resume(wall_gap_ms as u64, interval_ms) {
        watcher_log(&format!(
            "resume after {wall_gap_ms}ms wall-clock gap (sleep/App Nap) — clearing AX cache"
        ));
        clear_content_title_cache();
        maybe_notify_away(app, wall_gap_ms);
    }

    if !should_record(idle_ms, IDLE_SKIP_THRESHOLD_MS) {
        if *recording {
            watcher_log(&format!("capture paused — user idle {idle_ms}ms"));
            *recording = false;
        }
        return;
    }
    if !*recording {
        watcher_log("capture resumed — user active");
        *recording = true;
    }

    let Some(info) = active_window_fallback() else {
        return;
    };
    if is_self_app(&info.app_name) {
        return;
    }
    let metadata = serde_json::to_string(&info).ok();
    match store.record_active_window_context(
        &info.app_name,
        info.window_title.as_deref(),
        info.url.as_deref(),
        info.workspace_key.as_deref(),
        metadata.as_deref(),
        Some(interval),
    ) {
        Ok(()) => update_heartbeat(|hb| {
            hb.last_capture_at_ms = Some(epoch_ms());
            hb.consecutive_record_errors = 0;
        }),
        Err(error) => {
            watcher_log(&format!("record_active_window_context failed: {error}"));
            update_heartbeat(|hb| {
                hb.consecutive_record_errors = hb.consecutive_record_errors.saturating_add(1);
            });
        }
    }

    // Focus Mode: nudge if the user has drifted onto a distraction. No-op unless
    // a focus session is active.
    crate::focus::evaluate(app, &info);
}

fn duration_to_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

/// Whether the active window should be captured this tick.
fn should_record(idle_ms: u64, idle_threshold_ms: u64) -> bool {
    idle_ms < idle_threshold_ms
}

/// Whether the gap since the previous tick indicates the process was suspended
/// (system sleep or App Nap throttling) rather than a normal interval wait.
fn is_resume(gap_ms: u64, interval_ms: u64) -> bool {
    let threshold = interval_ms.saturating_mul(5).max(60_000);
    gap_ms >= threshold
}

fn clear_content_title_cache() {
    if let Ok(mut guard) = CONTENT_TITLE_CACHE.lock() {
        if let Some(cache) = guard.as_mut() {
            cache.clear();
        }
    }
}

/// Minimum away gap before posting a "welcome back" notification.
const AWAY_NOTIFY_THRESHOLD_MS: i64 = 10 * 60 * 1000;

/// After the process resumes from a long suspension (laptop sleep), post a
/// native notification so the user can classify the away time — even if
/// DayTrail's window was closed. Best-effort and non-fatal.
fn maybe_notify_away(app: &AppHandle, gap_ms: i64) {
    if gap_ms < AWAY_NOTIFY_THRESHOLD_MS {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    let body = format!(
        "You were away about {}. Open DayTrail to log it as a meeting, break, or offline work.",
        humanize_duration_ms(gap_ms)
    );
    match app
        .notification()
        .builder()
        .title("Welcome back to DayTrail")
        .body(&body)
        .show()
    {
        Ok(()) => watcher_log(&format!("away notification posted ({gap_ms}ms gap)")),
        Err(error) => watcher_log(&format!("away notification failed: {error}")),
    }
}

/// Humanize a millisecond duration as e.g. "27m" or "1h 34m". Pure for testing.
fn humanize_duration_ms(ms: i64) -> String {
    let total_minutes = (ms / 60_000).max(1);
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

// ── Capture liveness / heartbeat ──────────────────────────────────────────────
//
// The watcher publishes a heartbeat every tick so the rest of the app can tell
// the difference between "capture is quiet because you're idle" and "capture is
// broken". This is what makes a silent stop (App Nap suspension, a hung poll, or
// a revoked Accessibility permission) visible instead of looking like a normal
// quiet morning.

/// Diagnostics published by the capture watcher. All timestamps are UNIX epoch ms.
#[derive(Debug, Clone)]
pub struct WatcherHeartbeat {
    /// Most recent watcher tick — proves the poll thread is still alive.
    pub last_tick_at_ms: i64,
    /// Most recent successful active-window capture, if any this run.
    pub last_capture_at_ms: Option<i64>,
    /// HID idle (ms) measured at the last tick.
    pub last_idle_ms: u64,
    /// macOS Accessibility trust at the last tick; `None` on other platforms.
    pub accessibility_trusted: Option<bool>,
    /// Consecutive `record_active_window_context` failures.
    pub consecutive_record_errors: u32,
}

static WATCHER_HEARTBEAT: Mutex<Option<WatcherHeartbeat>> = Mutex::new(None);

/// Snapshot of the latest watcher heartbeat, or `None` before the first tick.
pub fn watcher_heartbeat() -> Option<WatcherHeartbeat> {
    WATCHER_HEARTBEAT.lock().ok().and_then(|guard| guard.clone())
}

fn update_heartbeat(apply: impl FnOnce(&mut WatcherHeartbeat)) {
    if let Ok(mut guard) = WATCHER_HEARTBEAT.lock() {
        let heartbeat = guard.get_or_insert_with(|| WatcherHeartbeat {
            last_tick_at_ms: epoch_ms(),
            last_capture_at_ms: None,
            last_idle_ms: 0,
            accessibility_trusted: None,
            consecutive_record_errors: 0,
        });
        apply(heartbeat);
    }
}

fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn accessibility_trusted() -> Option<bool> {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    Some(unsafe { AXIsProcessTrusted() != 0 })
}

#[cfg(not(target_os = "macos"))]
fn accessibility_trusted() -> Option<bool> {
    None
}

/// Overall liveness of the capture watcher, derived from its heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureLiveness {
    /// No heartbeat yet (just launched) — not enough info to judge.
    Unknown,
    /// Ticking recently and permissions intact.
    Healthy,
    /// The poll thread has not ticked within the staleness window — capture is
    /// effectively stopped (suspended, hung, or dead).
    Stalled,
    /// Ticking, but the OS Accessibility permission is no longer granted — capture
    /// is degraded (app/window titles cannot be read reliably).
    PermissionLost,
}

/// Classify capture liveness from a heartbeat snapshot. Pure for testability.
///
/// `stale_after_ms` is how long without a tick counts as stalled. `Stalled`
/// takes precedence over `PermissionLost`: if the thread is not ticking, the
/// permission reading is itself stale and not trustworthy.
pub fn assess_capture_liveness(
    heartbeat: Option<&WatcherHeartbeat>,
    now_ms: i64,
    stale_after_ms: i64,
) -> CaptureLiveness {
    let Some(heartbeat) = heartbeat else {
        return CaptureLiveness::Unknown;
    };
    if now_ms.saturating_sub(heartbeat.last_tick_at_ms) > stale_after_ms {
        return CaptureLiveness::Stalled;
    }
    if heartbeat.accessibility_trusted == Some(false) {
        return CaptureLiveness::PermissionLost;
    }
    CaptureLiveness::Healthy
}

/// Append-only watcher diagnostics so silent capture failures are debuggable in
/// shipped builds. Best-effort; truncates if it grows past ~1 MB.
fn watcher_log(message: &str) {
    let Some(dir) = dirs::data_local_dir() else {
        return;
    };
    let path = dir.join("ai.daytrail.desktop").join("watcher.log");
    if let Ok(metadata) = fs::metadata(&path) {
        if metadata.len() > 1_000_000 {
            let _ = fs::remove_file(&path);
        }
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(file, "{} {message}", now_utc());
    }
}

/// Hold an `NSProcessInfo` activity assertion that disables App Nap while still
/// allowing the system to sleep when idle. The returned token must be kept alive
/// for as long as capture should run; dropping it ends the activity.
#[cfg(target_os = "macos")]
fn begin_background_activity() -> Option<impl Sized> {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

    let reason = NSString::from_str("DayTrail continuous activity capture");
    let token = NSProcessInfo::processInfo()
        .beginActivityWithOptions_reason(NSActivityOptions::UserInitiatedAllowingIdleSystemSleep, &reason);
    watcher_log("App Nap disabled via NSProcessInfo activity assertion");
    Some(token)
}

/// Opt the capture process out of Windows power throttling (EcoQoS).
///
/// A classic Win32 process (which Tauri produces) is never PLM-suspended the way
/// a macOS App Nap target is, so capture keeps running when the window is hidden.
/// This call only stops the OS from down-clocking the capture process while it is
/// backgrounded, keeping poll timing reliable. Like the macOS assertion, it does
/// **not** prevent the system from sleeping, so it is battery-safe.
#[cfg(target_os = "windows")]
fn begin_background_activity() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation,
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_STATE,
    };

    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0, // 0 = throttling disabled for the controlled knob
    };
    let ok = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            std::ptr::addr_of!(state) as *const core::ffi::c_void,
            core::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if ok != 0 {
        watcher_log("Windows power throttling disabled for capture process");
    } else {
        watcher_log("Windows power throttling opt-out failed (non-fatal)");
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn begin_background_activity() {}

/// Returns how long the system HID devices (keyboard/mouse) have been idle, in milliseconds.
/// Returns `None` if the check is unavailable or fails.
#[cfg(target_os = "macos")]
fn system_idle_ms() -> Option<u64> {
    let output = Command::new("ioreg")
        .args(["-c", "IOHIDSystem", "-n", "IOHIDSystem"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.contains("HIDIdleTime") {
            // format: "HIDIdleTime" = 3456789012
            let ns: u64 = line.split('=').nth(1)?.trim().parse().ok()?;
            return Some(ns / 1_000_000);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn system_idle_ms() -> Option<u64> {
    use windows_sys::Win32::{
        System::SystemInformation::GetTickCount64,
        UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
    };

    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info) == 0 {
            return None;
        }

        // LASTINPUTINFO stores a 32-bit tick count, so use wrapping arithmetic
        // to stay correct across the normal Windows tick counter rollover.
        let now = GetTickCount64() as u32;
        Some(now.wrapping_sub(info.dwTime) as u64)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_idle_ms() -> Option<u64> {
    None
}

pub fn active_window_fallback() -> Option<ActiveWindowInfo> {
    platform_active_window()
}

#[cfg(target_os = "macos")]
fn platform_active_window() -> Option<ActiveWindowInfo> {
    native_frontmost_application().or_else(applescript_frontmost_application)
}

#[cfg(target_os = "macos")]
fn native_frontmost_application() -> Option<ActiveWindowInfo> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    fn ns_string_to_string(value: &NSString) -> Option<String> {
        let ptr = value.UTF8String();
        if ptr.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .ok()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let raw_name = app
        .localizedName()
        .as_deref()
        .and_then(ns_string_to_string)
        .or_else(|| {
            app.bundleIdentifier()
                .as_deref()
                .and_then(ns_string_to_string)
        })?;
    let app_name = normalize_app_display_name(&raw_name).to_owned();
    let process_id = u32::try_from(app.processIdentifier()).ok();
    let title = process_id
        .and_then(accessibility_focused_window_title)
        .or_else(|| window_title_for_app(&app_name));
    let url = browser_url_for_app(&app_name);
    let mut workspace_candidates = process_id
        .map(workspace_candidates_from_process)
        .unwrap_or_default();
    if workspace_candidates.is_empty() {
        workspace_candidates = editor_workspace_candidates_from_storage(&app_name);
    }
    let app_context = app_context_hint_for_process(&app_name, process_id);
    if let Some(cwd) = app_context.as_ref().and_then(|hint| hint.cwd.as_ref()) {
        push_workspace_candidate(&mut workspace_candidates, cwd);
    }
    // Enrich title: strip "App — Context" patterns or AX heading search when
    // the raw title is just the app name (Claude, Codex, Notion, etc.)
    let title = app_context
        .as_ref()
        .and_then(|hint| hint.title.clone())
        .or_else(|| enrich_window_title(title.as_deref(), &app_name, process_id))
        .or(title);

    let bridge_hint = terminal_bridge_hint_for_app(&app_name);
    let workspace_key = workspace_from_title(title.as_deref())
        .or_else(|| workspace_from_candidates(title.as_deref(), &workspace_candidates))
        .or_else(|| single_workspace_candidate(&workspace_candidates))
        .or_else(|| workspace_from_editor_storage(&app_name, title.as_deref()))
        .or_else(|| app_context.and_then(|hint| hint.cwd))
        .or_else(|| bridge_hint.as_ref().map(|hint| hint.cwd.clone()));
    let mut ai_tools = ai_tools_from_processes(&app_name, process_id);
    if let Some(hint) = bridge_hint {
        for tool in hint.ai_tools {
            push_tool(&mut ai_tools, &tool);
        }
    }
    let git_context = workspace_key.as_deref().and_then(detect_git_context);

    Some(ActiveWindowInfo {
        app_name,
        window_title: title,
        process_id,
        url,
        workspace_key,
        workspace_candidates,
        ai_tools,
        git_context,
        captured_at: now_utc(),
    })
}

#[cfg(target_os = "macos")]
fn applescript_frontmost_application() -> Option<ActiveWindowInfo> {
    let output = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to set frontApp to name of first application process whose frontmost is true"#,
            "-e",
            r#"tell application "System Events" to set frontTitle to "" "#,
            "-e",
            r#"tell application "System Events" to set frontPid to unix id of first application process whose frontmost is true"#,
            "-e",
            r#"tell application "System Events" to tell process frontApp to if exists window 1 then set frontTitle to name of window 1"#,
            "-e",
            r#"return frontApp & linefeed & frontTitle & linefeed & frontPid"#,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let raw_name = lines.next()?.trim().to_string();
    if raw_name.is_empty() {
        return None;
    }
    let app_name = normalize_app_display_name(&raw_name).to_owned();
    let title = lines
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let process_id = lines
        .next()
        .map(str::trim)
        .and_then(|value| value.parse::<u32>().ok());
    let url = browser_url_for_app(&app_name);
    let title = process_id
        .and_then(accessibility_focused_window_title)
        .or(title.map(ToOwned::to_owned));
    let mut workspace_candidates = process_id
        .map(workspace_candidates_from_process)
        .unwrap_or_default();
    if workspace_candidates.is_empty() {
        workspace_candidates = editor_workspace_candidates_from_storage(&app_name);
    }
    let app_context = app_context_hint_for_process(&app_name, process_id);
    if let Some(cwd) = app_context.as_ref().and_then(|hint| hint.cwd.as_ref()) {
        push_workspace_candidate(&mut workspace_candidates, cwd);
    }
    // Enrich title for apps whose window title is just their name
    let title = app_context
        .as_ref()
        .and_then(|hint| hint.title.clone())
        .or_else(|| enrich_window_title(title.as_deref(), &app_name, process_id))
        .or(title);

    let bridge_hint = terminal_bridge_hint_for_app(&app_name);
    let workspace_key = workspace_from_title(title.as_deref())
        .or_else(|| workspace_from_candidates(title.as_deref(), &workspace_candidates))
        .or_else(|| single_workspace_candidate(&workspace_candidates))
        .or_else(|| workspace_from_editor_storage(&app_name, title.as_deref()))
        .or_else(|| app_context.and_then(|hint| hint.cwd))
        .or_else(|| bridge_hint.as_ref().map(|hint| hint.cwd.clone()));
    let mut ai_tools = ai_tools_from_processes(&app_name, process_id);
    if let Some(hint) = bridge_hint {
        for tool in hint.ai_tools {
            push_tool(&mut ai_tools, &tool);
        }
    }
    let git_context = workspace_key.as_deref().and_then(detect_git_context);

    Some(ActiveWindowInfo {
        app_name,
        window_title: title,
        process_id,
        url,
        workspace_key,
        workspace_candidates,
        ai_tools,
        git_context,
        captured_at: now_utc(),
    })
}

#[cfg(target_os = "macos")]
fn accessibility_focused_window_title(pid: u32) -> Option<String> {
    use core_foundation::{
        base::{CFRelease, CFTypeRef, TCFType},
        string::{CFString, CFStringRef},
    };
    use std::{ffi::c_void, ptr};

    type AXError = i32;
    type AXUIElementRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
    }

    unsafe fn copy_attribute(element: AXUIElementRef, attribute: &str) -> Option<CFTypeRef> {
        let attribute = CFString::new(attribute);
        let mut value: CFTypeRef = ptr::null();
        let error =
            AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value);
        (error == 0 && !value.is_null()).then_some(value)
    }

    unsafe {
        let app = AXUIElementCreateApplication(pid as i32);
        if app.is_null() {
            return None;
        }

        let window =
            copy_attribute(app, "AXFocusedWindow").or_else(|| copy_attribute(app, "AXMainWindow"));
        CFRelease(app as CFTypeRef);

        let window = window?;
        let title = copy_attribute(window as AXUIElementRef, "AXTitle");
        CFRelease(window);

        let title = title?;
        let title = CFString::wrap_under_create_rule(title as CFStringRef)
            .to_string()
            .trim()
            .to_string();
        (!title.is_empty()).then_some(title)
    }
}

#[cfg(target_os = "macos")]
fn window_title_for_app(app_name: &str) -> Option<String> {
    let script = format!(
        r#"tell application "System Events" to tell process "{}" to if exists window 1 then return name of window 1"#,
        app_name.replace('"', "")
    );
    run_osascript(&[&script])
}

#[cfg(target_os = "macos")]
fn workspace_candidates_from_process(pid: u32) -> Vec<String> {
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string(), "-Fn"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let home = dirs::home_dir();
    let mut seen = HashSet::new();
    let mut candidates = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .filter_map(|value| workspace_candidate_from_path(Path::new(value), home.as_deref()))
        .filter(|value| seen.insert(value.clone()))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        project_name(right)
            .len()
            .cmp(&project_name(left).len())
            .then_with(|| left.cmp(right))
    });
    candidates
}

fn push_workspace_candidate(candidates: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() || candidates.iter().any(|candidate| candidate == value) {
        return;
    }
    candidates.insert(0, value.to_string());
}

fn workspace_from_title(title: Option<&str>) -> Option<String> {
    let title = title?;
    title
        .split_whitespace()
        .find(|part| part.starts_with('/') || part.starts_with("~/"))
        .and_then(|part| {
            let clean = part.trim_matches(|ch: char| {
                matches!(ch, ',' | ';' | ':' | '"' | '\'' | ')' | '(' | '[' | ']')
            });
            if let Some(rest) = clean.strip_prefix("~/") {
                dirs::home_dir().map(|home| home.join(rest).display().to_string())
            } else {
                Some(clean.to_string())
            }
        })
}

fn workspace_from_candidates(title: Option<&str>, candidates: &[String]) -> Option<String> {
    let title = title?.to_ascii_lowercase();
    candidates
        .iter()
        .filter(|candidate| {
            let name = project_name(candidate).to_ascii_lowercase();
            !name.is_empty() && title.contains(&name)
        })
        .max_by_key(|candidate| project_name(candidate).len())
        .cloned()
}

fn single_workspace_candidate(candidates: &[String]) -> Option<String> {
    (candidates.len() == 1).then(|| candidates[0].clone())
}

fn project_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_string()
}

fn workspace_from_editor_storage(app_name: &str, title: Option<&str>) -> Option<String> {
    let candidates = editor_workspace_candidates_from_storage(app_name);
    workspace_from_candidates(title, &candidates)
        .or_else(|| single_workspace_candidate(&candidates))
        .or_else(|| candidates.into_iter().next())
}

fn editor_workspace_candidates_from_storage(app_name: &str) -> Vec<String> {
    if !is_editor_app(app_name) {
        return Vec::new();
    }

    let mut sources = default_project_sources();
    sources.terminal_bridge_metadata_paths.clear();
    let app_lower = app_name.to_ascii_lowercase();
    sources.workspace_storage_roots.retain(|root| {
        let rendered = root.display().to_string().to_ascii_lowercase();
        if app_lower.contains("cursor") {
            rendered.contains("cursor")
        } else if app_lower.contains("code") || app_lower.contains("vscodium") {
            rendered.contains("code") && !rendered.contains("cursor")
        } else {
            true
        }
    });
    detect_project_candidates_from_sources(sources)
        .unwrap_or_default()
        .into_iter()
        .map(|context| context.path)
        .collect()
}

// ── Git context detection ─────────────────────────────────────────────────

/// Extract a ticket/issue ID from a branch name or window title.
/// Supports: Jira/Linear (PROJ-123), GitLab MR (!4209), GitHub/generic (#123).
fn extract_ticket_id(text: &str) -> Option<String> {
    for word in text.split(|c: char| c.is_whitespace() || matches!(c, '/' | '_' | ':' | ',')) {
        let word =
            word.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | '.'));
        if word.is_empty() {
            continue;
        }
        // GitLab MR: !NNNN
        if let Some(rest) = word.strip_prefix('!') {
            if (1..=6).contains(&rest.len()) && rest.chars().all(|c| c.is_ascii_digit()) {
                return Some(format!("!{rest}"));
            }
        }
        // GitHub / generic issue: #NNNN
        if let Some(rest) = word.strip_prefix('#') {
            if (1..=6).contains(&rest.len()) && rest.chars().all(|c| c.is_ascii_digit()) {
                return Some(format!("#{rest}"));
            }
        }
        // Jira / Linear style: PREFIX-NNNN  (2–10 uppercase letters, dash, 1–6 digits)
        if let Some(dash) = word.find('-') {
            let prefix = &word[..dash];
            let suffix = &word[dash + 1..];
            if (2..=10).contains(&prefix.len())
                && prefix.chars().all(|c| c.is_ascii_uppercase())
                && (1..=6).contains(&suffix.len())
                && suffix.chars().all(|c| c.is_ascii_digit())
            {
                return Some(word.to_string());
            }
        }
    }
    None
}

fn run_git(workspace_path: &str, args: &[&str]) -> Option<String> {
    let output = crate::platform::hidden_command("git")
        .arg("-C")
        .arg(workspace_path)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Run lightweight git commands to enrich an editor workspace event with
/// branch name, repo root, remote origin, and any ticket ID in the branch.
/// All calls use a 1-second timeout via the git config flag.
pub fn detect_git_context(workspace_path: &str) -> Option<GitContext> {
    // git is slow on cold start — skip if the dir isn't a git repo
    if !Path::new(workspace_path).join(".git").exists()
        && run_git(workspace_path, &["rev-parse", "--git-dir"]).is_none()
    {
        return None;
    }
    let branch = run_git(workspace_path, &["branch", "--show-current"]);
    let repo_root = run_git(workspace_path, &["rev-parse", "--show-toplevel"]);
    let remote_origin = run_git(workspace_path, &["remote", "get-url", "origin"]);
    let ticket_id = branch
        .as_deref()
        .and_then(extract_ticket_id)
        .or_else(|| remote_origin.as_deref().and_then(extract_ticket_id));

    // Only return Some if we got at least the branch
    branch.as_ref()?;
    Some(GitContext {
        branch,
        repo_root,
        remote_origin,
        ticket_id,
    })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn ai_tools_from_processes(app_name: &str, process_id: Option<u32>) -> Vec<String> {
    let mut tools = Vec::new();
    for tool in terminal_ai_tools_from_processes(app_name, process_id) {
        push_tool(&mut tools, &tool);
    }
    tools
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn editor_ai_tools_from_processes(app_name: &str) -> Vec<String> {
    // Do not count loaded editor extension/helper processes as usage. VS Code, Cursor,
    // and JetBrains keep AI extensions resident even when the user did not invoke them.
    // Usage must come from an editor bridge event, browser URL, AI app, or terminal command.
    let _ = app_name;
    Vec::new()
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn editor_ai_tools_from_processes_legacy_scan(app_name: &str) -> Vec<String> {
    if !is_editor_app(app_name) {
        return Vec::new();
    }

    let output = Command::new("ps").args(["-axo", "command"]).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let mut tools = Vec::new();
    if app_name == "Cursor" {
        push_tool(&mut tools, "Cursor");
    }

    for command in String::from_utf8_lossy(&output.stdout).lines() {
        let lower = command.to_ascii_lowercase();
        let relevant = lower.contains("/.vscode/extensions/")
            || lower.contains("/.cursor/extensions/")
            || lower.contains("/visual studio code.app/")
            || lower.contains("code helper")
            || lower.contains("codex app-server")
            || lower.contains("claude")
            || lower.contains("gemini");
        if !relevant {
            continue;
        }

        if lower.contains("codex app-server")
            || lower.contains("extensionid=openai.chatgpt")
            || lower.contains("/openai.chatgpt-")
        {
            push_tool(&mut tools, "Codex");
        }
        if lower.contains("github.copilot")
            || lower.contains("@vscode/copilot")
            || lower.contains("/extensions/copilot")
            || lower.contains("copilot")
        {
            push_tool(&mut tools, "Copilot");
        }
        if lower.contains("claude-code")
            || lower.contains("claude code")
            || lower.contains("anthropic.claude")
        {
            push_tool(&mut tools, "Claude Code");
        }
        if lower.contains("gemini") || lower.contains("@google/gemini-cli") {
            push_tool(&mut tools, "Gemini");
        }
        if lower.contains("saoudrizwan.claude-dev") || lower.contains("cline") {
            push_tool(&mut tools, "Cline");
        }
        if lower.contains("continue.continue") || lower.contains("/continue") {
            push_tool(&mut tools, "Continue");
        }
        if lower.contains("aider") {
            push_tool(&mut tools, "Aider");
        }
        if lower.contains("windsurf") {
            push_tool(&mut tools, "Windsurf");
        }
    }

    push_recent_editor_log_tools(&mut tools, app_name);
    tools
}

#[cfg(target_os = "macos")]
fn terminal_ai_tools_from_processes(app_name: &str, process_id: Option<u32>) -> Vec<String> {
    if !is_terminal_app(app_name) && !is_editor_app(app_name) {
        return Vec::new();
    }
    let Some(process_id) = process_id else {
        return Vec::new();
    };

    let output = Command::new("ps")
        .args(["-axo", "pid,ppid,command"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    terminal_ai_tools_from_ps_output(&String::from_utf8_lossy(&output.stdout), process_id)
}

fn terminal_ai_tools_from_ps_output(output: &str, root_pid: u32) -> Vec<String> {
    let rows = parse_process_rows(output);
    let mut children_by_parent: HashMap<u32, Vec<&ProcessRow>> = HashMap::new();
    for row in &rows {
        children_by_parent.entry(row.ppid).or_default().push(row);
    }

    let mut descendants = HashSet::from([root_pid]);
    let mut stack = vec![root_pid];
    while let Some(parent) = stack.pop() {
        if let Some(children) = children_by_parent.get(&parent) {
            for child in children {
                if descendants.insert(child.pid) {
                    stack.push(child.pid);
                }
            }
        }
    }

    let mut tools = Vec::new();
    for row in rows
        .iter()
        .filter(|row| row.pid != root_pid && descendants.contains(&row.pid))
    {
        push_cli_ai_tools_from_command(&mut tools, &row.command);
    }
    tools
}

#[derive(Debug)]
struct ProcessRow {
    pid: u32,
    ppid: u32,
    command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppContextHint {
    title: Option<String>,
    cwd: Option<String>,
}

fn parse_process_rows(output: &str) -> Vec<ProcessRow> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let command = parts.collect::<Vec<_>>().join(" ");
            (!command.is_empty()).then_some(ProcessRow { pid, ppid, command })
        })
        .collect()
}

fn descendant_process_ids(output: &str, root_pid: u32) -> HashSet<u32> {
    let rows = parse_process_rows(output);
    let mut children_by_parent: HashMap<u32, Vec<&ProcessRow>> = HashMap::new();
    for row in &rows {
        children_by_parent.entry(row.ppid).or_default().push(row);
    }

    let mut descendants = HashSet::from([root_pid]);
    let mut stack = vec![root_pid];
    while let Some(parent) = stack.pop() {
        if let Some(children) = children_by_parent.get(&parent) {
            for child in children {
                if descendants.insert(child.pid) {
                    stack.push(child.pid);
                }
            }
        }
    }
    descendants
}

fn app_context_hint_for_process(app_name: &str, process_id: Option<u32>) -> Option<AppContextHint> {
    if app_name == "Codex" {
        return process_id.and_then(codex_context_from_process);
    }
    None
}

#[cfg(target_os = "macos")]
fn codex_context_from_process(root_pid: u32) -> Option<AppContextHint> {
    let output = Command::new("ps")
        .args(["-axo", "pid,ppid,command"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let descendants = descendant_process_ids(&String::from_utf8_lossy(&output.stdout), root_pid);
    let mut sessions = Vec::new();
    for pid in descendants {
        let output = Command::new("lsof")
            .args(["-p", &pid.to_string(), "-Fn"])
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }
        for path in String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix('n'))
            .filter(|path| path.contains("/.codex/sessions/") && path.ends_with(".jsonl"))
        {
            let path = PathBuf::from(path);
            let modified = path
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            sessions.push((modified, path));
        }
    }

    sessions.sort_by_key(|session| std::cmp::Reverse(session.0));
    sessions
        .into_iter()
        .filter_map(|(_, path)| codex_thread_hint_from_rollout_path(&path))
        .next()
}

#[cfg(not(target_os = "macos"))]
fn codex_context_from_process(_root_pid: u32) -> Option<AppContextHint> {
    None
}

fn codex_thread_hint_from_rollout_path(rollout_path: &Path) -> Option<AppContextHint> {
    let db_path = dirs::home_dir()?.join(".codex/state_5.sqlite");
    codex_thread_hint_from_state_db(&db_path, rollout_path)
}

fn codex_thread_hint_from_state_db(db_path: &Path, rollout_path: &Path) -> Option<AppContextHint> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
            | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()?;
    let rollout_path = rollout_path.to_string_lossy();
    let mut stmt = conn
        .prepare(
            "SELECT title, cwd
             FROM threads
             WHERE rollout_path = ?1
             LIMIT 1",
        )
        .ok()?;
    stmt.query_row([rollout_path.as_ref()], |row| {
        let title: String = row.get(0)?;
        let cwd: String = row.get(1)?;
        Ok(AppContextHint {
            title: clean_codex_context_title(&title),
            cwd: (!cwd.trim().is_empty()).then(|| cwd.trim().to_string()),
        })
    })
    .ok()
}

fn clean_codex_context_title(value: &str) -> Option<String> {
    let first_line = value.lines().find(|line| !line.trim().is_empty())?;
    let mut title = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_TITLE_LEN: usize = 90;
    if title.len() > MAX_TITLE_LEN {
        title.truncate(MAX_TITLE_LEN);
        title = title.trim_end().to_string();
    }
    (!title.is_empty() && !title.eq_ignore_ascii_case("Codex")).then_some(title)
}

fn push_cli_ai_tools_from_command(tools: &mut Vec<String>, command: &str) {
    let lower = command.to_ascii_lowercase();
    if lower.contains("codex app-server") || lower.contains("/.vscode/extensions/") {
        return;
    }

    if command_invokes_tool(command, &["gemini"]) || lower.contains("@google/gemini-cli") {
        push_tool(tools, "Gemini");
    }
    if command_invokes_tool(command, &["codex"]) || lower.contains("@openai/codex") {
        push_tool(tools, "Codex");
    }
    if command_invokes_tool(command, &["claude", "claude-code"]) || lower.contains("claude-code") {
        push_tool(tools, "Claude Code");
    }
    if command_invokes_tool(command, &["aider"]) {
        push_tool(tools, "Aider");
    }
    if command_invokes_tool(command, &["cline"]) {
        push_tool(tools, "Cline");
    }
    if command_invokes_tool(command, &["continue"]) || lower.contains("continue.continue") {
        push_tool(tools, "Continue");
    }
    if command_invokes_tool(command, &["opencode"]) {
        push_tool(tools, "OpenCode");
    }
}

fn command_invokes_tool(command: &str, names: &[&str]) -> bool {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    let Some(first) = tokens.first() else {
        return false;
    };
    let first_name = executable_name(first);
    if names.iter().any(|name| first_name == *name) {
        return true;
    }

    let first_is_launcher = matches!(
        first_name.as_str(),
        "node" | "npm" | "npx" | "pnpm" | "bun" | "uvx"
    );
    if !first_is_launcher {
        return false;
    }

    tokens.iter().skip(1).any(|token| {
        let lower = token.to_ascii_lowercase();
        (lower.contains('/') || lower.contains('\\') || lower.starts_with('@'))
            && names.iter().any(|name| executable_name(token) == *name)
    })
}

fn executable_name(token: &str) -> String {
    let clean = token.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | '[' | ']'));
    Path::new(clean)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(clean)
        .trim_end_matches(".cmd")
        .trim_end_matches(".exe")
        .to_ascii_lowercase()
}

#[allow(dead_code)]
fn push_recent_editor_log_tools(tools: &mut Vec<String>, app_name: &str) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let roots = editor_log_roots(app_name, &home);
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(36 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    for root in roots {
        scan_recent_editor_log_tools(tools, &root, cutoff, 0);
    }
}

#[allow(dead_code)]
fn editor_log_roots(app_name: &str, home: &Path) -> Vec<PathBuf> {
    let is_cursor = app_name.eq_ignore_ascii_case("cursor");
    #[cfg(target_os = "macos")]
    {
        if is_cursor {
            vec![home.join("Library/Application Support/Cursor/logs")]
        } else {
            vec![home.join("Library/Application Support/Code/logs")]
        }
    }
    #[cfg(target_os = "windows")]
    {
        if is_cursor {
            vec![home.join("AppData/Roaming/Cursor/logs")]
        } else {
            vec![home.join("AppData/Roaming/Code/logs")]
        }
    }
    #[cfg(target_os = "linux")]
    {
        if is_cursor {
            vec![home.join(".config/Cursor/logs")]
        } else {
            vec![home.join(".config/Code/logs")]
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Vec::new()
    }
}

#[allow(dead_code)]
fn scan_recent_editor_log_tools(
    tools: &mut Vec<String>,
    path: &Path,
    cutoff: SystemTime,
    depth: u8,
) {
    if depth > 8 || !path.exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let modified = path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if modified >= cutoff {
            push_ai_tool_from_path(tools, &path);
        }
        if path.is_dir() {
            scan_recent_editor_log_tools(tools, &path, cutoff, depth + 1);
        }
    }
}

#[allow(dead_code)]
fn push_ai_tool_from_path(tools: &mut Vec<String>, path: &Path) {
    let lower = path.display().to_string().to_ascii_lowercase();
    if lower.contains("anthropic.claude-code") || lower.contains("claude-code") {
        push_tool(tools, "Claude Code");
    }
    if lower.contains("github.copilot") || lower.contains("copilot-chat") {
        push_tool(tools, "Copilot");
    }
    if lower.contains("openai.chatgpt") {
        push_tool(tools, "Codex");
    }
    if lower.contains("gemini") || lower.contains("google.gemini") {
        push_tool(tools, "Gemini");
    }
    if lower.contains("continue.continue") {
        push_tool(tools, "Continue");
    }
    if lower.contains("saoudrizwan.claude-dev") || lower.contains("cline") {
        push_tool(tools, "Cline");
    }
}

fn push_tool(tools: &mut Vec<String>, label: &str) {
    if !tools.iter().any(|tool| tool == label) {
        tools.push(label.to_string());
    }
}

#[cfg(target_os = "macos")]
fn workspace_candidate_from_path(path: &Path, home: Option<&Path>) -> Option<String> {
    if !path.is_absolute() || !path.exists() || path.is_dir() && is_ignored_path(path) {
        return None;
    }
    if let Some(home) = home {
        if !path.starts_with(home) {
            return None;
        }
    }

    let mut cursor = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    loop {
        if is_project_root(&cursor) {
            return Some(cursor.display().to_string());
        }
        if !cursor.pop() {
            break;
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn is_project_root(path: &Path) -> bool {
    [
        ".git",
        "package.json",
        "pnpm-workspace.yaml",
        "tsconfig.json",
        "Cargo.toml",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "gradlew",
        "go.mod",
        "pyproject.toml",
        "requirements.txt",
        "setup.py",
        "composer.json",
        ".idea",
        "nbproject",
    ]
    .iter()
    .any(|marker| path.join(marker).exists())
}

#[cfg(target_os = "macos")]
fn is_ignored_path(path: &Path) -> bool {
    let rendered = path.display().to_string();
    [
        "/Library/",
        "/Library/Caches/",
        "/Library/Application Support/",
        "/.cache/",
        "/node_modules/",
        "/target/",
        "/.git/",
    ]
    .iter()
    .any(|needle| rendered.contains(needle))
}

#[cfg(target_os = "macos")]
fn browser_url_for_app(app_name: &str) -> Option<String> {
    if app_name == "Safari" {
        return run_osascript(&[
            r#"tell application "Safari" to if (count of windows) > 0 then return URL of front document"#,
        ]);
    }

    if is_chromium_browser(app_name) {
        let script = format!(
            r#"tell application "{}" to if (count of windows) > 0 then return URL of active tab of front window"#,
            app_name.replace('"', "")
        );
        return run_osascript(&[&script]);
    }

    None
}

#[cfg(target_os = "macos")]
fn run_osascript(lines: &[&str]) -> Option<String> {
    let mut command = Command::new("osascript");
    for line in lines {
        command.arg("-e").arg(line);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(target_os = "macos")]
fn is_chromium_browser(app_name: &str) -> bool {
    matches!(
        app_name,
        "Google Chrome"
            | "Google Chrome Canary"
            | "Microsoft Edge"
            | "Brave Browser"
            | "ChatGPT Atlas"
            | "Arc"
            | "Chromium"
            | "Opera"
            | "Vivaldi"
    )
}

fn is_editor_app(app_name: &str) -> bool {
    matches!(
        app_name.to_ascii_lowercase().as_str(),
        "code"
            | "vs code"
            | "visual studio code"
            | "cursor"
            | "vscodium"
            | "webstorm"
            | "intellij idea"
            | "pycharm"
            | "phpstorm"
            | "goland"
            | "rubymine"
            | "netbeans"
            | "netbeans ide"
    )
}

fn is_terminal_app(app_name: &str) -> bool {
    matches!(
        app_name.to_ascii_lowercase().as_str(),
        "terminal"
            | "iterm"
            | "iterm2"
            | "warp"
            | "warp terminal"
            | "kitty"
            | "alacritty"
            | "wezterm"
            | "ghostty"
            | "windows terminal"
            | "powershell"
            | "command prompt"
    )
}

#[derive(Debug)]
struct TerminalBridgeHint {
    cwd: String,
    ai_tools: Vec<String>,
}

fn terminal_bridge_hint_for_app(app_name: &str) -> Option<TerminalBridgeHint> {
    if !is_terminal_app(app_name) && !is_editor_app(app_name) {
        return None;
    }

    let now = SystemTime::now();
    for path in default_project_sources().terminal_bridge_metadata_paths {
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if now.duration_since(modified).unwrap_or_default() > Duration::from_secs(5 * 60) {
            continue;
        }

        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(bridge) = serde_json::from_str::<TerminalBridgeMetadata>(&contents) else {
            continue;
        };
        let cwd = bridge.cwd.trim();
        if cwd.is_empty() {
            continue;
        }
        if !terminal_bridge_matches_app(app_name, &bridge) {
            continue;
        }

        let mut ai_tools = Vec::new();
        if let Some(command) = bridge.last_command.as_deref() {
            push_cli_ai_tools_from_command(&mut ai_tools, command);
        }
        return Some(TerminalBridgeHint {
            cwd: cwd.to_string(),
            ai_tools,
        });
    }

    None
}

fn terminal_bridge_matches_app(app_name: &str, bridge: &TerminalBridgeMetadata) -> bool {
    let app = app_name.to_ascii_lowercase();
    let terminal = bridge
        .terminal
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if !is_terminal_app(app_name) {
        return !terminal.is_empty()
            && (terminal == app
                || app.contains(&terminal)
                || terminal.contains(&app)
                || (app == "vs code" && terminal == "code")
                || (app == "visual studio code" && terminal == "code"));
    }

    terminal.is_empty()
        || terminal == app
        || app.contains(&terminal)
        || terminal.contains(&app)
        || (app == "vs code" && terminal == "code")
        || (app == "visual studio code" && terminal == "code")
}

#[cfg(target_os = "windows")]
fn platform_active_window() -> Option<ActiveWindowInfo> {
    use windows_sys::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    };

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }

        let mut pid = 0_u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }

        let title = windows_window_title(hwnd);
        let executable = windows_process_image_path(pid);
        let app_name = executable
            .as_deref()
            .and_then(display_app_name_from_executable)
            .unwrap_or_else(|| format!("Process {pid}"));
        let workspace_candidates = editor_workspace_candidates_from_storage(&app_name);
        let bridge_hint = terminal_bridge_hint_for_app(&app_name);
        let workspace_key = workspace_from_title(title.as_deref())
            .or_else(|| workspace_from_candidates(title.as_deref(), &workspace_candidates))
            .or_else(|| single_workspace_candidate(&workspace_candidates))
            .or_else(|| workspace_from_editor_storage(&app_name, title.as_deref()))
            .or_else(|| bridge_hint.as_ref().map(|hint| hint.cwd.clone()));
        let mut ai_tools = ai_tools_from_processes(&app_name, Some(pid));
        if let Some(hint) = bridge_hint {
            for tool in hint.ai_tools {
                push_tool(&mut ai_tools, &tool);
            }
        }
        let git_context = workspace_key.as_deref().and_then(detect_git_context);

        Some(ActiveWindowInfo {
            app_name,
            window_title: title,
            process_id: Some(pid),
            url: None,
            workspace_key,
            workspace_candidates,
            ai_tools,
            git_context,
            captured_at: now_utc(),
        })
    }
}

#[cfg(target_os = "windows")]
unsafe fn windows_window_title(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return None;
    }
    let mut buffer = vec![0_u16; (len as usize) + 1];
    let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    if copied <= 0 {
        return None;
    }
    utf16_to_string(&buffer[..copied as usize])
}

#[cfg(target_os = "windows")]
unsafe fn windows_process_image_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle.is_null() {
        return None;
    }

    let mut size = 32_768_u32;
    let mut buffer = vec![0_u16; size as usize];
    let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
    let _ = CloseHandle(handle);
    if ok == 0 || size == 0 {
        return None;
    }
    utf16_to_string(&buffer[..size as usize])
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn display_app_name_from_executable(path: &str) -> Option<String> {
    let stem = executable_stem_from_path(path)?;
    let lower = stem.to_ascii_lowercase();
    let label = match lower.as_str() {
        "code" => "VS Code",
        "cursor" => "Cursor",
        "vscodium" => "VSCodium",
        "chrome" => "Google Chrome",
        "msedge" => "Microsoft Edge",
        "brave" => "Brave Browser",
        "firefox" => "Firefox",
        "windowsterminal" | "wt" => "Windows Terminal",
        "powershell" | "pwsh" => "PowerShell",
        "cmd" => "Command Prompt",
        "idea64" | "idea" => "IntelliJ IDEA",
        "pycharm64" | "pycharm" => "PyCharm",
        "webstorm64" | "webstorm" => "WebStorm",
        "netbeans64" | "netbeans" => "NetBeans",
        _ => stem,
    };
    Some(label.to_string())
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn executable_stem_from_path(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\'])
        .next()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .strip_suffix(".exe")
                .or_else(|| value.strip_suffix(".EXE"))
                .unwrap_or(value)
        })
}

#[cfg(target_os = "windows")]
fn editor_ai_tools_from_processes(app_name: &str) -> Vec<String> {
    if !is_editor_app(app_name) {
        return Vec::new();
    }

    let mut tools = Vec::new();
    if app_name == "Cursor" {
        push_tool(&mut tools, "Cursor");
    }

    for executable in windows_process_executable_names() {
        let lower = executable.to_ascii_lowercase();
        if lower.contains("codex") || lower.contains("openai") {
            push_tool(&mut tools, "Codex");
        }
        if lower.contains("claude") {
            push_tool(&mut tools, "Claude Code");
        }
        if lower.contains("gemini") {
            push_tool(&mut tools, "Gemini");
        }
        if lower.contains("copilot") {
            push_tool(&mut tools, "Copilot");
        }
        if lower.contains("aider") {
            push_tool(&mut tools, "Aider");
        }
        if lower.contains("cline") {
            push_tool(&mut tools, "Cline");
        }
        if lower.contains("windsurf") {
            push_tool(&mut tools, "Windsurf");
        }
    }

    push_recent_editor_log_tools(&mut tools, app_name);
    tools
}

#[cfg(target_os = "windows")]
fn terminal_ai_tools_from_processes(app_name: &str, process_id: Option<u32>) -> Vec<String> {
    if !is_terminal_app(app_name) && !is_editor_app(app_name) {
        return Vec::new();
    }

    if let (Some(process_id), Some(output)) = (process_id, windows_process_command_output()) {
        let scoped_tools = terminal_ai_tools_from_ps_output(&output, process_id);
        if !scoped_tools.is_empty() {
            return scoped_tools;
        }
    }

    let mut tools = Vec::new();
    for executable in windows_process_executable_names() {
        push_cli_ai_tools_from_command(&mut tools, &executable);
    }
    tools
}

#[cfg(target_os = "windows")]
fn windows_process_command_output() -> Option<String> {
    let script = "Get-CimInstance Win32_Process | ForEach-Object { '{0} {1} {2}' -f $_.ProcessId,$_.ParentProcessId,(($_.CommandLine -as [string]) -replace '[\\r\\n]+',' ') }";
    let output = crate::platform::hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(target_os = "windows")]
fn windows_process_executable_names() -> Vec<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }

        let mut entry = std::mem::zeroed::<PROCESSENTRY32W>();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut names = Vec::new();
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if let Some(name) = utf16_to_string_until_nul(&entry.szExeFile) {
                    names.push(name);
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        names
    }
}

#[cfg(target_os = "windows")]
fn utf16_to_string_until_nul(values: &[u16]) -> Option<String> {
    let len = values
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(values.len());
    utf16_to_string(&values[..len])
}

#[cfg(target_os = "windows")]
fn utf16_to_string(values: &[u16]) -> Option<String> {
    let value = OsString::from_wide(values)
        .to_string_lossy()
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_active_window() -> Option<ActiveWindowInfo> {
    let _ = Command::new("true");
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn editor_ai_tools_from_processes(_app_name: &str) -> Vec<String> {
    Vec::new()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn terminal_ai_tools_from_processes(_app_name: &str, _process_id: Option<u32>) -> Vec<String> {
    Vec::new()
}

fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn is_self_app(app_name: &str) -> bool {
    matches!(
        app_name,
        "DayTrail"
            | "daytrail"
            | "ai.daytrail.desktop"
            | "WorkTrace AI"
            | "worktrace-ai"
            | "ai.worktrace.desktop"
    )
}

// ── Title enrichment ─────────────────────────────────────────────────────────

/// **Strategy 1 (0 ms)**: Extract context from window titles that encode it as
/// `"Context — AppName"` or `"AppName – Context"` etc.
///
/// Handles common separators used by browsers, Notion, Linear, Figma, Slack, …
fn enrich_title_from_pattern(title: &str, app_name: &str) -> Option<String> {
    // Ordered from most specific (longest) to shortest so `" — "` beats `"-"`
    let separators = [" — ", " – ", " - ", " | ", " · ", " > ", " / "];
    for sep in &separators {
        // "Context <sep> AppName" → "Context"
        if let Some(ctx) = title.strip_suffix(&format!("{sep}{app_name}")) {
            let ctx = ctx.trim();
            if ctx.len() > 2 && !ctx.eq_ignore_ascii_case(app_name) {
                return Some(ctx.to_string());
            }
        }
        // "AppName <sep> Context" → "Context"
        if let Some(ctx) = title.strip_prefix(&format!("{app_name}{sep}")) {
            let ctx = ctx.trim();
            if ctx.len() > 2 && !ctx.eq_ignore_ascii_case(app_name) {
                return Some(ctx.to_string());
            }
        }
    }
    None
}

fn clean_ax_candidate(value: &str) -> String {
    value
        .replace(
            ['\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}'],
            "",
        )
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|ch: char| matches!(ch, '-' | '|' | '·' | ':' | ' '))
        .to_string()
}

fn meaningful_ax_candidate(value: &str, app_name: &str) -> Option<String> {
    let cleaned = clean_ax_candidate(value);
    if cleaned.len() < 3 || cleaned.len() > 140 {
        return None;
    }

    let lower = cleaned.to_ascii_lowercase();
    let app_lower = app_name.to_ascii_lowercase();
    let generic = [
        "activity",
        "attach",
        "back",
        "calls",
        "cancel",
        "channels",
        "chats",
        "chatgpt",
        "claude",
        "close",
        "codex",
        "compose",
        "communities",
        "customize",
        "done",
        "edit",
        "emoji",
        "file",
        "forward",
        "gemini",
        "help",
        "home",
        "inbox",
        "minimize",
        "more",
        "new session",
        "notifications",
        "open",
        "pro",
        "reload",
        "recents",
        "save",
        "search",
        "send",
        "settings",
        "share",
        "skip to content",
        "threads",
        "today",
        "updates",
        "view",
        "window",
        "zoom",
    ];
    if lower == app_lower || generic.iter().any(|item| lower == *item) {
        return None;
    }

    Some(cleaned)
}

fn status_prefixed_ax_candidate(value: &str, app_name: &str) -> Option<(i32, String)> {
    let cleaned = clean_ax_candidate(value);
    let lower = cleaned.to_ascii_lowercase();
    let prefixes = [
        ("awaiting input ", 125),
        ("working on ", 120),
        ("working ", 118),
        ("running ", 116),
        ("active ", 108),
        ("selected ", 106),
        ("current ", 104),
        ("idle ", 42),
    ];

    for (prefix, score) in prefixes {
        if lower.starts_with(prefix) {
            let original = cleaned[prefix.len()..].trim();
            let candidate = meaningful_ax_candidate(original, app_name)?;
            return Some((score, candidate));
        }
    }

    None
}

fn keep_better_candidate(best: &mut Option<(i32, String)>, candidate: Option<(i32, String)>) {
    let Some(candidate) = candidate else {
        return;
    };
    if match best.as_ref() {
        Some(current) => candidate.0 > current.0,
        None => true,
    } {
        *best = Some(candidate);
    }
}

fn score_ax_context_candidate(
    role: &str,
    title: &str,
    value: &str,
    description: &str,
    app_name: &str,
    selected: Option<bool>,
    focused: Option<bool>,
) -> Option<(i32, String)> {
    let mut prefixed: Option<(i32, String)> = None;
    for raw in [title, value, description] {
        keep_better_candidate(&mut prefixed, status_prefixed_ax_candidate(raw, app_name));
    }
    if prefixed.is_some() {
        return prefixed;
    }

    let candidate = [title, value, description]
        .into_iter()
        .find_map(|raw| meaningful_ax_candidate(raw, app_name))?;

    if role == "AXHeading" {
        return Some((140, candidate));
    }

    let can_describe_current_item = matches!(
        role,
        "AXButton" | "AXCell" | "AXGroup" | "AXLink" | "AXRow" | "AXStaticText"
    );
    if !can_describe_current_item {
        return None;
    }

    if selected == Some(true) {
        return Some((112, candidate));
    }
    if focused == Some(true) {
        return Some((98, candidate));
    }

    None
}

/// **Strategy 2 (~2–5 ms, cached 10 s per PID)**: In-process AX API BFS.
///
/// `run_osascript` spawns a subprocess that has NO accessibility permission.
/// This function runs entirely within the DayTrail process (which DOES have the
/// permission) using the same C-level AX API as `accessibility_focused_window_title`.
///
/// Does a BFS through `AXChildren` up to depth 10, visiting at most 400 nodes,
/// looking for any element with role `AXHeading` whose value differs from the
/// app name.  Works for Electron apps (Claude, Codex, Notion, Linear, Obsidian…)
/// Write a debug line to /tmp/daytrail-ax.log (works from a signed .app bundle
/// where stderr isn't visible in the terminal).
#[cfg(target_os = "macos")]
fn ax_log(msg: &str) {
    if std::env::var_os("DAYTRAIL_AX_DEBUG").is_none() {
        return;
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/daytrail-ax.log")
    {
        let _ = writeln!(f, "[DT-AX] {}", msg);
    }
}

#[cfg(target_os = "macos")]
fn ax_content_title_cached(pid: u32, app_name: &str) -> Option<String> {
    use core_foundation::{
        base::{CFRelease, CFTypeRef, TCFType},
        string::{CFString, CFStringRef},
    };
    use std::{collections::VecDeque, ffi::c_void, ptr};

    const CACHE_TTL_SECS: u64 = 10;
    /// Shorter TTL used when the last result was None — AX may just need init.
    const CACHE_TTL_RETRY_SECS: u64 = 2;
    const MAX_DEPTH: u32 = 25; // Electron trees can be 20+ levels deep from window root
    const MAX_NODES: usize = 1200;

    // --- check cache first ---
    {
        let mut guard = CONTENT_TITLE_CACHE.lock().ok()?;
        let cache = guard.get_or_insert_with(HashMap::new);
        if let Some((cached, captured_at)) = cache.get(&pid) {
            let ttl = if cached.is_some() {
                CACHE_TTL_SECS
            } else {
                CACHE_TTL_RETRY_SECS
            };
            if captured_at.elapsed().ok()?.as_secs() < ttl {
                return cached.clone();
            }
        }
    }

    // --- C-level AX BFS (in-process, no permission issue) ---
    type AXUIElementRef = *const c_void;
    type AXError = i32;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
        fn AXUIElementSetAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> AXError;
        fn AXIsProcessTrusted() -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
        fn CFStringGetTypeID() -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFArrayGetCount(array: CFTypeRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CFTypeRef, idx: isize) -> CFTypeRef;
        fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
        /// kCFBooleanTrue — a CFBoolean constant exported by CoreFoundation
        static kCFBooleanTrue: CFTypeRef;
    }

    unsafe fn ax_str(el: AXUIElementRef, attr: &str) -> Option<String> {
        let key = CFString::new(attr);
        let mut val: CFTypeRef = ptr::null();
        if AXUIElementCopyAttributeValue(el, key.as_concrete_TypeRef(), &mut val) != 0
            || val.is_null()
        {
            return None;
        }
        if CFGetTypeID(val) != CFStringGetTypeID() {
            CFRelease(val);
            return None;
        }
        let s = CFString::wrap_under_create_rule(val as CFStringRef).to_string();
        let s = s.trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    unsafe fn ax_bool(el: AXUIElementRef, attr: &str) -> Option<bool> {
        let key = CFString::new(attr);
        let mut val: CFTypeRef = ptr::null();
        if AXUIElementCopyAttributeValue(el, key.as_concrete_TypeRef(), &mut val) != 0
            || val.is_null()
        {
            return None;
        }
        if CFGetTypeID(val) != CFBooleanGetTypeID() {
            CFRelease(val);
            return None;
        }
        let out = CFBooleanGetValue(val) != 0;
        CFRelease(val);
        Some(out)
    }

    /// Returns retained children (caller must CFRelease each).
    unsafe fn ax_children(el: AXUIElementRef) -> Vec<AXUIElementRef> {
        let key = CFString::new("AXChildren");
        let mut val: CFTypeRef = ptr::null();
        if AXUIElementCopyAttributeValue(el, key.as_concrete_TypeRef(), &mut val) != 0
            || val.is_null()
        {
            return Vec::new();
        }
        if CFGetTypeID(val) != CFArrayGetTypeID() {
            CFRelease(val);
            return Vec::new();
        }
        let count = CFArrayGetCount(val).max(0);
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let child = CFArrayGetValueAtIndex(val, i);
            if !child.is_null() {
                CFRetain(child); // take ownership
                out.push(child as AXUIElementRef);
            }
        }
        CFRelease(val);
        out
    }

    /// BFS from `root` (already owned/retained).
    ///
    /// Two strategies — whichever fires first wins:
    ///   1. **AXHeading**: any `<h1>`/`<h2>` element in the content tree whose
    ///      text differs from the app name.  Returned immediately.
    ///   2. **AXWebArea.AXTitle**: the Chromium/Electron page `<title>` attribute,
    ///      exposed on the web-area element.  Stored as a fallback; returned only
    ///      if no heading is found within the node budget.
    unsafe fn bfs_find_title(root: AXUIElementRef, app_name: &str) -> Option<String> {
        let mut queue: VecDeque<(AXUIElementRef, u32)> = VecDeque::new();
        queue.push_back((root, 0));
        let mut visited = 0usize;
        let mut web_area_title: Option<String> = None;
        let mut active_item_title: Option<(i32, String)> = None;

        while let Some((el, depth)) = queue.pop_front() {
            let role = ax_str(el, "AXRole").unwrap_or_default();
            let title = ax_str(el, "AXTitle").unwrap_or_default();
            let value = ax_str(el, "AXValue").unwrap_or_default();
            let description = ax_str(el, "AXDescription").unwrap_or_default();
            let selected = ax_bool(el, "AXSelected");
            let focused = ax_bool(el, "AXFocused");

            // Debug: log every node so we can see the tree shape
            if !role.is_empty() {
                ax_log(&format!(
                    "d={} role={:?} selected={:?} focused={:?} title={:?} value={:?} description={:?}",
                    depth, role,
                    selected, focused,
                    &title[..title.len().min(60)],
                    &value[..value.len().min(60)],
                    &description[..description.len().min(60)]
                ));
            }

            // ── Strategy 1: explicit heading ────────────────────────────────
            if role == "AXHeading" {
                if let Some((_, candidate)) = score_ax_context_candidate(
                    &role,
                    &title,
                    &value,
                    &description,
                    app_name,
                    selected,
                    focused,
                ) {
                    CFRelease(el as CFTypeRef);
                    for (e, _) in queue.drain(..) {
                        CFRelease(e as CFTypeRef);
                    }
                    return Some(candidate);
                }
            }

            // ── Strategy 2: selected/current item rows ──────────────────────
            // Chat, meeting, AI, and Electron apps often expose the active
            // conversation/document as a selected row/button/cell even when the
            // macOS window title is only the app name.
            keep_better_candidate(
                &mut active_item_title,
                score_ax_context_candidate(
                    &role,
                    &title,
                    &value,
                    &description,
                    app_name,
                    selected,
                    focused,
                ),
            );

            // ── Strategy 3: page-title via AXWebArea ────────────────────────
            if role == "AXWebArea" && web_area_title.is_none() {
                let t = if !title.is_empty() {
                    Some(title.clone())
                } else {
                    None
                };
                if let Some(t) = t {
                    let stripped =
                        enrich_title_from_pattern(&t, app_name).unwrap_or_else(|| t.clone());
                    if stripped.len() > 3 && !stripped.eq_ignore_ascii_case(app_name) {
                        ax_log(&format!("WebArea title hit: {:?}", stripped));
                        web_area_title = Some(stripped);
                    }
                }
            }

            // Expand children within limits
            if depth < MAX_DEPTH && visited < MAX_NODES {
                for child in ax_children(el) {
                    queue.push_back((child, depth + 1));
                }
            }

            CFRelease(el as CFTypeRef);
            visited += 1;
        }

        let active_item_title = active_item_title.map(|(_, title)| title);
        ax_log(&format!(
            "BFS done: visited={} active_item={:?} web_area={:?}",
            visited, active_item_title, web_area_title
        ));
        active_item_title.or(web_area_title)
    }

    ax_log(&format!(
        "ax_content_title_cached pid={} app={:?}",
        pid, app_name
    ));

    let enriched = unsafe {
        // Check if this process has TCC accessibility permission
        let trusted = AXIsProcessTrusted() != 0;
        ax_log(&format!("AXIsProcessTrusted={}", trusted));

        let app_el = AXUIElementCreateApplication(pid as i32);
        if app_el.is_null() {
            ax_log("AXUIElementCreateApplication returned null");
            None
        } else {
            // Trigger Chromium/Electron lazy accessibility init.
            // For native apps this is a no-op; for Electron it wakes up the AX
            // subsystem so that AXWindows/AXFocusedWindow become available on
            // the NEXT poll (2 s later — see CACHE_TTL_RETRY_SECS).
            let key_manual = CFString::new("AXManualAccessibility");
            let r = AXUIElementSetAttributeValue(
                app_el,
                key_manual.as_concrete_TypeRef(),
                kCFBooleanTrue,
            );
            ax_log(&format!("AXManualAccessibility set err={}", r));

            // Try focused / main window first
            let key_fw = CFString::new("AXFocusedWindow");
            let key_mw = CFString::new("AXMainWindow");
            let mut win_val: CFTypeRef = ptr::null();
            let fw_err =
                AXUIElementCopyAttributeValue(app_el, key_fw.as_concrete_TypeRef(), &mut win_val);
            if fw_err != 0 || win_val.is_null() {
                ax_log(&format!("FocusedWindow err={fw_err}, trying MainWindow"));
                win_val = ptr::null();
                let mw_err = AXUIElementCopyAttributeValue(
                    app_el,
                    key_mw.as_concrete_TypeRef(),
                    &mut win_val,
                );
                ax_log(&format!(
                    "MainWindow err={mw_err} null={}",
                    win_val.is_null()
                ));
            }

            // Fallback: AXWindows array (first element)
            if win_val.is_null() {
                let key_ws = CFString::new("AXWindows");
                let mut ws_val: CFTypeRef = ptr::null();
                let ws_err = AXUIElementCopyAttributeValue(
                    app_el,
                    key_ws.as_concrete_TypeRef(),
                    &mut ws_val,
                );
                ax_log(&format!("AXWindows err={ws_err} null={}", ws_val.is_null()));
                if ws_err == 0 && !ws_val.is_null() {
                    if CFGetTypeID(ws_val) == CFArrayGetTypeID() {
                        let count = CFArrayGetCount(ws_val);
                        ax_log(&format!("AXWindows count={count}"));
                        if count > 0 {
                            let first = CFArrayGetValueAtIndex(ws_val, 0);
                            if !first.is_null() {
                                CFRetain(first);
                                win_val = first;
                            }
                        }
                    }
                    CFRelease(ws_val);
                }
            }

            // Last resort: BFS from the application element itself
            if win_val.is_null() {
                ax_log("No window found; BFS from app element");
                bfs_find_title(app_el as AXUIElementRef, app_name)
            } else {
                CFRelease(app_el as CFTypeRef);

                // Strategy 3: AXFocusedUIElement walk-up
                // For Electron/Chromium apps the BFS only sees native shell elements;
                // the focused element IS inside the web content and its parent chain
                // often contains an AXHeading with the page/document title.
                let walk_result = {
                    let key_focused = CFString::new("AXFocusedUIElement");
                    let mut focused: CFTypeRef = ptr::null();
                    let fe_err = AXUIElementCopyAttributeValue(
                        win_val as AXUIElementRef,
                        key_focused.as_concrete_TypeRef(),
                        &mut focused,
                    );
                    if fe_err == 0 && !focused.is_null() {
                        ax_log("FocusedUIElement found – walking parent chain");
                        // Walk up to 15 parents looking for AXHeading or AXWebArea title
                        let key_parent = CFString::new("AXParent");
                        let key_children = CFString::new("AXChildren");
                        let mut cur = focused;
                        let mut found: Option<String> = None;
                        for _ in 0..20 {
                            // Check this element
                            let role = ax_str(cur as AXUIElementRef, "AXRole").unwrap_or_default();
                            let title =
                                ax_str(cur as AXUIElementRef, "AXTitle").unwrap_or_default();
                            let val = ax_str(cur as AXUIElementRef, "AXValue").unwrap_or_default();
                            let description =
                                ax_str(cur as AXUIElementRef, "AXDescription").unwrap_or_default();
                            let selected = ax_bool(cur as AXUIElementRef, "AXSelected");
                            let focused = ax_bool(cur as AXUIElementRef, "AXFocused");
                            ax_log(&format!(
                                "walk role={:?} selected={:?} focused={:?} title={:?} value={:?} description={:?}",
                                &role,
                                selected,
                                focused,
                                &title[..title.len().min(60)],
                                &val[..val.len().min(60)],
                                &description[..description.len().min(60)]
                            ));

                            if let Some((_, candidate)) = score_ax_context_candidate(
                                &role,
                                &title,
                                &val,
                                &description,
                                app_name,
                                selected,
                                focused,
                            ) {
                                found = Some(candidate);
                                CFRelease(cur);
                                cur = ptr::null();
                                break;
                            }
                            // Also scan siblings at this level for a heading
                            let mut parent_val: CFTypeRef = ptr::null();
                            let p_err = AXUIElementCopyAttributeValue(
                                cur as AXUIElementRef,
                                key_parent.as_concrete_TypeRef(),
                                &mut parent_val,
                            );
                            if p_err != 0 || parent_val.is_null() {
                                CFRelease(cur);
                                cur = ptr::null();
                                break;
                            }
                            // Check siblings (children of parent) for headings
                            let mut children_val: CFTypeRef = ptr::null();
                            let c_err = AXUIElementCopyAttributeValue(
                                parent_val as AXUIElementRef,
                                key_children.as_concrete_TypeRef(),
                                &mut children_val,
                            );
                            if c_err == 0
                                && !children_val.is_null()
                                && CFGetTypeID(children_val) == CFArrayGetTypeID()
                            {
                                let count = CFArrayGetCount(children_val).min(30);
                                for i in 0..count {
                                    let sib = CFArrayGetValueAtIndex(children_val, i);
                                    if sib.is_null() {
                                        continue;
                                    }
                                    let srole =
                                        ax_str(sib as AXUIElementRef, "AXRole").unwrap_or_default();
                                    let stitle = ax_str(sib as AXUIElementRef, "AXTitle")
                                        .unwrap_or_default();
                                    let sval = ax_str(sib as AXUIElementRef, "AXValue")
                                        .unwrap_or_default();
                                    let sdescription =
                                        ax_str(sib as AXUIElementRef, "AXDescription")
                                            .unwrap_or_default();
                                    let sselected = ax_bool(sib as AXUIElementRef, "AXSelected");
                                    let sfocused = ax_bool(sib as AXUIElementRef, "AXFocused");
                                    if let Some((_, candidate)) = score_ax_context_candidate(
                                        &srole,
                                        &stitle,
                                        &sval,
                                        &sdescription,
                                        app_name,
                                        sselected,
                                        sfocused,
                                    ) {
                                        ax_log(&format!("sibling context found: {:?}", candidate));
                                        found = Some(candidate);
                                        break;
                                    }
                                }
                                CFRelease(children_val);
                            }
                            if found.is_some() {
                                CFRelease(cur);
                                cur = ptr::null();
                                CFRelease(parent_val);
                                break;
                            }
                            CFRelease(cur);
                            cur = parent_val;
                        }
                        if !cur.is_null() && found.is_none() {
                            // cur still held — release
                            CFRelease(cur);
                        }
                        found
                    } else {
                        None
                    }
                };

                if walk_result.is_some() {
                    ax_log(&format!("walk-up result: {:?}", walk_result));
                    walk_result
                } else {
                    bfs_find_title(win_val as AXUIElementRef, app_name)
                }
            }
        }
    };

    // --- update cache ---
    if let Ok(mut guard) = CONTENT_TITLE_CACHE.lock() {
        let cache = guard.get_or_insert_with(HashMap::new);
        cache.insert(pid, (enriched.clone(), SystemTime::now()));
    }

    enriched
}

#[cfg(not(target_os = "macos"))]
fn ax_content_title_cached(_pid: u32, _app_name: &str) -> Option<String> {
    None
}

/// Enrich a raw `window_title` when it carries no more information than the
/// app name itself. Returns `Some(better_title)` or `None` if nothing was found.
fn enrich_window_title(
    raw_title: Option<&str>,
    app_name: &str,
    pid: Option<u32>,
) -> Option<String> {
    let title = raw_title.unwrap_or("").trim();

    // If the title is already informative (not just the app name), keep it
    if !title.is_empty() && !title.eq_ignore_ascii_case(app_name) {
        // Still try pattern extraction in case it embeds "Context — App"
        return enrich_title_from_pattern(title, app_name).or_else(|| Some(title.to_string()));
    }

    // Title is uninformative — try the two enrichment strategies
    if let Some(pid) = pid {
        if let Some(enriched) = ax_content_title_cached(pid, app_name) {
            return Some(enriched);
        }
    }

    None
}

pub fn normalize_app_display_name(name: &str) -> &str {
    match name {
        // VS Code family
        "Code" => "Visual Studio Code",
        "Code - Insiders" => "VS Code Insiders",
        "code" => "Visual Studio Code",
        // Cursor (already correct, kept for explicitness)
        "Cursor" => "Cursor",
        // JetBrains IDEs — short names macOS sometimes returns
        "idea" => "IntelliJ IDEA",
        "webstorm" => "WebStorm",
        "pycharm" => "PyCharm",
        "goland" => "GoLand",
        "rider" => "Rider",
        "clion" => "CLion",
        "datagrip" => "DataGrip",
        "rubymine" => "RubyMine",
        // Browsers
        "chrome" => "Google Chrome",
        "Google Chrome Helper" => "Google Chrome",
        "Google Chrome Helper (GPU)" => "Google Chrome",
        "Chromium" => "Chromium",
        // Comms
        "zoom.us" => "Zoom",
        // Terminals
        "iTerm2" => "iTerm2",
        // Everything else: return as-is
        other => other,
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::is_chromium_browser;
    use super::{
        clean_codex_context_title, codex_thread_hint_from_state_db,
        display_app_name_from_executable, is_resume, meaningful_ax_candidate, push_ai_tool_from_path,
        score_ax_context_candidate, should_record, single_workspace_candidate,
        status_prefixed_ax_candidate, terminal_ai_tools_from_ps_output, workspace_from_candidates,
    };
    use std::path::Path;

    #[test]
    fn records_only_while_user_is_active() {
        // Below the 60s idle threshold → capture.
        assert!(should_record(0, 60_000));
        assert!(should_record(59_999, 60_000));
        // At/above the threshold → user is away, skip.
        assert!(!should_record(60_000, 60_000));
        assert!(!should_record(10 * 60_000, 60_000));
    }

    #[test]
    fn detects_suspension_gap_as_resume() {
        let interval_ms = 2_000;
        // Normal ticks (~interval) are not a resume.
        assert!(!is_resume(0, interval_ms));
        assert!(!is_resume(2_100, interval_ms));
        assert!(!is_resume(9_000, interval_ms));
        // A large wall-clock gap (App Nap throttle / sleep) is a resume.
        assert!(is_resume(60_000, interval_ms));
        assert!(is_resume(12 * 60 * 60 * 1_000, interval_ms));
    }

    #[test]
    fn resume_threshold_has_a_floor_for_tiny_intervals() {
        // With a sub-second interval, 5x would be far too twitchy; the 60s floor
        // prevents false "resume" detection on ordinary scheduling jitter.
        let interval_ms = 100;
        assert!(!is_resume(30_000, interval_ms));
        assert!(is_resume(60_000, interval_ms));
    }

    fn heartbeat(tick_at_ms: i64, trusted: Option<bool>) -> super::WatcherHeartbeat {
        super::WatcherHeartbeat {
            last_tick_at_ms: tick_at_ms,
            last_capture_at_ms: None,
            last_idle_ms: 0,
            accessibility_trusted: trusted,
            consecutive_record_errors: 0,
        }
    }

    #[test]
    fn liveness_is_unknown_before_first_heartbeat() {
        assert_eq!(
            super::assess_capture_liveness(None, 1_000_000, 30_000),
            super::CaptureLiveness::Unknown
        );
    }

    #[test]
    fn liveness_flags_a_stalled_watcher() {
        let now = 1_000_000;
        // Last tick 31s ago with a 30s window → stalled (App Nap / hang / dead).
        let hb = heartbeat(now - 31_000, Some(true));
        assert_eq!(
            super::assess_capture_liveness(Some(&hb), now, 30_000),
            super::CaptureLiveness::Stalled
        );
    }

    #[test]
    fn liveness_flags_lost_accessibility_permission() {
        let now = 1_000_000;
        let hb = heartbeat(now - 2_000, Some(false));
        assert_eq!(
            super::assess_capture_liveness(Some(&hb), now, 30_000),
            super::CaptureLiveness::PermissionLost
        );
    }

    #[test]
    fn liveness_is_healthy_when_ticking_and_trusted() {
        let now = 1_000_000;
        let hb = heartbeat(now - 2_000, Some(true));
        assert_eq!(
            super::assess_capture_liveness(Some(&hb), now, 30_000),
            super::CaptureLiveness::Healthy
        );
        // Non-macOS reports `None` for trust and must still read as healthy.
        let hb_no_ax = heartbeat(now - 2_000, None);
        assert_eq!(
            super::assess_capture_liveness(Some(&hb_no_ax), now, 30_000),
            super::CaptureLiveness::Healthy
        );
    }

    #[test]
    fn humanizes_away_durations() {
        assert_eq!(super::humanize_duration_ms(0), "1m"); // floored to a minimum of 1m
        assert_eq!(super::humanize_duration_ms(27 * 60_000), "27m");
        assert_eq!(super::humanize_duration_ms(60 * 60_000), "1h 0m");
        assert_eq!(super::humanize_duration_ms(94 * 60_000), "1h 34m");
    }

    #[test]
    fn stalled_takes_precedence_over_permission_loss() {
        let now = 1_000_000;
        // Both stale AND untrusted → stalled wins (perm reading is itself stale).
        let hb = heartbeat(now - 120_000, Some(false));
        assert_eq!(
            super::assess_capture_liveness(Some(&hb), now, 30_000),
            super::CaptureLiveness::Stalled
        );
    }

    #[test]
    fn matches_focused_vscode_title_to_the_right_project_candidate() {
        let candidates = vec![
            "/Users/alice/work/LMS-production".to_string(),
            "/Users/alice/work/CFM-main".to_string(),
        ];

        assert_eq!(
            workspace_from_candidates(
                Some("deploy-production.yml — CFM-main — Untracked"),
                &candidates,
            )
            .as_deref(),
            Some("/Users/alice/work/CFM-main")
        );
        assert_eq!(
            workspace_from_candidates(
                Some("app.tsx — LMS-production — Visual Studio Code"),
                &candidates,
            )
            .as_deref(),
            Some("/Users/alice/work/LMS-production")
        );
        assert_eq!(workspace_from_candidates(Some("Code"), &candidates), None);
    }

    #[test]
    fn only_uses_single_candidate_when_no_title_match_exists() {
        assert_eq!(
            single_workspace_candidate(&["/Users/alice/work/LMS-production".to_string()])
                .as_deref(),
            Some("/Users/alice/work/LMS-production")
        );
        assert_eq!(
            single_workspace_candidate(&[
                "/Users/alice/work/LMS-production".to_string(),
                "/Users/alice/work/CFM-main".to_string()
            ]),
            None
        );
    }

    #[test]
    fn detects_ai_tools_from_recent_editor_log_paths() {
        let mut tools = Vec::new();
        push_ai_tool_from_path(
            &mut tools,
            Path::new("/Users/alice/Library/Application Support/Code/logs/window1/exthost/Anthropic.claude-code/Claude VSCode.log"),
        );
        push_ai_tool_from_path(
            &mut tools,
            Path::new("/Users/alice/Library/Application Support/Code/logs/window1/exthost/openai.chatgpt/Codex.1.log"),
        );

        assert!(tools.iter().any(|tool| tool == "Claude Code"));
        assert!(tools.iter().any(|tool| tool == "Codex"));
    }

    #[test]
    fn detects_cli_ai_tools_from_terminal_descendant_processes() {
        let output = r#"
86535     1 /Applications/Warp.app/Contents/MacOS/stable
86537 86535 /Applications/Warp.app/Contents/MacOS/stable terminal-server --parent-pid=86535
 5001 86537 -zsh -g --no_rcs
 5204  5001 node /opt/homebrew/bin/gemini --yolo
 5216  5204 /opt/homebrew/Cellar/node/25.9.0_2/bin/node --max-old-space-size=8192 /opt/homebrew/bin/gemini --yolo
40417     1 /Users/alice/.vscode/extensions/openai.chatgpt/bin/macos-aarch64/codex app-server --analytics-default-enabled
"#;

        let tools = terminal_ai_tools_from_ps_output(output, 86535);

        assert_eq!(tools, vec!["Gemini".to_string()]);
    }

    #[test]
    fn extracts_context_from_status_prefixed_accessibility_labels() {
        assert_eq!(
            status_prefixed_ax_candidate(
                "Awaiting input Deep dive into project codebase",
                "Claude",
            ),
            Some((125, "Deep dive into project codebase".to_string()))
        );
        assert_eq!(
            status_prefixed_ax_candidate("Selected Customer escalation", "Slack"),
            Some((106, "Customer escalation".to_string()))
        );
    }

    #[test]
    fn rejects_generic_accessibility_controls_as_context() {
        assert_eq!(meaningful_ax_candidate("Settings", "Slack"), None);
        assert_eq!(meaningful_ax_candidate("ChatGPT", "ChatGPT Atlas"), None);
        assert_eq!(meaningful_ax_candidate("Chats", "WhatsApp"), None);
        assert_eq!(meaningful_ax_candidate("Slack", "Slack"), None);
    }

    #[test]
    fn uses_selected_rows_as_generic_chat_or_document_context() {
        assert_eq!(
            score_ax_context_candidate(
                "AXRow",
                "Engineering standup",
                "",
                "",
                "Slack",
                Some(true),
                Some(false),
            ),
            Some((112, "Engineering standup".to_string()))
        );
        assert_eq!(
            score_ax_context_candidate(
                "AXButton",
                "Open preferences",
                "",
                "",
                "Teams",
                Some(false),
                Some(false),
            ),
            None
        );
    }

    #[test]
    fn extracts_codex_thread_context_from_state_db() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("state_5.sqlite");
        let rollout_path = dir.path().join("rollout.jsonl");
        std::fs::write(&rollout_path, "{}\n").expect("rollout");
        let conn = rusqlite::Connection::open(&db_path).expect("db");
        conn.execute(
            "CREATE TABLE threads (
                rollout_path TEXT NOT NULL,
                title TEXT NOT NULL,
                cwd TEXT NOT NULL
            )",
            [],
        )
        .expect("schema");
        conn.execute(
            "INSERT INTO threads (rollout_path, title, cwd) VALUES (?1, ?2, ?3)",
            (
                rollout_path.to_string_lossy().as_ref(),
                "Fix production capture labels\nwith extra detail",
                "/Users/alice/work/daytrail",
            ),
        )
        .expect("insert");

        let hint = codex_thread_hint_from_state_db(&db_path, &rollout_path).expect("hint");

        assert_eq!(
            hint,
            super::AppContextHint {
                title: Some("Fix production capture labels".to_string()),
                cwd: Some("/Users/alice/work/daytrail".to_string()),
            }
        );
    }

    #[test]
    fn trims_long_codex_titles_without_reading_full_conversation() {
        let title = clean_codex_context_title(
            "Please investigate the capture issue where every Codex app row is shown as only Codex instead of the active project",
        )
        .expect("title");

        assert!(title.len() <= 90);
        assert!(title.starts_with("Please investigate"));
    }

    #[test]
    fn detects_cli_ai_tools_from_editor_integrated_terminal_processes() {
        let output = r#"
10000     1 /Applications/Visual Studio Code.app/Contents/MacOS/Electron
10010 10000 /Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)
10020 10000 /Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin) /Users/alice/.vscode/extensions/openai.chatgpt/bin/macos-aarch64/codex app-server
10030 10010 -zsh
10040 10030 /opt/homebrew/bin/claude --dangerously-skip-permissions
"#;

        let tools = terminal_ai_tools_from_ps_output(output, 10000);

        assert_eq!(tools, vec!["Claude Code".to_string()]);
    }

    #[test]
    fn normalizes_common_windows_executable_names() {
        let cases = [
            (
                r"C:\Users\alice\AppData\Local\Programs\Microsoft VS Code\Code.exe",
                "VS Code",
            ),
            (
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                "Google Chrome",
            ),
            (
                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
                "Microsoft Edge",
            ),
            (
                r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
                "Brave Browser",
            ),
            (r"C:\Program Files\PowerShell\7\pwsh.exe", "PowerShell"),
            (r"C:\Windows\System32\cmd.exe", "Command Prompt"),
            (
                r"C:\Program Files\JetBrains\WebStorm\bin\webstorm64.exe",
                "WebStorm",
            ),
            (
                r"C:\Users\alice\AppData\Local\Programs\cursor\Cursor.exe",
                "Cursor",
            ),
        ];

        for (path, expected) in cases {
            assert_eq!(
                display_app_name_from_executable(path).as_deref(),
                Some(expected)
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn treats_chatgpt_atlas_as_chromium_browser_for_tab_urls() {
        assert!(is_chromium_browser("ChatGPT Atlas"));
    }
}
