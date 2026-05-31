//! Focus Mode: an opt-in, per-session nudge that reminds you when you drift onto
//! a distraction during a focus block. It reuses the capture watcher (which
//! already knows the frontmost app/title/url every tick) and the system
//! notification plugin — DayTrail observes and nudges, it never blocks.
//!
//! Classification is a fast, offline, deterministic rules layer:
//!   * on-task  — matches the chosen focus target (app or project), OR is neutral
//!     (we don't nag on neutral apps when the AI adjudicator is off);
//!   * off-task — matches the distraction list (apps/domains).
//! An optional AI adjudicator for the neutral "gray zone" can be layered on top
//! later; it is off by default.

use std::{
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::active_window::ActiveWindowInfo;

const DEFAULT_GRACE_SECS: i64 = 45;
const DEFAULT_COOLDOWN_SECS: i64 = 180;

/// Distraction keywords matched (lowercase, as substrings) against the app name,
/// window title, AND url — so it works whether or not the browser URL was
/// captured. Bare brand names catch window titles like "… - YouTube".
const DEFAULT_DISTRACTIONS: &[&str] = &[
    "youtube",
    "youtu.be",
    "netflix",
    "twitch",
    "reddit",
    "twitter.com",
    "x.com",
    "instagram",
    "facebook",
    "tiktok",
    "primevideo",
    "prime video",
    "hotstar",
    "9gag",
    "linkedin",
    "whatsapp",
    "telegram",
    "discord",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    OnTask,
    OffTask,
    Neutral,
}

#[derive(Debug, Clone)]
struct FocusSession {
    started_at_ms: i64,
    ends_at_ms: Option<i64>,
    label: String,
    /// App names that count as on-task (lowercased).
    focus_apps: Vec<String>,
    /// Project paths that count as on-task.
    focus_projects: Vec<String>,
    /// Distraction keywords/domains (lowercased).
    distractions: Vec<String>,
    grace_secs: i64,
    cooldown_secs: i64,
    // mutable tracking
    off_task_since_ms: Option<i64>,
    last_nudge_ms: Option<i64>,
    snooze_until_ms: Option<i64>,
    focus_secs: i64,
    off_task_secs: i64,
    last_tick_ms: i64,
    nudge_count: u32,
}

static SESSION: Mutex<Option<FocusSession>> = Mutex::new(None);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartFocusInput {
    /// What you're focusing on (free text, shown in the nudge).
    pub label: Option<String>,
    /// Optional duration; `None`/0 means open-ended.
    pub duration_minutes: Option<u32>,
    /// App names that count as on-task.
    pub focus_apps: Option<Vec<String>>,
    /// Project paths that count as on-task.
    pub focus_projects: Option<Vec<String>>,
    /// Override the default distraction list.
    pub distractions: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusStatus {
    pub active: bool,
    pub label: String,
    pub started_at_ms: i64,
    pub ends_at_ms: Option<i64>,
    pub focus_secs: i64,
    pub off_task_secs: i64,
    pub nudge_count: u32,
    pub snoozed_until_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FocusSummary {
    pub label: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub total_secs: i64,
    pub focus_secs: i64,
    pub off_task_secs: i64,
    pub nudge_count: u32,
}

/// Append a line to the same diagnostics log the watcher uses, so Focus Mode
/// behaviour is debuggable in shipped builds. Best-effort.
fn focus_log(message: &str) {
    let Some(dir) = dirs::data_local_dir() else {
        return;
    };
    let path = dir.join("ai.daytrail.desktop").join("watcher.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{} [focus] {message}", chrono::Utc::now().to_rfc3339());
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// Classify the current window against a focus session's rules. Pure: no state,
/// no I/O — this is the unit-tested heart of Focus Mode.
fn classify(
    info: &ActiveWindowInfo,
    focus_apps: &[String],
    focus_projects: &[String],
    distractions: &[String],
) -> Classification {
    let app = info.app_name.to_ascii_lowercase();
    let haystack = format!(
        "{} {} {}",
        app,
        info.window_title.as_deref().unwrap_or("").to_ascii_lowercase(),
        info.url.as_deref().unwrap_or("").to_ascii_lowercase(),
    );

    // Distractions win only if the window isn't part of the focus target — a
    // YouTube tab inside your focus project's browser is still a distraction,
    // but we check on-task first so an explicitly allowed app is never nagged.
    let on_task_app = focus_apps.iter().any(|a| !a.is_empty() && app.contains(a));
    let on_task_project = info
        .workspace_key
        .as_deref()
        .map(|w| focus_projects.iter().any(|p| !p.is_empty() && w == p))
        .unwrap_or(false);
    if on_task_app || on_task_project {
        return Classification::OnTask;
    }

    if distractions
        .iter()
        .any(|d| !d.is_empty() && haystack.contains(d))
    {
        return Classification::OffTask;
    }

    // No focus target set at all → treat anything non-distraction as on-task
    // (pure distraction-list mode). With a target set, unknown apps are neutral.
    if focus_apps.is_empty() && focus_projects.is_empty() {
        Classification::OnTask
    } else {
        Classification::Neutral
    }
}

/// Whether a nudge should fire, given how long the user has been off-task and
/// the cooldown/snooze gates. Pure for testability.
fn should_nudge(
    off_task_for_secs: i64,
    grace_secs: i64,
    now: i64,
    last_nudge_ms: Option<i64>,
    cooldown_secs: i64,
    snooze_until_ms: Option<i64>,
) -> bool {
    if off_task_for_secs < grace_secs {
        return false;
    }
    if snooze_until_ms.is_some_and(|until| now < until) {
        return false;
    }
    match last_nudge_ms {
        Some(last) => now - last >= cooldown_secs * 1000,
        None => true,
    }
}

pub fn start(input: StartFocusInput) -> FocusStatus {
    let now = now_ms();
    let label = input
        .label
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "Focus session".to_string());
    let distractions = input
        .distractions
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| DEFAULT_DISTRACTIONS.iter().map(|s| s.to_string()).collect())
        .into_iter()
        .map(|d| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect();
    let ends_at_ms = input
        .duration_minutes
        .filter(|m| *m > 0)
        .map(|m| now + (m as i64) * 60_000);

    let session = FocusSession {
        started_at_ms: now,
        ends_at_ms,
        label,
        focus_apps: lowered(input.focus_apps),
        focus_projects: input.focus_projects.unwrap_or_default(),
        distractions,
        grace_secs: DEFAULT_GRACE_SECS,
        cooldown_secs: DEFAULT_COOLDOWN_SECS,
        off_task_since_ms: None,
        last_nudge_ms: None,
        snooze_until_ms: None,
        focus_secs: 0,
        off_task_secs: 0,
        last_tick_ms: now,
        nudge_count: 0,
    };
    let status = status_of(&session);
    if let Ok(mut guard) = SESSION.lock() {
        *guard = Some(session);
    }
    status
}

pub fn end() -> Option<FocusSummary> {
    let mut guard = SESSION.lock().ok()?;
    let session = guard.take()?;
    let ended = now_ms();
    Some(FocusSummary {
        label: session.label,
        started_at_ms: session.started_at_ms,
        ended_at_ms: ended,
        total_secs: (ended - session.started_at_ms) / 1000,
        focus_secs: session.focus_secs,
        off_task_secs: session.off_task_secs,
        nudge_count: session.nudge_count,
    })
}

/// Cheap check for whether a focus session is running, so the watcher can decide
/// to keep evaluating Focus Mode even when the user looks "idle" (e.g. passively
/// watching a video — the exact moment a nudge matters most).
pub fn is_active() -> bool {
    SESSION.lock().map(|guard| guard.is_some()).unwrap_or(false)
}

pub fn snapshot() -> Option<FocusStatus> {
    let guard = SESSION.lock().ok()?;
    guard.as_ref().map(status_of)
}

pub fn snooze(minutes: u32) -> Option<FocusStatus> {
    let mut guard = SESSION.lock().ok()?;
    let session = guard.as_mut()?;
    session.snooze_until_ms = Some(now_ms() + (minutes.max(1) as i64) * 60_000);
    Some(status_of(session))
}

/// Called from the capture watcher each tick. Accumulates on/off-task time and
/// fires a nudge notification when the user has drifted past the grace period.
/// Auto-ends the session when its duration elapses.
pub fn evaluate(app: &AppHandle, info: &ActiveWindowInfo) {
    let nudge: Option<(String, String)> = {
        let Ok(mut guard) = SESSION.lock() else {
            return;
        };
        let Some(session) = guard.as_mut() else {
            return;
        };
        let now = now_ms();

        // Auto-end on duration.
        if session.ends_at_ms.is_some_and(|end| now >= end) {
            *guard = None;
            return;
        }

        let elapsed_secs = ((now - session.last_tick_ms).max(0) / 1000).min(10);
        session.last_tick_ms = now;

        match classify(
            info,
            &session.focus_apps,
            &session.focus_projects,
            &session.distractions,
        ) {
            Classification::OffTask => {
                session.off_task_secs += elapsed_secs;
                if session.off_task_since_ms.is_none() {
                    session.off_task_since_ms = Some(now);
                }
                let off_for = session
                    .off_task_since_ms
                    .map(|since| (now - since) / 1000)
                    .unwrap_or(0);
                if should_nudge(
                    off_for,
                    session.grace_secs,
                    now,
                    session.last_nudge_ms,
                    session.cooldown_secs,
                    session.snooze_until_ms,
                ) {
                    session.last_nudge_ms = Some(now);
                    session.nudge_count += 1;
                    let what = info.app_name.clone();
                    let body = format!(
                        "Focusing on \"{}\" — you've been on {} for {}. Back to it?",
                        session.label,
                        what,
                        humanize_secs(off_for)
                    );
                    Some(("Stay on track".to_string(), body))
                } else {
                    None
                }
            }
            Classification::OnTask => {
                session.focus_secs += elapsed_secs;
                session.off_task_since_ms = None;
                None
            }
            Classification::Neutral => {
                // Don't nag on neutral apps with the AI adjudicator off; count
                // the time as focus so a brief glance at docs isn't punished.
                session.focus_secs += elapsed_secs;
                session.off_task_since_ms = None;
                None
            }
        }
    };

    if let Some((title, body)) = nudge {
        use tauri_plugin_notification::NotificationExt;
        focus_log(&format!("nudge: {body}"));
        let _ = app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .sound(crate::platform::notification_sound())
            .show();
    }
}

fn status_of(session: &FocusSession) -> FocusStatus {
    FocusStatus {
        active: true,
        label: session.label.clone(),
        started_at_ms: session.started_at_ms,
        ends_at_ms: session.ends_at_ms,
        focus_secs: session.focus_secs,
        off_task_secs: session.off_task_secs,
        nudge_count: session.nudge_count,
        snoozed_until_ms: session.snooze_until_ms,
    }
}

fn lowered(values: Option<Vec<String>>) -> Vec<String> {
    values
        .unwrap_or_default()
        .into_iter()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .collect()
}

fn humanize_secs(secs: i64) -> String {
    let minutes = secs / 60;
    if minutes >= 1 {
        format!("{minutes}m")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(app: &str, title: &str, url: Option<&str>, workspace: Option<&str>) -> ActiveWindowInfo {
        ActiveWindowInfo {
            app_name: app.to_string(),
            window_title: Some(title.to_string()),
            process_id: None,
            url: url.map(ToString::to_string),
            workspace_key: workspace.map(ToString::to_string),
            workspace_candidates: vec![],
            ai_tools: vec![],
            git_context: None,
            captured_at: String::new(),
        }
    }

    fn distractions() -> Vec<String> {
        vec!["youtube.com".into(), "reddit.com".into()]
    }

    #[test]
    fn flags_distraction_domains_as_off_task() {
        let i = info("Chrome", "Lofi beats - YouTube", Some("https://youtube.com/watch"), None);
        assert_eq!(
            classify(&i, &["code".into()], &[], &distractions()),
            Classification::OffTask
        );
    }

    #[test]
    fn focus_app_is_on_task_even_if_title_mentions_a_site() {
        // An explicitly allowed app is never nagged.
        let i = info("Code", "youtube.com integration test", None, Some("/repo/app"));
        assert_eq!(
            classify(&i, &["code".into()], &["/repo/app".into()], &distractions()),
            Classification::OnTask
        );
    }

    #[test]
    fn focus_project_match_is_on_task() {
        let i = info("iTerm", "zsh", None, Some("/repo/app"));
        assert_eq!(
            classify(&i, &[], &["/repo/app".into()], &distractions()),
            Classification::OnTask
        );
    }

    #[test]
    fn unknown_app_is_neutral_when_a_target_exists() {
        let i = info("Notes", "groceries", None, None);
        assert_eq!(
            classify(&i, &["code".into()], &[], &distractions()),
            Classification::Neutral
        );
    }

    #[test]
    fn pure_distraction_list_mode_treats_unknown_as_on_task() {
        let i = info("Notes", "groceries", None, None);
        assert_eq!(classify(&i, &[], &[], &distractions()), Classification::OnTask);
    }

    #[test]
    fn nudge_waits_for_grace_then_respects_cooldown_and_snooze() {
        let now = 1_000_000_000;
        // Under grace → no nudge.
        assert!(!should_nudge(30, 60, now, None, 300, None));
        // Past grace, no prior nudge → nudge.
        assert!(should_nudge(70, 60, now, None, 300, None));
        // Within cooldown → no nudge.
        assert!(!should_nudge(70, 60, now, Some(now - 100_000), 300, None));
        // After cooldown → nudge.
        assert!(should_nudge(70, 60, now, Some(now - 301_000), 300, None));
        // Snoozed → no nudge even past grace.
        assert!(!should_nudge(70, 60, now, None, 300, Some(now + 60_000)));
    }
}
