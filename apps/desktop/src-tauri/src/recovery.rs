use std::sync::Mutex;

use tauri::AppHandle;

use crate::{
    active_window::ActiveWindowInfo,
    models::{ExportRangeInput, RecoveryEvent, RecoveryEventInput, RecoverySummary},
    store::WorktraceStore,
};

static RECOVERY_RUNTIME: Mutex<Option<RecoveryRuntime>> = Mutex::new(None);

pub const DEFAULT_RECOVERY_THRESHOLD_MS: i64 = 25 * 60 * 1000;
pub const DEFAULT_RECOVERY_SNOOZE_MS: i64 = 5 * 60 * 1000;
const DEFAULT_RECOVERY_COOLDOWN_MS: i64 = 15 * 60 * 1000;
const MAX_DAILY_SKIPS: u32 = 3;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct RecoveryTick {
    pub now_ms: i64,
    pub active: bool,
    pub paused: bool,
    pub info: Option<ActiveWindowInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryDecision {
    pub should_notify: bool,
    pub title: Option<String>,
    pub body: Option<String>,
    pub event_type: Option<String>,
    pub streak_ms: i64,
}

#[derive(Debug, Clone, Default)]
pub struct RecoveryRuntime {
    streak_started_at_ms: Option<i64>,
    last_prompt_ms: Option<i64>,
    snoozed_until_ms: Option<i64>,
    daily_skip_count: u32,
    day_key: i64,
}

impl RecoveryRuntime {
    pub fn evaluate(&mut self, tick: RecoveryTick) -> RecoveryDecision {
        self.reset_day_if_needed(tick.now_ms);
        if tick.paused || !tick.active || tick.info.is_none() {
            self.streak_started_at_ms = None;
            return RecoveryDecision::default();
        }

        let started_at = self.streak_started_at_ms.get_or_insert(tick.now_ms);
        let streak_ms = tick.now_ms.saturating_sub(*started_at);
        if streak_ms < DEFAULT_RECOVERY_THRESHOLD_MS
            || self.daily_skip_count >= MAX_DAILY_SKIPS
            || self
                .snoozed_until_ms
                .is_some_and(|until| tick.now_ms < until)
            || self
                .last_prompt_ms
                .is_some_and(|last| tick.now_ms.saturating_sub(last) < DEFAULT_RECOVERY_COOLDOWN_MS)
        {
            return RecoveryDecision {
                streak_ms,
                ..RecoveryDecision::default()
            };
        }

        self.last_prompt_ms = Some(tick.now_ms);
        RecoveryDecision {
            should_notify: true,
            title: Some("Time for a quick recovery".to_string()),
            body: Some("Look away, stand up, or reset for a few minutes.".to_string()),
            event_type: Some("prompted".to_string()),
            streak_ms,
        }
    }

    pub fn snooze_until(&mut self, until_ms: i64) {
        self.snoozed_until_ms = Some(until_ms);
    }

    pub fn snooze_for_default(&mut self, now_ms: i64) {
        self.snooze_until(now_ms + DEFAULT_RECOVERY_SNOOZE_MS);
    }

    pub fn record_skip(&mut self, now_ms: i64) {
        self.reset_day_if_needed(now_ms);
        self.daily_skip_count = self.daily_skip_count.saturating_add(1);
        self.snooze_for_default(now_ms);
    }

    pub fn record_taken(&mut self) {
        self.streak_started_at_ms = None;
        self.last_prompt_ms = None;
        self.snoozed_until_ms = None;
    }

    pub fn daily_skip_count(&self) -> u32 {
        self.daily_skip_count
    }

    fn reset_day_if_needed(&mut self, now_ms: i64) {
        let next_key = now_ms.div_euclid(DAY_MS);
        if self.day_key != next_key {
            self.day_key = next_key;
            self.daily_skip_count = 0;
            self.last_prompt_ms = None;
            self.snoozed_until_ms = None;
        }
    }
}

pub fn evaluate(app: &AppHandle, store: &WorktraceStore, info: &ActiveWindowInfo, active: bool) {
    let paused = store
        .pause_state()
        .map(|state| state.paused)
        .unwrap_or(false);
    let excluded = store
        .active_window_context_is_excluded(
            &info.app_name,
            info.url.as_deref(),
            info.workspace_key.as_deref(),
        )
        .unwrap_or(false);
    let decision = {
        let Ok(mut guard) = RECOVERY_RUNTIME.lock() else {
            return;
        };
        let runtime = guard.get_or_insert_with(RecoveryRuntime::default);
        runtime.evaluate(RecoveryTick {
            now_ms: now_ms(),
            active: active && !excluded,
            paused,
            info: Some(info.clone()),
        })
    };
    if !decision.should_notify {
        return;
    }

    let _ = store.record_recovery_event(RecoveryEventInput {
        id: None,
        event_type: decision
            .event_type
            .clone()
            .unwrap_or_else(|| "prompted".to_string()),
        started_at: now_ms(),
        ended_at: None,
        note: Some("Smart Recovery prompt".to_string()),
        evidence_json: serde_json::to_string(&serde_json::json!({
            "streakMs": decision.streak_ms,
            "source": "watcher"
        }))
        .ok(),
    });

    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title(
            decision
                .title
                .unwrap_or_else(|| "Time for a quick recovery".to_string()),
        )
        .body(
            decision
                .body
                .unwrap_or_else(|| "Look away, stand up, or reset for a few minutes.".to_string()),
        )
        .sound(crate::platform::notification_sound())
        .show();
}

pub fn record_event(
    store: &WorktraceStore,
    input: RecoveryEventInput,
) -> anyhow::Result<RecoveryEvent> {
    store.record_recovery_event(input)
}

pub fn summary(
    store: &WorktraceStore,
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> anyhow::Result<RecoverySummary> {
    let export = store.export_data_range(ExportRangeInput {
        from_date: from_date.map(ToString::to_string),
        to_date: to_date.map(ToString::to_string),
    })?;
    Ok(export.recovery_summary)
}

pub fn snooze(store: &WorktraceStore, minutes: u32) -> anyhow::Result<RecoveryEvent> {
    let now = now_ms();
    let minutes = minutes.max(1);
    let until = now + i64::from(minutes) * 60_000;
    if let Ok(mut guard) = RECOVERY_RUNTIME.lock() {
        guard
            .get_or_insert_with(RecoveryRuntime::default)
            .snooze_until(until);
    }
    store.record_recovery_event(RecoveryEventInput {
        id: None,
        event_type: "snoozed".to_string(),
        started_at: now,
        ended_at: Some(until),
        note: Some(format!("Snoozed Smart Recovery for {minutes}m")),
        evidence_json: None,
    })
}

pub fn skip(store: &WorktraceStore) -> anyhow::Result<RecoveryEvent> {
    let now = now_ms();
    if let Ok(mut guard) = RECOVERY_RUNTIME.lock() {
        guard
            .get_or_insert_with(RecoveryRuntime::default)
            .record_skip(now);
    }
    store.record_recovery_event(RecoveryEventInput {
        id: None,
        event_type: "skipped".to_string(),
        started_at: now,
        ended_at: None,
        note: Some("Skipped Smart Recovery prompt".to_string()),
        evidence_json: None,
    })
}

pub fn take_break(store: &WorktraceStore, minutes: u32) -> anyhow::Result<RecoveryEvent> {
    let now = now_ms();
    let minutes = minutes.max(1);
    let ended_at = now + i64::from(minutes) * 60_000;
    if let Ok(mut guard) = RECOVERY_RUNTIME.lock() {
        guard
            .get_or_insert_with(RecoveryRuntime::default)
            .record_taken();
    }
    store.record_recovery_event(RecoveryEventInput {
        id: None,
        event_type: "taken".to_string(),
        started_at: now,
        ended_at: Some(ended_at),
        note: Some(format!("Smart Recovery break {minutes}m")),
        evidence_json: None,
    })
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(app: &str) -> ActiveWindowInfo {
        ActiveWindowInfo {
            app_name: app.to_string(),
            window_title: Some("Work".to_string()),
            process_id: None,
            url: None,
            workspace_key: Some("DayTrail".to_string()),
            workspace_candidates: vec![],
            ai_tools: vec![],
            git_context: None,
            captured_at: String::new(),
        }
    }

    #[test]
    fn prompts_after_uninterrupted_threshold() {
        let mut runtime = RecoveryRuntime::default();
        let first = runtime.evaluate(RecoveryTick {
            now_ms: 0,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });
        assert!(!first.should_notify);

        let prompted = runtime.evaluate(RecoveryTick {
            now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 1_000,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });

        assert!(prompted.should_notify);
        assert_eq!(prompted.event_type.as_deref(), Some("prompted"));
    }

    #[test]
    fn snooze_blocks_prompt_until_gate_expires() {
        let mut runtime = RecoveryRuntime::default();
        runtime.evaluate(RecoveryTick {
            now_ms: 0,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });
        runtime.snooze_until(DEFAULT_RECOVERY_THRESHOLD_MS + 20 * 60_000);
        let snoozed = runtime.evaluate(RecoveryTick {
            now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 1_000,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });
        assert!(!snoozed.should_notify);

        let after_gate = runtime.evaluate(RecoveryTick {
            now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 21 * 60_000,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });
        assert!(after_gate.should_notify);
    }

    #[test]
    fn daily_skip_limit_suppresses_more_prompts() {
        let mut runtime = RecoveryRuntime::default();
        runtime.evaluate(RecoveryTick {
            now_ms: 0,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });
        runtime.record_skip(0);
        runtime.record_skip(1_000);
        runtime.record_skip(2_000);

        let decision = runtime.evaluate(RecoveryTick {
            now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 1_000,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });

        assert!(!decision.should_notify);
        assert_eq!(runtime.daily_skip_count(), 3);
    }

    #[test]
    fn pause_or_inactive_resets_streak() {
        let mut runtime = RecoveryRuntime::default();
        runtime.evaluate(RecoveryTick {
            now_ms: 0,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });
        runtime.evaluate(RecoveryTick {
            now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 5_000,
            active: true,
            paused: true,
            info: Some(info("VS Code")),
        });
        let decision = runtime.evaluate(RecoveryTick {
            now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 7_000,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });

        assert!(!decision.should_notify);
    }

    #[test]
    fn excluded_context_resets_streak_like_inactive_time() {
        let mut runtime = RecoveryRuntime::default();
        runtime.evaluate(RecoveryTick {
            now_ms: 0,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });
        runtime.evaluate(RecoveryTick {
            now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 5_000,
            active: false,
            paused: false,
            info: Some(info("Private browser")),
        });
        let decision = runtime.evaluate(RecoveryTick {
            now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 7_000,
            active: true,
            paused: false,
            info: Some(info("VS Code")),
        });

        assert!(!decision.should_notify);
    }
}
