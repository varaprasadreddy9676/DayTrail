use std::sync::Mutex;

use tauri::AppHandle;

use crate::{
    active_window::ActiveWindowInfo,
    models::{ExportRangeInput, RecoveryEvent, RecoveryEventInput, RecoverySummary, Settings},
    store::WorktraceStore,
};

static RECOVERY_RUNTIME: Mutex<Option<RecoveryRuntime>> = Mutex::new(None);

pub const DEFAULT_RECOVERY_THRESHOLD_MS: i64 = 30 * 60 * 1000;
pub const DEFAULT_RECOVERY_SNOOZE_MS: i64 = 5 * 60 * 1000;
const DEFAULT_RECOVERY_COOLDOWN_MS: i64 = 3 * 60 * 1000;
const DEFAULT_RECOVERY_ACTIVE_GRACE_MS: u64 = 4 * 60 * 1000;
const DEFAULT_RECOVERY_UNKNOWN_GRACE_MS: i64 = 15 * 1000;
const MAX_DAILY_SKIPS: u32 = 3;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryConfig {
    pub enabled: bool,
    pub threshold_ms: i64,
    pub active_grace_ms: u64,
    pub cooldown_ms: i64,
}

impl RecoveryConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        let threshold_minutes = settings.recovery_threshold_minutes.clamp(15, 120);
        Self {
            enabled: settings.recovery_enabled,
            threshold_ms: threshold_minutes * 60_000,
            active_grace_ms: DEFAULT_RECOVERY_ACTIVE_GRACE_MS,
            cooldown_ms: DEFAULT_RECOVERY_COOLDOWN_MS,
        }
    }

    #[cfg(test)]
    fn enabled_for_tests(threshold_ms: i64) -> Self {
        Self {
            enabled: true,
            threshold_ms,
            active_grace_ms: DEFAULT_RECOVERY_ACTIVE_GRACE_MS,
            cooldown_ms: DEFAULT_RECOVERY_COOLDOWN_MS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecoveryTick {
    pub now_ms: i64,
    pub can_track: bool,
    pub paused: bool,
    pub idle_ms: u64,
    pub info: Option<ActiveWindowInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryDecision {
    pub should_notify: bool,
    pub title: Option<String>,
    pub body: Option<String>,
    pub event_type: Option<String>,
    pub reminder_kind: Option<String>,
    pub streak_ms: i64,
}

#[derive(Debug, Clone, Default)]
pub struct RecoveryRuntime {
    streak_started_at_ms: Option<i64>,
    cycle_started_at_ms: Option<i64>,
    next_stage_index: usize,
    last_prompt_ms: Option<i64>,
    snoozed_until_ms: Option<i64>,
    unknown_context_since_ms: Option<i64>,
    daily_skip_count: u32,
    day_key: i64,
}

impl RecoveryRuntime {
    pub fn evaluate(&mut self, tick: RecoveryTick, config: &RecoveryConfig) -> RecoveryDecision {
        self.reset_day_if_needed(tick.now_ms);
        let recently_active = tick.idle_ms <= config.active_grace_ms;
        if !config.enabled || tick.paused || !recently_active {
            self.reset_streak();
            return RecoveryDecision::default();
        }
        if tick.info.is_none() || !tick.can_track {
            if tick.info.is_none()
                && recently_active
                && self
                    .unknown_context_since_ms
                    .get_or_insert(tick.now_ms)
                    .saturating_add(DEFAULT_RECOVERY_UNKNOWN_GRACE_MS)
                    > tick.now_ms
            {
                let streak_ms = self
                    .streak_started_at_ms
                    .map(|started_at| tick.now_ms.saturating_sub(started_at))
                    .unwrap_or_default();
                return RecoveryDecision {
                    streak_ms,
                    ..RecoveryDecision::default()
                };
            }
            self.reset_streak();
            return RecoveryDecision::default();
        }
        self.unknown_context_since_ms = None;
        let context_allowed = tick
            .info
            .as_ref()
            .is_some_and(context_allows_recovery_reminders);
        if !context_allowed {
            self.reset_streak();
            return RecoveryDecision::default();
        }

        let started_at = self.streak_started_at_ms.get_or_insert(tick.now_ms);
        let streak_ms = tick.now_ms.saturating_sub(*started_at);
        let cycle_started_at = self.cycle_started_at_ms.get_or_insert(tick.now_ms);
        let cycle_ms = tick.now_ms.saturating_sub(*cycle_started_at);
        let stages = reminder_stages(config);
        let next_stage = stages.get(self.next_stage_index);

        if next_stage.is_none()
            || next_stage.is_some_and(|stage| cycle_ms < stage.at_ms)
            || self.daily_skip_count >= MAX_DAILY_SKIPS
            || self
                .snoozed_until_ms
                .is_some_and(|until| tick.now_ms < until)
        {
            return RecoveryDecision {
                streak_ms,
                ..RecoveryDecision::default()
            };
        }

        let stage = next_stage.expect("checked above");
        self.last_prompt_ms = Some(tick.now_ms);
        self.next_stage_index += 1;
        if stage.kind == RecoveryReminderKind::Break {
            self.cycle_started_at_ms = Some(tick.now_ms);
            self.next_stage_index = 0;
        }
        RecoveryDecision {
            should_notify: true,
            title: Some(stage.title.to_string()),
            body: Some(stage.body(streak_ms)),
            event_type: Some(stage.event_type.to_string()),
            reminder_kind: Some(stage.kind.as_str().to_string()),
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
        self.reset_streak();
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

    fn reset_streak(&mut self) {
        self.streak_started_at_ms = None;
        self.cycle_started_at_ms = None;
        self.next_stage_index = 0;
        self.unknown_context_since_ms = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryReminderKind {
    Blink,
    Posture,
    Break,
}

impl RecoveryReminderKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Blink => "blink",
            Self::Posture => "posture",
            Self::Break => "break",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RecoveryStage {
    kind: RecoveryReminderKind,
    at_ms: i64,
    title: &'static str,
    event_type: &'static str,
}

impl RecoveryStage {
    fn body(self, streak_ms: i64) -> String {
        let minutes = (streak_ms / 60_000).max(1);
        match self.kind {
            RecoveryReminderKind::Blink => {
                format!(
                    "{minutes}m of steady input. Soften your gaze and blink slowly a few times."
                )
            }
            RecoveryReminderKind::Posture => {
                "Quick posture reset: drop your shoulders, relax your jaw, and unclench your hands."
                    .to_string()
            }
            RecoveryReminderKind::Break => {
                format!("{minutes}m of continuous input. Look away, stand up, or take a two-minute reset before you continue.")
            }
        }
    }
}

fn reminder_stages(config: &RecoveryConfig) -> Vec<RecoveryStage> {
    let threshold = config.threshold_ms.max(15 * 60_000);
    let blink_at = (threshold / 3)
        .clamp(8 * 60_000, 20 * 60_000)
        .min(threshold - 5 * 60_000);
    let posture_at = ((threshold * 2) / 3)
        .clamp(15 * 60_000, 45 * 60_000)
        .min(threshold - 2 * 60_000)
        .max(blink_at + 5 * 60_000);
    vec![
        RecoveryStage {
            kind: RecoveryReminderKind::Blink,
            at_ms: blink_at,
            title: "Blink check",
            event_type: "blink_prompted",
        },
        RecoveryStage {
            kind: RecoveryReminderKind::Posture,
            at_ms: posture_at.min(threshold - 1),
            title: "Posture reset",
            event_type: "posture_prompted",
        },
        RecoveryStage {
            kind: RecoveryReminderKind::Break,
            at_ms: threshold,
            title: "Give your eyes a reset",
            event_type: "break_prompted",
        },
    ]
}

fn context_allows_recovery_reminders(info: &ActiveWindowInfo) -> bool {
    let haystack = [
        info.app_name.as_str(),
        info.window_title.as_deref().unwrap_or_default(),
        info.url.as_deref().unwrap_or_default(),
    ]
    .join(" ")
    .to_ascii_lowercase();

    let quiet_contexts = [
        "zoom",
        "google meet",
        "meet.google.com",
        "microsoft teams",
        "facetime",
        "webex",
        "slack huddle",
        "presentation",
        "presenting",
        "slideshow",
        "slide show",
        "keynote",
        "powerpoint",
    ];

    !quiet_contexts
        .iter()
        .any(|needle| haystack.contains(needle))
}

pub fn evaluate(
    app: &AppHandle,
    store: &WorktraceStore,
    info: Option<&ActiveWindowInfo>,
    can_track: bool,
    idle_ms: u64,
) {
    let settings = store.get_settings().unwrap_or_default();
    let config = RecoveryConfig::from_settings(&settings);
    let paused = store
        .pause_state()
        .map(|state| state.paused)
        .unwrap_or(false);
    let excluded = info
        .map(|info| {
            store
                .active_window_context_is_excluded(
                    &info.app_name,
                    info.url.as_deref(),
                    info.workspace_key.as_deref(),
                )
                .unwrap_or(false)
        })
        .unwrap_or(false);
    let decision = {
        let Ok(mut guard) = RECOVERY_RUNTIME.lock() else {
            return;
        };
        let runtime = guard.get_or_insert_with(RecoveryRuntime::default);
        runtime.evaluate(
            RecoveryTick {
                now_ms: now_ms(),
                can_track: can_track && !excluded,
                paused,
                idle_ms,
                info: info.cloned(),
            },
            &config,
        )
    };
    if !decision.should_notify {
        return;
    }

    if let Err(error) = store.record_recovery_event(RecoveryEventInput {
        id: None,
        event_type: decision
            .event_type
            .clone()
            .unwrap_or_else(|| "prompted".to_string()),
        started_at: now_ms(),
        ended_at: None,
        note: Some("Smart Break reminder".to_string()),
        evidence_json: serde_json::to_string(&serde_json::json!({
            "streakMs": decision.streak_ms,
            "kind": decision.reminder_kind,
            "source": "watcher"
        }))
        .ok(),
    }) {
        eprintln!("failed to record Smart Break reminder: {error:#}");
    }

    crate::daytrail_notification::notify(
        app,
        Some(store),
        crate::daytrail_notification::DaytrailNotificationKind::Recovery,
        decision
            .title
            .unwrap_or_else(|| "Give your eyes a reset".to_string()),
        decision.body.unwrap_or_else(|| {
            "Look away, stand up, or take a two-minute reset before you continue.".to_string()
        }),
    );
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
        note: Some(format!("Snoozed Smart Breaks for {minutes}m")),
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
        note: Some("Skipped Smart Break reminder".to_string()),
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
        note: Some(format!("Recorded Smart Break reset {minutes}m")),
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
    fn prompts_for_blink_posture_and_break_during_sustained_input() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig::enabled_for_tests(DEFAULT_RECOVERY_THRESHOLD_MS);
        let stages = reminder_stages(&config);
        let first = runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        assert!(!first.should_notify);

        let blink = runtime.evaluate(
            RecoveryTick {
                now_ms: stages[0].at_ms + 1_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        assert!(blink.should_notify);
        assert_eq!(blink.event_type.as_deref(), Some("blink_prompted"));

        let posture = runtime.evaluate(
            RecoveryTick {
                now_ms: stages[1].at_ms + 1_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        assert!(posture.should_notify);
        assert_eq!(posture.event_type.as_deref(), Some("posture_prompted"));

        let break_prompt = runtime.evaluate(
            RecoveryTick {
                now_ms: stages[2].at_ms + 1_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        assert!(break_prompt.should_notify);
        assert_eq!(break_prompt.event_type.as_deref(), Some("break_prompted"));
    }

    #[test]
    fn minimum_threshold_prompts_break_at_configured_boundary() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig::enabled_for_tests(15 * 60_000);
        let stages = reminder_stages(&config);
        runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        for stage in stages.iter().take(2) {
            let decision = runtime.evaluate(
                RecoveryTick {
                    now_ms: stage.at_ms + 1_000,
                    can_track: true,
                    paused: false,
                    idle_ms: 1_000,
                    info: Some(info("VS Code")),
                },
                &config,
            );
            assert!(decision.should_notify);
        }

        let break_prompt = runtime.evaluate(
            RecoveryTick {
                now_ms: 15 * 60_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );

        assert!(break_prompt.should_notify);
        assert_eq!(break_prompt.event_type.as_deref(), Some("break_prompted"));
    }

    #[test]
    fn transient_unknown_context_does_not_reset_streak() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig::enabled_for_tests(DEFAULT_RECOVERY_THRESHOLD_MS);
        runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        let unknown = runtime.evaluate(
            RecoveryTick {
                now_ms: 20 * 60_000,
                can_track: false,
                paused: false,
                idle_ms: 1_000,
                info: None,
            },
            &config,
        );
        assert!(!unknown.should_notify);

        let blink = runtime.evaluate(
            RecoveryTick {
                now_ms: 20 * 60_000 + 5_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );

        assert!(blink.should_notify);
        assert_eq!(blink.event_type.as_deref(), Some("blink_prompted"));
    }

    #[test]
    fn disabled_recovery_never_prompts() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig {
            enabled: false,
            threshold_ms: DEFAULT_RECOVERY_THRESHOLD_MS,
            active_grace_ms: DEFAULT_RECOVERY_ACTIVE_GRACE_MS,
            cooldown_ms: DEFAULT_RECOVERY_COOLDOWN_MS,
        };
        runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        let decision = runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 1_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );

        assert!(!decision.should_notify);
    }

    #[test]
    fn recent_input_gate_resets_when_user_stops_moving() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig::enabled_for_tests(DEFAULT_RECOVERY_THRESHOLD_MS);
        runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 1_000,
                can_track: true,
                paused: false,
                idle_ms: DEFAULT_RECOVERY_ACTIVE_GRACE_MS + 1,
                info: Some(info("VS Code")),
            },
            &config,
        );
        let decision = runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 2_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );

        assert!(!decision.should_notify);
    }

    #[test]
    fn call_and_presentation_contexts_suppress_reminders() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig::enabled_for_tests(DEFAULT_RECOVERY_THRESHOLD_MS);
        runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("Zoom")),
            },
            &config,
        );
        let decision = runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 1_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("Zoom")),
            },
            &config,
        );

        assert!(!decision.should_notify);
    }

    #[test]
    fn snooze_blocks_prompt_until_gate_expires() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig::enabled_for_tests(DEFAULT_RECOVERY_THRESHOLD_MS);
        runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        runtime.snooze_until(DEFAULT_RECOVERY_THRESHOLD_MS + 20 * 60_000);
        let snoozed = runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 1_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        assert!(!snoozed.should_notify);

        let after_gate = runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 21 * 60_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        assert!(after_gate.should_notify);
    }

    #[test]
    fn daily_skip_limit_suppresses_more_prompts() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig::enabled_for_tests(DEFAULT_RECOVERY_THRESHOLD_MS);
        runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        runtime.record_skip(0);
        runtime.record_skip(1_000);
        runtime.record_skip(2_000);

        let decision = runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 1_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );

        assert!(!decision.should_notify);
        assert_eq!(runtime.daily_skip_count(), 3);
    }

    #[test]
    fn pause_or_inactive_resets_streak() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig::enabled_for_tests(DEFAULT_RECOVERY_THRESHOLD_MS);
        runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 5_000,
                can_track: true,
                paused: true,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        let decision = runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 7_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );

        assert!(!decision.should_notify);
    }

    #[test]
    fn excluded_context_resets_streak_like_inactive_time() {
        let mut runtime = RecoveryRuntime::default();
        let config = RecoveryConfig::enabled_for_tests(DEFAULT_RECOVERY_THRESHOLD_MS);
        runtime.evaluate(
            RecoveryTick {
                now_ms: 0,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );
        runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 5_000,
                can_track: false,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("Private browser")),
            },
            &config,
        );
        let decision = runtime.evaluate(
            RecoveryTick {
                now_ms: DEFAULT_RECOVERY_THRESHOLD_MS + 7_000,
                can_track: true,
                paused: false,
                idle_ms: 1_000,
                info: Some(info("VS Code")),
            },
            &config,
        );

        assert!(!decision.should_notify);
    }
}
