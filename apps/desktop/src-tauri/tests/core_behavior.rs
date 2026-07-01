use std::{collections::HashMap, fs, io::Cursor, sync::Mutex, thread, time::Duration};

use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;
use worktrace_ai_desktop::{
    models::{
        AgentRunInput, BrowserBridgeEvent, CalendarEventInput, CommitmentInput, EmailThreadInput,
        ExportRangeInput, FieldVisitInput, FocusSessionInput, IdleBlockInput, MeetingInput,
        RecoveryEventInput, ScratchpadNoteInput, SettingsPatch, SourceEventInput,
        StateSnapshotInput, TaskInput, TaskStatus, TerminalBridgeMetadata, WorkOutputInput,
    },
    native_messaging,
    platform::KeychainAdapter,
    project_detection::{detect_project_from_sources, ProjectDetectionSources},
    store::WorktraceStore,
};

#[derive(Default)]
struct TestKeychain {
    values: Mutex<HashMap<String, String>>,
}

fn test_today_noon_ms() -> i64 {
    chrono::Local::now()
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid local noon")
        .and_local_timezone(chrono::Local)
        .earliest()
        .expect("local noon timestamp")
        .timestamp_millis()
}

#[test]
fn calendar_events_reconcile_planned_vs_actual_work() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();
    let meeting_start = now - 90 * 60_000;
    let meeting_end = meeting_start + 60 * 60_000;
    let missed_start = now - 4 * 60 * 60_000;
    let missed_end = missed_start + 45 * 60_000;

    store
        .upsert_calendar_event(CalendarEventInput {
            id: Some("cal-client-sync".into()),
            source: Some("manual".into()),
            external_id: Some("external-client-sync".into()),
            calendar_name: Some("Work".into()),
            title: "Client sync".into(),
            starts_at: meeting_start,
            ends_at: meeting_end,
            location: Some("Google Meet".into()),
            status: Some("confirmed".into()),
            planned_work_type: Some("meeting".into()),
        })
        .expect("calendar event");
    store
        .upsert_calendar_event(CalendarEventInput {
            id: Some("cal-missed-review".into()),
            source: Some("manual".into()),
            external_id: None,
            calendar_name: Some("Work".into()),
            title: "Design review".into(),
            starts_at: missed_start,
            ends_at: missed_end,
            location: None,
            status: Some("confirmed".into()),
            planned_work_type: Some("review".into()),
        })
        .expect("missed calendar event");
    store
        .record_source_event(SourceEventInput {
            id: Some("meeting-signal".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Google Meet".into()),
            title: Some("Client sync".into()),
            url: None,
            workspace_key: Some("meet.google.com".into()),
            started_at: Some(meeting_start + 5 * 60_000),
            ended_at: Some(meeting_end - 5 * 60_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record event");

    let today = store.today_snapshot().expect("today");

    assert_eq!(today.calendar_events.len(), 2);
    assert_eq!(today.calendar_reconciliation.planned_events, 2);
    assert_eq!(today.calendar_reconciliation.matched_events, 1);
    assert_eq!(today.calendar_reconciliation.unmatched_events, 1);
    assert!(today
        .calendar_reconciliation
        .items
        .iter()
        .any(|item| item.title == "Client sync" && item.status == "matched"));
    assert!(today
        .calendar_reconciliation
        .items
        .iter()
        .any(|item| item.title == "Design review" && item.status == "missed"));

    let weekly = store.generate_weekly_review().expect("weekly review");
    assert!(weekly.body_markdown.contains("Planned vs actual"));
    assert!(weekly.body_markdown.contains("Client sync"));
    assert!(weekly.body_markdown.contains("Design review"));
}

#[test]
fn infers_presentation_meeting_blocks_from_sustained_slides_activity() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();
    let start = now - 60 * 60_000;

    for (index, offset_minutes, duration_minutes, domain) in [
        (0, 0, 29, "docs.google.com"),
        (1, 31, 24, "medicsprime.in"),
        (2, 57, 8, "docs.google.com"),
    ] {
        store
            .record_source_event(SourceEventInput {
                id: Some(format!("slides-{index}")),
                source: "active-window".into(),
                event_type: "browser_tab".into(),
                app: Some("ChatGPT Atlas".into()),
                title: Some("Client_Engagement 3rd June - Google Slides".into()),
                url: Some(format!("https://{domain}/presentation/d/client-engagement")),
                workspace_key: Some(domain.into()),
                started_at: Some(start + offset_minutes * 60_000),
                ended_at: Some(start + (offset_minutes + duration_minutes) * 60_000),
                sensitivity: None,
                metadata_json: None,
            })
            .expect("slides event");
    }

    store
        .record_source_event(SourceEventInput {
            id: Some("gitlab-short".into()),
            source: "active-window".into(),
            event_type: "browser_tab".into(),
            app: Some("ChatGPT Atlas".into()),
            title: Some("Review requests - GitLab".into()),
            url: Some("https://gitlab.example/review".into()),
            workspace_key: Some("gitlab.example".into()),
            started_at: Some(start + 65 * 60_000),
            ended_at: Some(start + 66 * 60_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("short event");

    let today = store.today_snapshot().expect("today snapshot");
    let inferred = today
        .inferred_work_blocks
        .iter()
        .find(|block| block.category == "presentation_meeting")
        .expect("presentation meeting inference");

    assert!(inferred.title.contains("Client_Engagement 3rd June"));
    assert!(inferred.duration_ms >= 60 * 60_000);
    assert!(inferred.confidence_percent >= 70);
    assert!(inferred.reason.contains("Google Slides"));
    assert!(inferred.evidence_ids.contains(&"slides-0".to_string()));
    assert!(inferred
        .suggested_actions
        .iter()
        .any(|action| action.contains("meeting")));
}

#[test]
fn focus_sessions_measure_drift_from_the_declared_context() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let start = test_today_noon_ms() - 40 * 60_000;
    let end = start + 30 * 60_000;

    store
        .upsert_focus_session(FocusSessionInput {
            id: Some("focus-ticket-123".into()),
            goal: "Ship ticket 123".into(),
            client: Some("Acme".into()),
            project: Some("DayTrail".into()),
            task: Some("Ticket 123".into()),
            ticket_id: Some("DT-123".into()),
            target_ms: 30 * 60_000,
            started_at: start,
            ended_at: Some(end),
            status: Some("completed".into()),
        })
        .expect("focus session");
    store
        .record_source_event(SourceEventInput {
            id: Some("focus-code".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("VS Code".into()),
            title: Some("DayTrail App.tsx".into()),
            url: None,
            workspace_key: Some("DayTrail".into()),
            started_at: Some(start),
            ended_at: Some(start + 20 * 60_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record code");
    store
        .record_source_event(SourceEventInput {
            id: Some("focus-drift".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("YouTube".into()),
            title: Some("Unrelated video".into()),
            url: None,
            workspace_key: Some("youtube.com".into()),
            started_at: Some(start + 20 * 60_000),
            ended_at: Some(end),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record drift");

    let today = store.today_snapshot().expect("today");
    let focus = today
        .focus_sessions
        .iter()
        .find(|session| session.id == "focus-ticket-123")
        .expect("focus session in snapshot");

    assert_eq!(focus.status, "completed");
    assert_eq!(focus.actual_duration_ms, 30 * 60_000);
    assert_eq!(focus.matched_work_ms, 20 * 60_000);
    assert_eq!(focus.drift_ms, 10 * 60_000);
    assert!(focus
        .drift_events
        .iter()
        .any(|event| event.contains("YouTube")));
}

#[test]
fn smart_breaks_score_long_work_and_logged_breaks() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let start = test_today_noon_ms() - 90 * 60_000;
    let first_end = start + 32 * 60_000;
    let second_start = first_end;
    let second_end = second_start + 24 * 60_000;
    let break_start = second_end;
    let break_end = break_start + 4 * 60_000;
    let final_start = break_end;
    let final_end = final_start + 18 * 60_000;

    store
        .record_source_event(SourceEventInput {
            id: Some("recovery-code".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("VS Code".into()),
            title: Some("Smart Break implementation".into()),
            url: None,
            workspace_key: Some("DayTrail".into()),
            started_at: Some(start),
            ended_at: Some(first_end),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record code");
    store
        .record_source_event(SourceEventInput {
            id: Some("recovery-browser".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Chrome".into()),
            title: Some("Rust docs".into()),
            url: Some("https://doc.rust-lang.org/std/time/".into()),
            workspace_key: Some("doc.rust-lang.org".into()),
            started_at: Some(second_start),
            ended_at: Some(second_end),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record browser");
    store
        .record_source_event(SourceEventInput {
            id: Some("recovery-code-after-break".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("VS Code".into()),
            title: Some("Smart Break settings".into()),
            url: None,
            workspace_key: Some("DayTrail".into()),
            started_at: Some(final_start),
            ended_at: Some(final_end),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record final code");
    store
        .record_recovery_event(RecoveryEventInput {
            id: Some("recovery-break-1".into()),
            event_type: "taken".into(),
            started_at: break_start,
            ended_at: Some(break_end),
            note: Some("Looked away and stretched".into()),
            evidence_json: None,
        })
        .expect("record recovery");

    let today = store.today_snapshot().expect("today");

    assert_eq!(today.recovery_summary.taken_count, 1);
    assert_eq!(today.recovery_summary.skipped_count, 0);
    assert_eq!(today.recovery_summary.longest_uninterrupted_ms, 56 * 60_000);
    assert!(today.recovery_summary.score >= 70);
    assert!(today.recovery_summary.score <= 100);
    assert_eq!(
        today
            .recovery_summary
            .next_prompt
            .as_ref()
            .map(|prompt| prompt.action.as_str()),
        Some("ready")
    );
}

#[test]
fn weekly_review_includes_smart_breaks() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let start = test_today_noon_ms() - 2 * 60 * 60_000;
    let end = start + 65 * 60_000;

    store
        .record_source_event(SourceEventInput {
            id: Some("weekly-recovery-run".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("VS Code".into()),
            title: Some("Long implementation block".into()),
            url: None,
            workspace_key: Some("DayTrail".into()),
            started_at: Some(start),
            ended_at: Some(end),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record source");
    store
        .record_recovery_event(RecoveryEventInput {
            id: Some("weekly-recovery-skip".into()),
            event_type: "skipped".into(),
            started_at: end,
            ended_at: None,
            note: Some("Shipping build".into()),
            evidence_json: None,
        })
        .expect("record skip");

    let export = store
        .export_data_range(ExportRangeInput {
            from_date: Some(
                chrono::Local::now()
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string(),
            ),
            to_date: Some(
                chrono::Local::now()
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string(),
            ),
        })
        .expect("export");
    assert_eq!(export.recovery_summary.skipped_count, 1);
    assert_eq!(export.recovery_events.len(), 1);
    assert_eq!(export.recovery_events[0].id, "weekly-recovery-skip");

    let review = store.generate_weekly_review().expect("weekly review");

    assert!(review.body_markdown.contains("Smart Breaks"));
    assert!(review.body_markdown.contains("Longest uninterrupted"));
    assert!(review.body_markdown.contains("1 skipped"));
}

#[test]
fn staged_smart_break_prompt_events_are_persisted() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();

    store
        .record_recovery_event(RecoveryEventInput {
            id: Some("blink-stage".into()),
            event_type: "blink_prompted".into(),
            started_at: now,
            ended_at: None,
            note: Some("Smart Break reminder".into()),
            evidence_json: Some(json!({ "kind": "blink" }).to_string()),
        })
        .expect("record staged prompt");

    let export = store.export_data().expect("export");

    assert_eq!(export.recovery_events.len(), 1);
    assert_eq!(export.recovery_events[0].event_type, "blink_prompted");
    assert_eq!(export.recovery_summary.prompted_count, 1);
}

#[test]
fn smart_break_summary_uses_configured_threshold() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    store
        .update_settings(SettingsPatch {
            recovery_threshold_minutes: Some(45),
            ..Default::default()
        })
        .expect("settings");
    let start = test_today_noon_ms() - 35 * 60_000;
    let end = start + 35 * 60_000;

    store
        .record_source_event(SourceEventInput {
            id: Some("configured-smart-break-threshold".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("VS Code".into()),
            title: Some("Configured Smart Break threshold".into()),
            url: None,
            workspace_key: Some("DayTrail".into()),
            started_at: Some(start),
            ended_at: Some(end),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record source");

    let today = store.today_snapshot().expect("today");

    assert_eq!(
        today
            .recovery_summary
            .next_prompt
            .as_ref()
            .map(|prompt| prompt.action.as_str()),
        Some("ready")
    );
}

#[test]
fn recovery_breaks_split_overlapping_active_window_spans() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let start = test_today_noon_ms() - 90 * 60_000;
    let end = start + 60 * 60_000;
    let break_start = start + 30 * 60_000;
    let break_end = break_start + 5 * 60_000;

    store
        .record_source_event(SourceEventInput {
            id: Some("spanning-recovery-run".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("VS Code".into()),
            title: Some("One long captured span".into()),
            url: None,
            workspace_key: Some("DayTrail".into()),
            started_at: Some(start),
            ended_at: Some(end),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record source");
    store
        .record_recovery_event(RecoveryEventInput {
            id: Some("overlap-break".into()),
            event_type: "taken".into(),
            started_at: break_start,
            ended_at: Some(break_end),
            note: Some("Actual break inside a coalesced active-window span".into()),
            evidence_json: None,
        })
        .expect("record break");

    let today = store.today_snapshot().expect("today");

    assert_eq!(today.recovery_summary.total_screen_ms, 55 * 60_000);
    assert_eq!(today.recovery_summary.longest_uninterrupted_ms, 30 * 60_000);
    assert_eq!(today.recovery_summary.taken_count, 1);
}

#[test]
fn weekly_review_uses_last_seven_days_not_only_today() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let three_days_ago = chrono::Local::now().timestamp_millis() - 3 * 24 * 60 * 60_000;

    store
        .record_source_event(SourceEventInput {
            id: Some("weekly-prior-work".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("VS Code".into()),
            title: Some("Refactored calendar reconciliation".into()),
            url: None,
            workspace_key: Some("DayTrail".into()),
            started_at: Some(three_days_ago),
            ended_at: Some(three_days_ago + 30 * 60_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record prior work");
    store
        .materialize_work_memory()
        .expect("materialize prior work");

    let review = store.generate_weekly_review().expect("weekly review");

    assert_eq!(review.report_type, "weekly_review");
    assert!(review
        .body_markdown
        .contains("Refactored calendar reconciliation"));
    assert!(review.body_markdown.contains("AI weekly auto-draft"));
}

#[test]
fn records_unclassified_idle_gap_candidate_for_return_prompt() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let start = test_today_noon_ms() - 35 * 60_000;
    let end = start + 25 * 60_000;

    let recorded = store
        .record_idle_gap_candidate("resume", start, end)
        .expect("record candidate")
        .expect("candidate should be recorded");

    assert!(!recorded.classified);
    assert!(recorded.category.is_none());
    assert!(recorded
        .evidence_json
        .as_deref()
        .unwrap_or("")
        .contains("resume"));

    let again = store
        .record_idle_gap_candidate("resume", start, end)
        .expect("duplicate candidate");
    assert!(again.is_none());
}

#[test]
fn idle_gap_candidate_is_suppressed_when_paused_or_already_classified() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let start = test_today_noon_ms() - 45 * 60_000;
    let end = start + 20 * 60_000;

    store.pause("manual pause").expect("pause");
    assert!(store
        .record_idle_gap_candidate("hid-return", start, end)
        .expect("paused candidate")
        .is_none());

    store.resume().expect("resume");
    store
        .upsert_idle_block(IdleBlockInput {
            id: Some("classified-away".into()),
            started_at: start,
            ended_at: end,
            category: Some("Meeting".into()),
            classified: Some(true),
            evidence_json: None,
        })
        .expect("classified block");

    assert!(store
        .record_idle_gap_candidate("hid-return", start + 60_000, end - 60_000)
        .expect("covered candidate")
        .is_none());
}

#[test]
fn search_indexes_weekly_calendar_focus_ai_and_offline_sources() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let start = test_today_noon_ms() - 60 * 60_000;
    let end = start + 30 * 60_000;

    store
        .upsert_calendar_event(CalendarEventInput {
            id: Some("cal-roadmap".into()),
            source: Some("manual".into()),
            external_id: None,
            calendar_name: Some("Product".into()),
            title: "Roadmap sync".into(),
            starts_at: start,
            ends_at: end,
            location: Some("Zoom".into()),
            status: Some("confirmed".into()),
            planned_work_type: Some("meeting".into()),
        })
        .expect("calendar");
    store
        .upsert_focus_session(FocusSessionInput {
            id: Some("focus-roadmap".into()),
            goal: "Roadmap digest draft".into(),
            client: None,
            project: Some("DayTrail".into()),
            task: Some("Roadmap digest".into()),
            ticket_id: None,
            target_ms: 25 * 60_000,
            started_at: start,
            ended_at: Some(end),
            status: Some("completed".into()),
        })
        .expect("focus");
    store
        .upsert_meeting(MeetingInput {
            id: Some("meeting-roadmap".into()),
            title: "Roadmap follow-up".into(),
            starts_at: Some(start),
            ends_at: Some(end),
            attendees_json: None,
            summary: Some("Calendar launch plan".into()),
            actions_json: None,
        })
        .expect("meeting");
    store
        .record_work_output(WorkOutputInput {
            id: Some("output-roadmap".into()),
            output_type: "weekly_update".into(),
            title: "Roadmap weekly update".into(),
            source: Some("AI weekly auto-draft".into()),
            ai_assisted: Some(true),
            status: Some("drafted".into()),
            evidence_json: None,
        })
        .expect("output");
    store.generate_weekly_review().expect("weekly");

    let results = store.search_work_memory("roadmap", 20).expect("search");
    let types = results
        .iter()
        .map(|result| result.entity_type.as_str())
        .collect::<std::collections::HashSet<_>>();

    assert!(types.contains("calendar_event"));
    assert!(types.contains("focus_session"));
    assert!(types.contains("meeting"));
    assert!(types.contains("work_output"));
    assert!(types.contains("weekly_review"));
}

impl KeychainAdapter for TestKeychain {
    fn keychain_get(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self.values.lock().expect("lock").get(key).cloned())
    }

    fn keychain_set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.values
            .lock()
            .expect("lock")
            .insert(key.to_string(), value.to_string());
        Ok(())
    }
}

#[test]
fn stores_tasks_notes_settings_and_exports_them() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let task = store
        .create_task(TaskInput {
            title: "Prepare daily summary".into(),
            due_date: Some("2026-05-23".into()),
            due_at: Some(1_716_454_800_000),
            notes: Some("Include blockers and follow-ups".into()),
            priority: Some("high".into()),
            source: Some("test".into()),
            project_path: Some("/repo/worktrace".into()),
            client_label: Some("Internal".into()),
            project_label: Some("DayTrail".into()),
        })
        .expect("create task");
    assert_eq!(task.status, TaskStatus::Open);
    assert_eq!(
        task.notes.as_deref(),
        Some("Include blockers and follow-ups")
    );
    assert_eq!(task.priority.as_deref(), Some("high"));
    assert_eq!(task.due_at, Some(1_716_454_800_000));

    let note = store
        .add_quick_note(
            "Follow up on browser bridge",
            Some("extension"),
            Some("/repo/worktrace"),
        )
        .expect("quick note");
    assert_eq!(note.body, "Follow up on browser bridge");

    let commitment = store
        .create_commitment(CommitmentInput {
            id: Some("commitment-1".into()),
            title: "Send Oval MoM by EOD".into(),
            source: Some("email".into()),
            owner: Some("me".into()),
            due_at: Some(1_800_000),
            confidence: Some(0.93),
            evidence_json: Some(r#"["mail-thread-1"]"#.into()),
        })
        .expect("commitment");
    assert_eq!(commitment.status, "open");

    let thread = store
        .upsert_email_thread(EmailThreadInput {
            id: "mail-thread-1".into(),
            subject: "Oval MoM confirmation".into(),
            latest_sender: Some("client@example.com".into()),
            latest_at: Some(1_700_000),
            pending_reply: true,
            evidence_json: Some(r#"["latest sender is not you"]"#.into()),
        })
        .expect("email thread");
    assert!(thread.pending_reply);

    store
        .update_settings(SettingsPatch {
            idle_timeout_minutes: Some(12),
            export_format: Some("json".into()),
            browser_bridge_enabled: Some(true),
            terminal_bridge_path: Some(dir.path().join("terminal.json").display().to_string()),
            excluded_apps: None,
            excluded_domains: None,
            excluded_projects: None,
            ..SettingsPatch::default()
        })
        .expect("settings");
    store.pause("lunch").expect("pause");

    let today = store.today_snapshot().expect("today");
    assert_eq!(today.tasks.len(), 1);
    assert_eq!(today.tasks[0].project_label.as_deref(), Some("DayTrail"));
    assert_eq!(today.quick_notes.len(), 1);
    assert_eq!(today.commitments.len(), 1);
    assert_eq!(today.pending_replies.len(), 1);
    assert_eq!(
        today
            .next_best_action
            .as_ref()
            .map(|action| action.source_type.as_str()),
        Some("email_thread")
    );
    assert!(today.pause_state.paused);
    assert_eq!(today.settings.idle_timeout_minutes, 12);

    store.complete_task(task.id).expect("complete task");
    let tasks = store.list_tasks(None).expect("list tasks");
    assert_eq!(tasks[0].status, TaskStatus::Done);

    let export = store.export_data().expect("export");
    assert_eq!(export.tasks.len(), 1);
    assert_eq!(
        export.tasks[0].notes.as_deref(),
        Some("Include blockers and follow-ups")
    );
    assert_eq!(export.quick_notes[0].id, note.id);
    assert_eq!(export.commitments[0].id, "commitment-1");
    assert_eq!(export.pending_replies[0].id, "mail-thread-1");
    assert_eq!(export.settings.export_format, "json");

    let report = store.generate_daily_report().expect("daily report");
    assert_eq!(report.report_type, "daily");
    assert!(report.title.contains("Daily Work Execution Report"));
    assert!(report.body_markdown.contains("Prepare daily summary"));
    assert!(report.body_markdown.contains("Follow up on browser bridge"));
    assert!(report.body_markdown.contains("Send Oval MoM by EOD"));
    assert!(report.body_markdown.contains("Oval MoM confirmation"));
}

#[test]
fn drafts_bulk_tasks_from_pasted_text_without_due_dates() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let drafts = store
        .draft_tasks_from_text(
            "HER Health LIS Integration\n\n- Implementation issues\n2. NOVA Path kind LIS Integration",
            Some("high".into()),
        )
        .expect("draft tasks");

    assert_eq!(drafts.len(), 3);
    assert_eq!(drafts[0].title, "HER Health LIS Integration");
    assert_eq!(drafts[1].title, "Implementation issues");
    assert_eq!(drafts[2].title, "NOVA Path kind LIS Integration");
    assert!(drafts
        .iter()
        .all(|draft| draft.priority.as_deref() == Some("high")));
    assert!(drafts.iter().all(|draft| draft.due_date.is_none()));
    assert!(drafts.iter().all(|draft| draft.due_at.is_none()));
}

#[test]
fn manages_tasks_and_reminders_lifecycle() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let task = store
        .create_task(TaskInput {
            title: "Renew test certificate".into(),
            due_date: None,
            due_at: Some(1_800_000),
            notes: Some("Check staging first".into()),
            priority: Some("high".into()),
            source: Some("manual".into()),
            project_path: None,
            client_label: Some("Ops".into()),
            project_label: Some("Infrastructure".into()),
        })
        .expect("create task");

    assert_eq!(task.status, TaskStatus::Open);
    assert_eq!(task.due_date.as_deref(), Some("1970-01-01"));
    assert_eq!(task.due_at, Some(1_800_000));
    assert_eq!(task.notes.as_deref(), Some("Check staging first"));
    assert_eq!(task.priority.as_deref(), Some("high"));

    let updated = store
        .update_task(
            task.id,
            TaskInput {
                title: "Renew production certificate".into(),
                due_date: Some("2026-06-05".into()),
                due_at: None,
                notes: Some("Coordinate maintenance window".into()),
                priority: Some("medium".into()),
                source: Some("manual".into()),
                project_path: None,
                client_label: Some("Ops".into()),
                project_label: Some("Security".into()),
            },
        )
        .expect("update task");
    assert_eq!(updated.id, task.id);
    assert_eq!(updated.title, "Renew production certificate");
    assert_eq!(updated.due_date.as_deref(), Some("2026-06-05"));
    assert_eq!(updated.due_at, None);
    assert_eq!(
        updated.notes.as_deref(),
        Some("Coordinate maintenance window")
    );
    assert_eq!(updated.priority.as_deref(), Some("medium"));
    assert_eq!(updated.project_label.as_deref(), Some("Security"));

    let due = store
        .list_due_task_reminders(2_000_000)
        .expect("due reminders");
    assert!(due.is_empty());

    let snoozed_for_reminder = store
        .snooze_task(task.id, 1_800_000)
        .expect("snooze task for reminder");
    assert_eq!(snoozed_for_reminder.due_at, Some(1_800_000));

    let due = store
        .list_due_task_reminders(2_000_000)
        .expect("due reminders");
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, task.id);

    store
        .mark_task_reminder_sent(task.id, 2_000_000)
        .expect("mark reminder sent");
    assert!(store
        .list_due_task_reminders(2_100_000)
        .expect("due reminders after mark")
        .is_empty());

    let snoozed = store.snooze_task(task.id, 3_600_000).expect("snooze task");
    assert_eq!(snoozed.status, TaskStatus::Open);
    assert_eq!(snoozed.due_at, Some(3_600_000));
    assert!(snoozed.reminder_sent_at.is_none());

    let completed = store.complete_task(task.id).expect("complete task");
    assert_eq!(completed.status, TaskStatus::Done);
    assert!(store
        .list_tasks(Some(TaskStatus::Open))
        .expect("open tasks")
        .is_empty());

    let deleted = store.delete_task(task.id).expect("delete task");
    assert_eq!(deleted.deleted_rows, 1);
    assert!(store.list_tasks(None).expect("all tasks").is_empty());
}

#[test]
fn defaults_to_simple_mode_and_persists_display_settings() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let defaults = store.get_settings().expect("default settings");
    assert!(defaults.launch_at_login);
    assert_eq!(defaults.experience_mode, "simple");
    assert!(!defaults.show_system_apps);
    assert!(!defaults.show_raw_events);
    assert!(!defaults.show_capture_confidence);
    assert_eq!(defaults.show_ai_details, "summary");
    assert_eq!(defaults.data_retention_days, 0);
    assert_eq!(defaults.task_retention_days, 0);
    assert!(!defaults.recovery_enabled);
    assert_eq!(defaults.recovery_threshold_minutes, 30);
    assert!(!defaults.premium_notifications_enabled);
    assert_eq!(defaults.notification_sound, "daytrail");

    let updated = store
        .update_settings(SettingsPatch {
            experience_mode: Some("pro".into()),
            show_system_apps: Some(true),
            show_raw_events: Some(true),
            show_capture_confidence: Some(true),
            show_ai_details: Some("detailed".into()),
            data_retention_days: Some(90),
            task_retention_days: Some(180),
            recovery_enabled: Some(true),
            recovery_threshold_minutes: Some(45),
            premium_notifications_enabled: Some(true),
            notification_sound: Some("subtle".into()),
            ..SettingsPatch::default()
        })
        .expect("update display settings");

    assert_eq!(updated.experience_mode, "pro");
    assert!(updated.show_system_apps);
    assert!(updated.show_raw_events);
    assert!(updated.show_capture_confidence);
    assert_eq!(updated.show_ai_details, "detailed");
    assert_eq!(updated.data_retention_days, 90);
    assert_eq!(updated.task_retention_days, 180);
    assert!(updated.recovery_enabled);
    assert_eq!(updated.recovery_threshold_minutes, 45);
    assert!(updated.premium_notifications_enabled);
    assert_eq!(updated.notification_sound, "subtle");

    let normalized = store
        .update_settings(SettingsPatch {
            notification_sound: Some("unknown".into()),
            ..SettingsPatch::default()
        })
        .expect("normalize unknown sound");
    assert_eq!(normalized.notification_sound, "daytrail");
}

#[test]
fn daily_report_is_non_empty_when_work_sessions_exist() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let start = test_today_noon_ms() - 20 * 60_000;
    let end = start + 15 * 60_000;

    store
        .record_source_event(SourceEventInput {
            id: Some("report-vscode".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("VS Code".into()),
            title: Some("DayTrail App.tsx".into()),
            url: None,
            workspace_key: Some("/repo/daytrail".into()),
            started_at: Some(start),
            ended_at: Some(end),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record source event");
    store
        .record_source_event(SourceEventInput {
            id: Some("report-system-settings".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("System Settings".into()),
            title: Some("Accessibility".into()),
            url: None,
            workspace_key: None,
            started_at: Some(end),
            ended_at: Some(end + 60_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record system source event");
    store
        .materialize_work_memory()
        .expect("materialize work session");

    let report = store.generate_daily_report().expect("daily report");

    assert!(report.body_markdown.contains("## Summary"));
    assert!(report.body_markdown.contains("## What happened"));
    assert!(report.body_markdown.contains("## Work sessions"));
    assert!(report.body_markdown.contains("## Apps used"));
    assert!(report.body_markdown.contains("## AI detected"));
    assert!(report.body_markdown.contains("## Needs review"));
    assert!(report.body_markdown.contains("DayTrail"));
    assert!(!report.body_markdown.contains("System Settings"));
}

#[test]
fn stores_ai_settings_with_keychain_reference_only() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let keychain = TestKeychain::default();

    store
        .update_settings(SettingsPatch {
            ai_provider: Some("OpenAI Compatible".into()),
            ai_model: Some("gpt-4.1-mini".into()),
            ai_endpoint: Some("https://llm.example.com/v1/".into()),
            ai_redact_secrets: Some(true),
            full_clipboard_history: Some(false),
            ..SettingsPatch::default()
        })
        .expect("AI settings");
    let settings = store
        .set_ai_api_key_with_keychain("OpenAI Compatible", "sk-test-secret", &keychain)
        .expect("store API key");

    assert_eq!(settings.ai_provider, "OpenAI Compatible");
    assert_eq!(settings.ai_model, "gpt-4.1-mini");
    assert_eq!(settings.ai_endpoint, "https://llm.example.com/v1");
    assert_eq!(
        settings.ai_api_key_ref.as_deref(),
        Some("keychain:ai-provider-openai-compatible")
    );
    assert_eq!(
        keychain
            .keychain_get("ai-provider-openai-compatible")
            .expect("keychain get")
            .as_deref(),
        Some("sk-test-secret")
    );

    let conn = Connection::open(&db_path).expect("open database");
    let raw_settings = conn
        .prepare("SELECT group_concat(value, ' ') FROM settings")
        .expect("prepare settings")
        .query_row([], |row| row.get::<_, String>(0))
        .expect("settings values");
    assert!(!raw_settings.contains("sk-test-secret"));
}

#[test]
fn exports_and_imports_portable_settings_without_keychain_secret() {
    let source_dir = tempdir().expect("source temp dir");
    let source_db = source_dir.path().join("source.sqlite3");
    let source = WorktraceStore::open(&source_db).expect("open source store");
    let keychain = TestKeychain::default();

    source
        .update_settings(SettingsPatch {
            idle_timeout_minutes: Some(7),
            export_format: Some("json".into()),
            browser_bridge_enabled: Some(false),
            terminal_bridge_path: Some("/Users/alice/.daytrail/terminal.json".into()),
            excluded_apps: Some(vec!["Slack".into(), "Messages".into(), "Slack".into()]),
            excluded_domains: Some(vec!["private.example.com".into()]),
            excluded_projects: Some(vec!["/Users/alice/private".into()]),
            ai_provider: Some("OpenAI Compatible".into()),
            ai_model: Some("gpt-4.1-mini".into()),
            ai_endpoint: Some("https://llm.example.com/v1/".into()),
            ai_redact_secrets: Some(true),
            full_clipboard_history: Some(false),
            data_retention_days: Some(30),
            task_retention_days: Some(90),
            recovery_enabled: Some(true),
            recovery_threshold_minutes: Some(60),
            ..SettingsPatch::default()
        })
        .expect("source settings");
    source
        .set_ai_api_key_with_keychain("OpenAI Compatible", "sk-test-secret", &keychain)
        .expect("source API key");

    let config_json = source
        .export_settings_config_json()
        .expect("export settings config");
    assert!(config_json.contains("\"schemaVersion\": 1"));
    assert!(config_json.contains("OpenAI Compatible"));
    assert!(!config_json.contains("sk-test-secret"));
    assert!(!config_json.contains("ai-provider-openai-compatible"));
    assert!(!config_json.contains("aiApiKeyRef"));

    let target_dir = tempdir().expect("target temp dir");
    let target_db = target_dir.path().join("target.sqlite3");
    let target = WorktraceStore::open(&target_db).expect("open target store");
    let imported = target
        .import_settings_config_json(&config_json)
        .expect("import settings config");

    assert_eq!(imported.idle_timeout_minutes, 7);
    assert!(!imported.browser_bridge_enabled);
    assert_eq!(
        imported.terminal_bridge_path.as_deref(),
        Some("/Users/alice/.daytrail/terminal.json")
    );
    assert_eq!(imported.excluded_apps, vec!["messages", "slack"]);
    assert_eq!(imported.excluded_domains, vec!["private.example.com"]);
    assert_eq!(imported.excluded_projects, vec!["/users/alice/private"]);
    assert_eq!(imported.ai_provider, "OpenAI Compatible");
    assert_eq!(imported.ai_model, "gpt-4.1-mini");
    assert_eq!(imported.ai_endpoint, "https://llm.example.com/v1");
    assert!(imported.ai_redact_secrets);
    assert!(!imported.full_clipboard_history);
    assert_eq!(imported.data_retention_days, 30);
    assert_eq!(imported.task_retention_days, 90);
    assert!(imported.recovery_enabled);
    assert_eq!(imported.recovery_threshold_minutes, 60);
    assert_eq!(imported.ai_api_key_ref, None);
}

#[test]
fn backs_up_and_restores_sqlite_database_with_integrity_check() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .create_task(TaskInput {
            title: "Task preserved in backup".into(),
            due_date: None,
            due_at: None,
            notes: None,
            priority: None,
            source: Some("test".into()),
            project_path: Some("/repo/daytrail".into()),
            client_label: None,
            project_label: None,
        })
        .expect("create initial task");

    let backup = store
        .backup_database_to_default()
        .expect("create database backup");
    assert!(fs::metadata(&backup.path).expect("backup metadata").len() > 0);
    assert!(backup.bytes > 0);
    assert!(backup.pre_restore_backup_path.is_none());
    let storage = store.storage_locations().expect("storage locations");
    assert!(storage.database_bytes > 0);
    assert!(storage.backup_bytes >= backup.bytes);
    assert!(storage.total_bytes >= storage.database_bytes + storage.backup_bytes);
    assert_eq!(storage.retention_days, 0);

    store
        .create_task(TaskInput {
            title: "Task created after backup".into(),
            due_date: None,
            due_at: None,
            notes: None,
            priority: None,
            source: Some("test".into()),
            project_path: Some("/repo/daytrail".into()),
            client_label: None,
            project_label: None,
        })
        .expect("create post-backup task");
    assert_eq!(
        store.list_tasks(None).expect("tasks before restore").len(),
        2
    );

    let restore = store
        .restore_database_from_path(&backup.path)
        .expect("restore database from backup");
    assert_eq!(restore.path, backup.path);
    let pre_restore_backup_path = restore
        .pre_restore_backup_path
        .as_deref()
        .expect("pre-restore backup path");
    assert!(
        fs::metadata(pre_restore_backup_path)
            .expect("pre-restore backup metadata")
            .len()
            > 0
    );

    let restored_tasks = store.list_tasks(None).expect("tasks after restore");
    assert_eq!(restored_tasks.len(), 1);
    assert_eq!(restored_tasks[0].title, "Task preserved in backup");

    let conn = Connection::open(&db_path).expect("open restored database");
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("integrity check");
    assert_eq!(integrity, "ok");

    let invalid_path = dir.path().join("not-a-database.sqlite3");
    fs::write(&invalid_path, "not sqlite").expect("write invalid db");
    assert!(store.restore_database_from_path(&invalid_path).is_err());
}

#[test]
fn generates_plans_weekly_reviews_and_searches_work_memory() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    store
        .create_task(TaskInput {
            title: "Close Oval billing validation".into(),
            due_date: Some(today),
            due_at: None,
            notes: None,
            priority: None,
            source: Some("Jira".into()),
            project_path: Some("/repo/billing".into()),
            client_label: None,
            project_label: None,
        })
        .expect("create due task");
    store
        .create_commitment(CommitmentInput {
            id: Some("commitment-plan".into()),
            title: "Send sponsor update before EOD".into(),
            source: Some("meeting".into()),
            owner: Some("me".into()),
            due_at: Some(1),
            confidence: Some(0.9),
            evidence_json: None,
        })
        .expect("create commitment");
    store
        .upsert_email_thread(EmailThreadInput {
            id: "mail-plan".into(),
            subject: "Oval billing follow-up".into(),
            latest_sender: Some("client@example.com".into()),
            latest_at: Some(2),
            pending_reply: true,
            evidence_json: Some(r#"["latest sender is client"]"#.into()),
        })
        .expect("email thread");
    store
        .add_quick_note(
            "The billing validation workaround depends on sponsor confirmation.",
            Some("scratchpad"),
            Some("/repo/billing"),
        )
        .expect("note");

    let morning = store.generate_morning_plan().expect("morning plan");
    assert_eq!(morning.horizon, "today");
    assert!(morning
        .must_close
        .iter()
        .any(|item| item.title.contains("Oval billing")));
    assert!(morning
        .at_risk
        .iter()
        .any(|item| item.title.contains("sponsor update")));
    assert!(morning.body_markdown.contains("Must close"));

    let weekly = store.generate_weekly_plan().expect("weekly plan");
    assert_eq!(weekly.horizon, "week");
    assert!(weekly.body_markdown.contains("Weekly Plan"));

    let review = store.generate_weekly_review().expect("weekly review");
    assert_eq!(review.report_type, "weekly_review");
    assert!(review.body_markdown.contains("Risks and follow-ups"));

    let results = store
        .search_work_memory("Oval billing", 10)
        .expect("search memory");
    assert!(results
        .iter()
        .any(|result| result.title.contains("Oval billing")));
}

#[test]
fn migrates_core_fact_tables_from_technical_requirements() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let _store = WorktraceStore::open(&db_path).expect("open store");
    let conn = Connection::open(&db_path).expect("open migrated database");

    for table in [
        "source_events",
        "workspace_contexts",
        "work_sessions",
        "parallel_streams",
        "stream_events",
        "scratchpad_notes",
        "state_snapshots",
        "clipboard_events",
        "agent_runs",
        "commitments",
        "email_threads",
        "meetings",
        "field_visits",
        "idle_blocks",
        "recovery_events",
        "ai_usage",
        "outputs",
        "decisions",
        "reports",
        "plans",
        "weekly_reviews",
        "work_memory_fts",
        "projects",
        "people",
        "work_graph_edges",
    ] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("query sqlite schema");
        assert_eq!(count, 1, "missing table {table}");
    }
}

#[test]
fn migrates_legacy_text_id_tasks_table_before_creating_indexes() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    {
        let conn = Connection::open(&db_path).expect("open legacy database");
        conn.execute_batch(
            r#"
            CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT,
                project_id TEXT,
                source_type TEXT,
                source_id TEXT,
                owner TEXT,
                due_at INTEGER,
                priority TEXT,
                status TEXT NOT NULL,
                confidence REAL DEFAULT 0,
                evidence_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            INSERT INTO tasks (
                id, title, project_id, source_type, due_at, status, created_at, updated_at
            )
            VALUES (
                'legacy-task-1',
                'Legacy follow-up',
                'project-alpha',
                'email',
                1800000000000,
                'open',
                1700000000000,
                1700000000000
            );
            "#,
        )
        .expect("seed legacy tasks table");
    }

    let store = WorktraceStore::open(&db_path).expect("migrate legacy store");
    let tasks = store.list_tasks(None).expect("list migrated tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].title, "Legacy follow-up");
    assert_eq!(tasks[0].due_date.as_deref(), Some("2027-01-15"));
    assert_eq!(tasks[0].source.as_deref(), Some("email"));
    assert_eq!(tasks[0].project_path.as_deref(), Some("project-alpha"));

    let created = store
        .create_task(TaskInput {
            title: "New task after migration".into(),
            due_date: None,
            due_at: None,
            notes: None,
            priority: None,
            source: None,
            project_path: None,
            client_label: None,
            project_label: None,
        })
        .expect("create task after migration");
    assert!(created.id > tasks[0].id);
}

#[test]
fn migrates_legacy_capture_tables_used_by_startup_indexes() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    {
        let conn = Connection::open(&db_path).expect("open legacy database");
        conn.execute_batch(
            r#"
            CREATE TABLE source_events (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                event_type TEXT NOT NULL,
                app TEXT,
                title TEXT,
                domain TEXT,
                url_redacted TEXT,
                started_at INTEGER NOT NULL,
                ended_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                sensitivity TEXT DEFAULT 'normal',
                metadata_json TEXT,
                created_at INTEGER NOT NULL
            );
            INSERT INTO source_events (
                id, source, event_type, app, title, domain, url_redacted,
                started_at, ended_at, duration_ms, created_at
            )
            VALUES (
                'source-1', 'browser-extension', 'browser_tab', 'Safari', 'Inbox',
                'example.com', 'https://example.com', 10, 20, 10, 20
            );

            CREATE TABLE quick_notes (
                id TEXT PRIMARY KEY,
                note TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                related_session_id TEXT,
                related_task_id TEXT
            );
            INSERT INTO quick_notes (id, note, created_at)
            VALUES ('note-1', 'Legacy note body', 1700000000000);

            CREATE TABLE email_threads (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                provider_thread_id TEXT NOT NULL,
                participants_json TEXT,
                subject TEXT,
                latest_sender TEXT,
                latest_received_at INTEGER,
                reply_required INTEGER DEFAULT 0,
                reply_status TEXT,
                priority TEXT,
                snippet TEXT,
                linked_tasks_json TEXT,
                metadata_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            INSERT INTO email_threads (
                id, provider, provider_thread_id, subject, latest_sender,
                latest_received_at, reply_required, created_at, updated_at
            )
            VALUES (
                'thread-1', 'gmail', 'provider-thread-1', 'Legacy email',
                'client@example.com', 1700000000000, 1, 1700000000000, 1700000000000
            );

            CREATE TABLE meetings (
                id TEXT PRIMARY KEY,
                provider TEXT,
                provider_event_id TEXT,
                title TEXT,
                meeting_type TEXT,
                started_at INTEGER NOT NULL,
                ended_at INTEGER NOT NULL,
                attendees_json TEXT,
                summary TEXT,
                decisions_json TEXT,
                action_items_json TEXT,
                linked_tasks_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            INSERT INTO meetings (
                id, provider, provider_event_id, title, started_at, ended_at,
                attendees_json, summary, action_items_json, created_at, updated_at
            )
            VALUES (
                'meeting-1', 'calendar', 'event-1', 'Legacy meeting',
                1700000000000, 1700003600000, '[]', 'Legacy summary', '[]',
                1700000000000, 1700000000000
            );

            CREATE TABLE ai_usage (
                id TEXT PRIMARY KEY,
                tool TEXT NOT NULL,
                usage_category TEXT,
                started_at INTEGER NOT NULL,
                ended_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                related_session_id TEXT,
                related_task_id TEXT,
                related_output_id TEXT,
                project_id TEXT,
                confidence REAL DEFAULT 0,
                summary TEXT,
                metadata_json TEXT,
                created_at INTEGER NOT NULL
            );
            INSERT INTO ai_usage (
                id, tool, usage_category, started_at, ended_at, duration_ms,
                project_id, summary, created_at
            )
            VALUES (
                'legacy-ai-1', 'ChatGPT', 'browser', 1700000000000,
                1700000060000, 60000, 'legacy-project', 'Drafted summary',
                1700000000000
            );
            "#,
        )
        .expect("seed legacy capture tables");
    }

    let store = WorktraceStore::open(&db_path).expect("migrate legacy capture tables");
    let export = store.export_data().expect("export migrated data");
    assert_eq!(export.quick_notes[0].body, "Legacy note body");
    assert_eq!(export.pending_replies[0].id, "thread-1");
    let ai_usage = store.list_ai_usage(10).expect("list migrated ai usage");
    assert_eq!(ai_usage[0].tool_name.as_deref(), Some("ChatGPT"));
    assert_eq!(ai_usage[0].context_id.as_deref(), Some("legacy-project"));
    assert_eq!(
        ai_usage[0].prompt_summary.as_deref(),
        Some("Drafted summary")
    );
    let today = store.today_snapshot().expect("legacy today snapshot");
    assert_eq!(today.meetings[0].title, "Legacy meeting");
    let report = store
        .generate_daily_report()
        .expect("generate report with legacy reports table");
    assert!(report.title.contains("Daily Work Execution Report"));
    assert!(report.body_markdown.contains("Daily Work Report"));

    let conn = Connection::open(&db_path).expect("open migrated database");
    let workspace_key_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('source_events') WHERE name = 'workspace_key'",
            [],
            |row| row.get(0),
        )
        .expect("query source_events columns");
    assert_eq!(workspace_key_count, 1);
    let ai_provider_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('ai_usage') WHERE name = 'provider'",
            [],
            |row| row.get(0),
        )
        .expect("query ai_usage provider column");
    assert_eq!(ai_provider_count, 1);
}

#[test]
fn ingests_browser_events_without_persisting_raw_url_queries() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .ingest_browser_event(BrowserBridgeEvent {
            url: Some("https://chatgpt.com/c/abc?token=secret#frag".into()),
            title: Some("ChatGPT - Regex solution".into()),
            source: Some("browser-extension".into()),
            captured_at: Some("2026-05-23T08:00:00Z".into()),
            tab_id: Some(7),
            window_id: Some(2),
            incognito: Some(false),
        })
        .expect("ingest browser event");

    let conn = Connection::open(&db_path).expect("open database");
    let (domain, source_url, source_metadata): (String, String, String) = conn
        .query_row(
            "SELECT domain, url_redacted, metadata_json FROM source_events LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("source event");
    assert_eq!(domain, "chatgpt.com");
    assert_eq!(source_url, "https://chatgpt.com/c/abc");
    assert!(!source_metadata.contains("token=secret"));

    let (activity_url, activity_metadata): (String, String) = conn
        .query_row(
            "SELECT url, metadata_json FROM activity_events LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("activity event");
    assert_eq!(activity_url, "https://chatgpt.com/c/abc");
    assert!(!activity_metadata.contains("token=secret"));
}

#[test]
fn redacts_general_browser_paths_to_domain_origin() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .ingest_browser_event(BrowserBridgeEvent {
            url: Some("https://example.com/client/private/case-123?token=secret#frag".into()),
            title: Some("Client case lookup".into()),
            source: Some("browser-extension".into()),
            captured_at: Some("2026-05-23T08:00:00Z".into()),
            tab_id: Some(7),
            window_id: Some(2),
            incognito: Some(false),
        })
        .expect("ingest browser event");

    let conn = Connection::open(&db_path).expect("open database");
    let (domain, source_url, source_metadata): (String, String, String) = conn
        .query_row(
            "SELECT domain, url_redacted, metadata_json FROM source_events LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("source event");

    assert_eq!(domain, "example.com");
    assert_eq!(source_url, "https://example.com");
    assert!(!source_metadata.contains("case-123"));
    assert!(!source_metadata.contains("token=secret"));

    conn.execute(
        r#"
        UPDATE source_events
        SET url_redacted = 'https://example.com/client/private/case-123?token=secret',
            metadata_json = '{"url":"https://example.com/client/private/case-123?token=secret"}'
        "#,
        [],
    )
    .expect("seed legacy source url");
    conn.execute(
        r#"
        UPDATE activity_events
        SET url = 'https://example.com/client/private/case-123?token=secret',
            metadata_json = '{"url":"https://example.com/client/private/case-123?token=secret"}'
        "#,
        [],
    )
    .expect("seed legacy activity url");
    drop(conn);
    drop(store);

    let _store = WorktraceStore::open(&db_path).expect("reopen store and migrate redactions");
    let conn = Connection::open(&db_path).expect("open migrated database");
    let (source_url, source_metadata, activity_url, activity_metadata): (
        String,
        String,
        String,
        String,
    ) = conn
        .query_row(
            r#"
            SELECT source_events.url_redacted,
                   source_events.metadata_json,
                   activity_events.url,
                   activity_events.metadata_json
            FROM source_events
            CROSS JOIN activity_events
            LIMIT 1
            "#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("migrated urls");

    assert_eq!(source_url, "https://example.com");
    assert_eq!(activity_url, "https://example.com");
    assert!(!source_metadata.contains("case-123"));
    assert!(!activity_metadata.contains("case-123"));
}

#[test]
fn privacy_settings_block_excluded_capture_sources() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .update_settings(SettingsPatch {
            idle_timeout_minutes: None,
            export_format: None,
            browser_bridge_enabled: None,
            terminal_bridge_path: None,
            excluded_apps: Some(vec!["Slack".into()]),
            excluded_domains: Some(vec!["chatgpt.com".into()]),
            excluded_projects: None,
            ..SettingsPatch::default()
        })
        .expect("privacy settings");

    store
        .record_activity(
            "active_window",
            Some("Slack"),
            Some("Private channel"),
            None,
            None,
            None,
        )
        .expect("excluded app should be ignored");
    store
        .ingest_browser_event(BrowserBridgeEvent {
            url: Some("https://chatgpt.com/c/private?token=secret".into()),
            title: Some("ChatGPT private thread".into()),
            source: Some("browser-extension".into()),
            captured_at: None,
            tab_id: None,
            window_id: None,
            incognito: Some(false),
        })
        .expect("excluded domain should be ignored");
    store
        .record_active_window_context(
            "Google Chrome",
            Some("ChatGPT private thread"),
            Some("https://chatgpt.com/c/active-window?token=secret"),
            Some("chatgpt.com"),
            None,
            Some(Duration::from_secs(2)),
        )
        .expect("excluded active-window domain should be ignored");

    let conn = Connection::open(&db_path).expect("open database");
    let activity_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_events", [], |row| row.get(0))
        .expect("activity count");
    let source_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM source_events", [], |row| row.get(0))
        .expect("source count");
    assert_eq!(activity_count, 0);
    assert_eq!(source_count, 0);

    store.pause("privacy").expect("pause tracking");
    store
        .record_activity(
            "active_window",
            Some("Code"),
            Some("Allowed but paused"),
            None,
            None,
            None,
        )
        .expect("paused tracking should ignore activity");
    let activity_count_after_pause: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_events", [], |row| row.get(0))
        .expect("activity count after pause");
    assert_eq!(activity_count_after_pause, 0);
}

#[test]
fn privacy_admin_controls_delete_contexts_clipboard_and_captured_data() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .update_settings(SettingsPatch {
            idle_timeout_minutes: None,
            export_format: None,
            browser_bridge_enabled: None,
            terminal_bridge_path: None,
            excluded_apps: None,
            excluded_domains: None,
            excluded_projects: Some(vec!["/private/repo".into()]),
            ..SettingsPatch::default()
        })
        .expect("privacy settings");
    store
        .record_source_event(SourceEventInput {
            id: Some("excluded-project-event".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Private project".into()),
            url: None,
            workspace_key: Some("/private/repo/app".into()),
            started_at: Some(0),
            ended_at: Some(1),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("excluded project should not fail");

    let conn = Connection::open(&db_path).expect("open database");
    let source_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM source_events", [], |row| row.get(0))
        .expect("source count");
    assert_eq!(source_count, 0);
    store
        .record_active_window_context(
            "Code",
            Some("Private project"),
            None,
            Some("/private/repo/app"),
            None,
            None,
        )
        .expect("excluded active window should not fail");
    let activity_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_events", [], |row| row.get(0))
        .expect("activity count");
    assert_eq!(activity_count, 0);

    store
        .record_source_event(SourceEventInput {
            id: Some("context-event".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Allowed project".into()),
            url: None,
            workspace_key: Some("/repo/worktrace".into()),
            started_at: Some(10),
            ended_at: Some(20),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("allowed source event");
    let deleted = store
        .delete_context_data("context-repo-worktrace")
        .expect("delete context data");
    assert!(deleted.deleted_rows >= 2);

    conn.execute(
        r#"
        INSERT INTO clipboard_events
            (id, content_hash, content_type, size_bytes, created_at)
        VALUES ('clip-1', 'hash', 'text/plain', 4, 1)
        "#,
        [],
    )
    .expect("seed clipboard");
    assert_eq!(
        store
            .clear_clipboard_history()
            .expect("clear clipboard")
            .deleted_rows,
        1
    );

    store
        .create_task(TaskInput {
            title: "Temporary task".into(),
            due_date: None,
            due_at: None,
            notes: None,
            priority: None,
            source: None,
            project_path: None,
            client_label: None,
            project_label: None,
        })
        .expect("task");
    assert!(
        store
            .purge_captured_data()
            .expect("purge captured data")
            .deleted_rows
            >= 1
    );
    let task_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .expect("task count");
    assert_eq!(task_count, 0);
}

#[test]
fn retention_policy_deletes_old_captured_data_and_keeps_recent_data() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = chrono::Utc::now().timestamp_millis();
    let old_start = now - 45 * 24 * 60 * 60 * 1000;
    let recent_start = now - 2 * 24 * 60 * 60 * 1000;

    store
        .record_source_event(SourceEventInput {
            id: Some("old-event".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Old file".into()),
            url: None,
            workspace_key: Some("/repo/old".into()),
            started_at: Some(old_start),
            ended_at: Some(old_start + 60_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("old source event");
    store
        .record_source_event(SourceEventInput {
            id: Some("recent-event".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Recent file".into()),
            url: None,
            workspace_key: Some("/repo/recent".into()),
            started_at: Some(recent_start),
            ended_at: Some(recent_start + 60_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("recent source event");

    store
        .update_settings(SettingsPatch {
            data_retention_days: Some(30),
            ..SettingsPatch::default()
        })
        .expect("retention settings");
    let pruned = store.apply_retention_policy().expect("apply retention");
    assert!(pruned.deleted_rows >= 1);

    let conn = Connection::open(&db_path).expect("open database");
    let remaining_events: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT id FROM source_events ORDER BY id")
            .expect("prepare source event query");
        stmt.query_map([], |row| row.get::<_, String>(0))
            .expect("query source events")
            .map(|row| row.expect("source event row"))
            .collect()
    };
    assert_eq!(remaining_events, vec!["recent-event"]);
    assert_eq!(
        store.get_settings().expect("settings").data_retention_days,
        30
    );
}

#[test]
fn active_window_capture_writes_canonical_source_events() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .record_active_window(
            "Code",
            Some("worktrace-ai - native_messaging.rs"),
            Some(r#"{"source":"test"}"#),
            Some(std::time::Duration::from_secs(30)),
        )
        .expect("record active window");

    let conn = Connection::open(&db_path).expect("open database");
    let (source, event_type, app, title): (String, String, String, String) = conn
        .query_row(
            "SELECT source, event_type, app, title FROM source_events LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("source event");
    assert_eq!(source, "active-window");
    assert_eq!(event_type, "active_window");
    assert_eq!(app, "Code");
    assert_eq!(title, "worktrace-ai - native_messaging.rs");

    let activity_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_events", [], |row| row.get(0))
        .expect("activity count");
    assert_eq!(activity_count, 1);
}

#[test]
fn active_window_capture_persists_detected_workspace_and_url_context() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .record_active_window_context(
            "WebStorm",
            Some("billing.ts - payments-api"),
            None,
            Some("/Users/alice/work/payments-api"),
            Some(r#"{"source":"test"}"#),
            Some(std::time::Duration::from_secs(30)),
        )
        .expect("record active window with workspace");
    store
        .record_active_window_context(
            "Google Chrome",
            Some("ChatGPT - MongoDB index clue"),
            Some("https://chatgpt.com/c/abc?token=secret#frag"),
            None,
            None,
            Some(std::time::Duration::from_secs(30)),
        )
        .expect("record active browser window");

    let conn = Connection::open(&db_path).expect("open database");
    let workspace_key: String = conn
        .query_row(
            "SELECT workspace_key FROM source_events WHERE app = 'WebStorm'",
            [],
            |row| row.get(0),
        )
        .expect("workspace key");
    assert_eq!(workspace_key, "/Users/alice/work/payments-api");

    let (domain, url): (String, String) = conn
        .query_row(
            "SELECT domain, url_redacted FROM source_events WHERE app = 'Google Chrome'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("browser source event");
    assert_eq!(domain, "chatgpt.com");
    assert_eq!(url, "https://chatgpt.com/c/abc");

    let project_path: String = conn
        .query_row(
            "SELECT project_path FROM activity_events WHERE source = 'WebStorm'",
            [],
            |row| row.get(0),
        )
        .expect("activity project path");
    assert_eq!(project_path, "/Users/alice/work/payments-api");
}

#[test]
fn ingests_editor_bridge_jsonl_file_without_native_messaging_configuration() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let bridge_path = dir.path().join("editor-bridge.jsonl");
    let store = WorktraceStore::open(&db_path).expect("open store");

    fs::write(
        &bridge_path,
        serde_json::to_string(&json!({
            "type": "worktrace.editor_context_batch",
            "schemaVersion": 1,
            "source": "vscode-extension",
            "capturedAt": "2026-05-23T08:00:01.000Z",
            "events": [
                {
                    "type": "worktrace.editor_context",
                    "schemaVersion": 1,
                    "source": "vscode-extension",
                    "capturedAt": "2026-05-23T08:00:00.000Z",
                    "eventType": "active_editor_changed",
                    "app": "NetBeans",
                    "workspace": {
                        "name": "billing",
                        "folders": ["/Users/alice/work/billing"]
                    },
                    "document": {
                        "uri": "file:///Users/alice/work/billing/src/Invoice.java",
                        "filePath": "/Users/alice/work/billing/src/Invoice.java",
                        "fileName": "Invoice.java",
                        "languageId": "java",
                        "contentCaptured": false
                    },
                    "sensitivity": "normal",
                    "metadata": {}
                }
            ]
        }))
        .expect("serialize bridge line"),
    )
    .expect("write bridge file");

    let stored = store
        .ingest_editor_bridge_file(&bridge_path)
        .expect("ingest editor bridge file");
    assert_eq!(stored, 1);

    let conn = Connection::open(&db_path).expect("open database");
    let (app, workspace_key): (String, String) = conn
        .query_row(
            "SELECT app, workspace_key FROM source_events LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("source event");
    assert_eq!(app, "NetBeans");
    assert_eq!(workspace_key, "/Users/alice/work/billing");
}

#[test]
fn terminal_bridge_metadata_records_cli_folder_context() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .ingest_terminal_bridge_metadata(TerminalBridgeMetadata {
            cwd: "/Users/alice/work/payments-api".into(),
            shell: Some("/bin/zsh".into()),
            terminal: Some("WarpTerminal".into()),
            updated_at: Some("2026-05-23T09:10:00Z".into()),
            event_type: None,
            last_command: None,
            git_branch: None,
            git_repo: None,
        })
        .expect("ingest terminal metadata");

    let conn = Connection::open(&db_path).expect("open database");
    let (source, app, workspace_key): (String, String, String) = conn
        .query_row(
            "SELECT source, app, workspace_key FROM source_events LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("source event");
    assert_eq!(source, "terminal-bridge");
    assert_eq!(app, "Warp");
    assert_eq!(workspace_key, "/Users/alice/work/payments-api");

    let folder_path: String = conn
        .query_row(
            "SELECT folder_path FROM workspace_contexts WHERE context_key = '/Users/alice/work/payments-api'",
            [],
            |row| row.get(0),
        )
        .expect("workspace context");
    assert_eq!(folder_path, "/Users/alice/work/payments-api");
}

#[test]
fn terminal_bridge_metadata_does_not_store_terminal_capability_as_app() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .ingest_terminal_bridge_metadata(TerminalBridgeMetadata {
            cwd: "/Users/alice/work/daytrail".into(),
            shell: Some("/bin/zsh".into()),
            terminal: Some("dumb".into()),
            updated_at: Some("2026-05-23T09:10:00Z".into()),
            event_type: Some("command".into()),
            last_command: Some("printf daytrail qa --api-key secret".into()),
            git_branch: None,
            git_repo: None,
        })
        .expect("ingest terminal metadata");

    let conn = Connection::open(&db_path).expect("open database");
    let (app, title, metadata_json): (String, String, String) = conn
        .query_row(
            "SELECT app, title, metadata_json FROM source_events LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("source event");

    assert_eq!(app, "Terminal");
    assert_eq!(title, "printf daytrail qa --api-key [redacted]");
    assert!(metadata_json.contains("\"terminal\":\"Terminal\""));
    assert!(!metadata_json.contains("\"terminal\":\"dumb\""));
}

#[test]
fn coalesces_adjacent_source_events_without_creating_false_idle_blocks() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let first = store
        .record_source_event(SourceEventInput {
            id: None,
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Implement watcher coalescing".into()),
            url: None,
            workspace_key: Some("/repo/worktrace".into()),
            started_at: Some(0),
            ended_at: Some(1_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("first source event");
    let second = store
        .record_source_event(SourceEventInput {
            id: None,
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Implement watcher coalescing".into()),
            url: None,
            workspace_key: Some("/repo/worktrace".into()),
            started_at: Some(2_000),
            ended_at: Some(3_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("second source event");
    assert_eq!(first.id, second.id);
    assert_eq!(second.duration_ms, 3_000);

    store
        .record_source_event(SourceEventInput {
            id: Some("after-idle".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Resume after lunch".into()),
            url: None,
            workspace_key: Some("/repo/worktrace".into()),
            started_at: Some(11 * 60_000 + 3_000),
            ended_at: Some(11 * 60_000 + 4_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("after idle source event");

    let conn = Connection::open(&db_path).expect("open database");
    let source_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM source_events", [], |row| row.get(0))
        .expect("source count");
    let idle_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM idle_blocks", [], |row| row.get(0))
        .expect("idle count");
    assert_eq!(source_count, 2);
    assert_eq!(idle_count, 0);
}

#[test]
fn native_messaging_accepts_browser_payloads_through_redacted_ingestion() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let mut input = Vec::new();
    native_messaging::write_message(
        &mut input,
        &json!({
            "type": "worktrace.browser_tab",
            "schemaVersion": 1,
            "source": "native-test",
            "capturedAt": "2026-05-23T08:00:00Z",
            "title": "Claude - debugging",
            "url": "https://claude.ai/chat/abc?token=secret#frag",
            "tabId": 11,
            "windowId": 3,
            "incognito": false
        }),
    )
    .expect("write native message");

    let mut output = Vec::new();
    native_messaging::run_with_store_io(&store, Cursor::new(input), &mut output)
        .expect("run native host loop");
    let response = native_messaging::read_message(&mut Cursor::new(output))
        .expect("read response")
        .expect("response message");
    assert_eq!(response["ok"], true);
    assert_eq!(response["stored"], true);

    let conn = Connection::open(&db_path).expect("open database");
    let (domain, url, started_at): (String, String, i64) = conn
        .query_row(
            "SELECT domain, url_redacted, started_at FROM source_events LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("source event");
    assert_eq!(domain, "claude.ai");
    assert_eq!(url, "https://claude.ai/chat/abc");
    assert_eq!(
        started_at,
        chrono::DateTime::parse_from_rfc3339("2026-05-23T08:00:00Z")
            .expect("parse capturedAt")
            .timestamp_millis()
    );
}

#[test]
fn native_messaging_rejects_unknown_messages_and_ignores_incognito_tabs() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let unknown = native_messaging::handle_message(&store, json!({"type": "unknown"}));
    assert_eq!(unknown["ok"], false);

    let incognito = native_messaging::handle_message(
        &store,
        json!({
            "type": "worktrace.browser_tab",
            "schemaVersion": 1,
            "source": "native-test",
            "title": "Private tab",
            "url": "https://example.com/private?secret=1",
            "incognito": true
        }),
    );
    assert_eq!(incognito["ok"], true);
    assert_eq!(incognito["stored"], false);

    let conn = Connection::open(&db_path).expect("open database");
    let source_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM source_events", [], |row| row.get(0))
        .expect("source count");
    assert_eq!(source_count, 0);
}

#[test]
fn native_messaging_rejects_bad_schema_and_skips_unsupported_urls() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let missing_schema = native_messaging::handle_message(
        &store,
        json!({
            "type": "worktrace.browser_tab",
            "title": "Missing schema",
            "url": "https://example.com"
        }),
    );
    assert_eq!(missing_schema["ok"], false);

    let unsupported = native_messaging::handle_message(
        &store,
        json!({
            "type": "worktrace.browser_tab",
            "schemaVersion": 1,
            "title": "Chrome settings",
            "url": "chrome://settings"
        }),
    );
    assert_eq!(unsupported["ok"], true);
    assert_eq!(unsupported["stored"], false);
    assert_eq!(unsupported["ignoredReason"], "unsupported_url_scheme");

    let conn = Connection::open(&db_path).expect("open database");
    let source_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM source_events", [], |row| row.get(0))
        .expect("source count");
    assert_eq!(source_count, 0);
}

#[test]
fn native_messaging_accepts_batched_browser_payloads() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let response = native_messaging::handle_message(
        &store,
        json!({
            "type": "worktrace.browser_tab_batch",
            "schemaVersion": 1,
            "events": [
                {
                    "source": "batch-test",
                    "title": "GitHub PR",
                    "url": "https://github.com/org/repo/pull/1",
                    "incognito": false
                },
                {
                    "source": "batch-test",
                    "title": "Browser settings",
                    "url": "chrome://settings",
                    "incognito": false
                }
            ]
        }),
    );

    assert_eq!(response["ok"], true);
    assert_eq!(response["stored"], 1);
    assert_eq!(response["ignored"], 1);

    let conn = Connection::open(&db_path).expect("open database");
    let source_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM source_events", [], |row| row.get(0))
        .expect("source count");
    assert_eq!(source_count, 1);
}

#[test]
fn native_messaging_accepts_editor_context_batches_for_return_markers() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let response = native_messaging::handle_message(
        &store,
        json!({
            "type": "worktrace.editor_context_batch",
            "schemaVersion": 1,
            "source": "vscode-extension",
            "capturedAt": "2026-05-23T08:00:01.000Z",
            "events": [
                {
                    "type": "worktrace.editor_context",
                    "schemaVersion": 1,
                    "source": "vscode-extension",
                    "capturedAt": "2026-05-23T08:00:00.000Z",
                    "eventType": "active_editor_changed",
                    "app": "Cursor",
                    "workspace": {
                        "name": "payments-api",
                        "folders": ["/Users/alice/work/payments-api"]
                    },
                    "document": {
                        "uri": "file:///Users/alice/work/payments-api/src/billing.ts?token=secret#frag",
                        "filePath": "/Users/alice/work/payments-api/src/billing.ts",
                        "fileName": "billing.ts",
                        "languageId": "typescript",
                        "cursor": {"line": 42, "character": 7},
                        "contentCaptured": false
                    },
                    "sensitivity": "normal",
                    "metadata": {
                        "note": "sponsor validation clue"
                    }
                }
            ]
        }),
    );

    assert_eq!(response["ok"], true);
    assert_eq!(response["stored"], 1);

    let conn = Connection::open(&db_path).expect("open database");
    let (source, event_type, app, title, url, metadata): (
        String,
        String,
        String,
        String,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT source, event_type, app, title, url_redacted, metadata_json FROM source_events LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("source event");
    assert_eq!(source, "vscode-extension");
    assert_eq!(event_type, "editor_context");
    assert_eq!(app, "Cursor");
    assert_eq!(title, "billing.ts");
    assert_eq!(url, "file:///Users/alice/work/payments-api/src/billing.ts");
    assert!(!metadata.contains("token=secret"));

    let (snapshot_type, active_file, cursor): (String, String, String) = conn
        .query_row(
            "SELECT snapshot_type, active_file, cursor_position FROM state_snapshots LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("state snapshot");
    assert_eq!(snapshot_type, "active_editor");
    assert_eq!(active_file, "/Users/alice/work/payments-api/src/billing.ts");
    assert!(cursor.contains("\"line\":42"));
}

#[test]
fn tracks_ai_usage_outputs_today_report_and_next_action() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .ingest_browser_event(BrowserBridgeEvent {
            url: Some("https://chatgpt.com/c/thread-1?token=secret".into()),
            title: Some("ChatGPT - client MoM draft".into()),
            source: Some("browser-extension".into()),
            captured_at: Some("2026-05-23T09:00:00Z".into()),
            tab_id: Some(3),
            window_id: Some(1),
            incognito: Some(false),
        })
        .expect("AI browser event");
    let usage = store.list_ai_usage(10).expect("AI usage");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].tool_name.as_deref(), Some("ChatGPT"));

    let output = store
        .record_work_output(WorkOutputInput {
            id: Some("output-1".into()),
            output_type: "email_draft".into(),
            title: "Client MoM draft from ChatGPT".into(),
            source: Some("chatgpt".into()),
            ai_assisted: Some(true),
            status: Some("drafted".into()),
            evidence_json: Some(r#"["ai-source-event"]"#.into()),
        })
        .expect("record output");
    assert!(output.ai_assisted);

    let today = store.today_snapshot().expect("today");
    assert_eq!(today.ai_outputs.len(), 1);
    assert_eq!(
        today
            .next_best_action
            .as_ref()
            .map(|action| action.source_type.as_str()),
        Some("output")
    );

    let report = store.generate_daily_report().expect("daily report");
    assert!(report
        .body_markdown
        .contains("Client MoM draft from ChatGPT"));
    let export = store.export_data().expect("export");
    assert_eq!(export.outputs.len(), 1);
}

#[test]
fn detects_unclosed_loops_ghost_agents_and_stale_hypotheses() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .record_work_output(WorkOutputInput {
            id: Some("draft-output".into()),
            output_type: "client_reply".into(),
            title: "AG Diagnostics feasibility reply".into(),
            source: Some("ChatGPT".into()),
            ai_assisted: Some(true),
            status: Some("drafted".into()),
            evidence_json: Some(r#"["chatgpt-thread"]"#.into()),
        })
        .expect("draft output");
    store
        .record_agent_run(AgentRunInput {
            id: Some("agent-failed".into()),
            context_id: Some("context-backend".into()),
            tool_name: Some("Claude Code".into()),
            command_label: Some("backend refactor".into()),
            started_at: Some(1_000),
            ended_at: Some(14_000),
            status: Some("failed".into()),
            exit_code: Some(1),
            summary: Some("compiler error".into()),
            error_tail: Some("type mismatch".into()),
            notified: Some(false),
            metadata_json: Some(r#"["cargo test"]"#.into()),
        })
        .expect("agent run");
    store
        .create_state_snapshot(StateSnapshotInput {
            id: Some("snapshot-stale".into()),
            context_id: "context-backend".into(),
            trigger_type: "context_switch".into(),
            snapshot_type: "interruption".into(),
            summary: Some("Temporary hypothesis: commented out auth validation".into()),
            terminal_tail: None,
            git_diff_summary: Some("commented out auth validation while testing CORS".into()),
            active_file: Some("src/auth.ts".into()),
            cursor_position: None,
            ai_context_summary: None,
            metadata_json: None,
        })
        .expect("state snapshot");

    let risks = store.detect_loop_risks().expect("loop risks");
    assert!(risks.iter().any(|risk| risk.risk_type == "ai_output_open"));
    assert!(risks.iter().any(|risk| risk.risk_type == "ghost_agent"));
    assert!(risks
        .iter()
        .any(|risk| risk.risk_type == "stale_hypothesis"));

    let today = store.today_snapshot().expect("today");
    assert!(today
        .loop_risks
        .iter()
        .any(|risk| risk.risk_type == "ghost_agent"));
    assert!(today
        .unclosed_loop_inbox
        .iter()
        .any(|item| item.category == "Agent" && item.title.contains("backend refactor")));
    assert!(today
        .unclosed_loop_inbox
        .iter()
        .any(|item| item.category == "AI Output"
            && item.title.contains("AG Diagnostics feasibility reply")));
    let report = store.generate_daily_report().expect("daily report");
    assert!(report.body_markdown.contains("Unclosed loops"));
    assert!(report.body_markdown.contains("backend refactor"));
}

#[test]
fn tracks_meetings_field_visits_and_idle_recovery_in_today_and_reports() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();

    store
        .upsert_meeting(MeetingInput {
            id: Some("meeting-1".into()),
            title: "Client escalation sync".into(),
            starts_at: Some(1_000),
            ends_at: Some(2_000),
            attendees_json: Some(r#"["client@example.com"]"#.into()),
            summary: Some("Confirmed rollout blocker".into()),
            actions_json: Some(r#"["Share revised ETA"]"#.into()),
        })
        .expect("meeting");
    store
        .upsert_field_visit(FieldVisitInput {
            id: Some("visit-1".into()),
            client_label: Some("Oval Foods".into()),
            starts_at: Some(3_000),
            ends_at: Some(6_000),
            location_label: Some("Warehouse".into()),
            debrief: Some("Verified inventory handoff process".into()),
            status: Some("completed".into()),
        })
        .expect("field visit");
    store
        .upsert_idle_block(IdleBlockInput {
            id: Some("idle-1".into()),
            started_at: now - 187_000,
            ended_at: now - 7_000,
            category: None,
            classified: Some(false),
            evidence_json: Some(r#"["away from laptop"]"#.into()),
        })
        .expect("idle block");

    let today = store.today_snapshot().expect("today");
    assert_eq!(today.meetings.len(), 1);
    assert_eq!(today.field_visits.len(), 1);
    assert_eq!(today.idle_blocks.len(), 1);
    assert!(store
        .delete_idle_block("idle-1")
        .expect("delete idle block"));
    assert_eq!(
        store
            .today_snapshot()
            .expect("today after delete")
            .idle_blocks
            .len(),
        0
    );
    store
        .upsert_idle_block(IdleBlockInput {
            id: Some("idle-1".into()),
            started_at: now - 187_000,
            ended_at: now - 7_000,
            category: None,
            classified: Some(false),
            evidence_json: Some(r#"["away from laptop"]"#.into()),
        })
        .expect("restore idle block");
    assert_eq!(
        today
            .next_best_action
            .as_ref()
            .map(|action| action.source_type.as_str()),
        Some("idle_block")
    );

    let report = store.generate_daily_report().expect("daily report");
    assert!(report.body_markdown.contains("Client escalation sync"));
    assert!(report.body_markdown.contains("Oval Foods"));
    assert!(report.body_markdown.contains("minutes need classification"));
}

#[test]
fn materializes_source_events_into_sessions_streams_and_graph_edges() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();

    for event in [
        SourceEventInput {
            id: Some("evt-code-1".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Fix pool timeout - main.rs".into()),
            url: None,
            workspace_key: Some("/repo/backend".into()),
            started_at: Some(now),
            ended_at: Some(now + 1_000),
            sensitivity: None,
            metadata_json: None,
        },
        SourceEventInput {
            id: Some("evt-ai-1".into()),
            source: "browser-extension".into(),
            event_type: "browser_tab".into(),
            app: Some("Chrome".into()),
            title: Some("Claude - pool timeout".into()),
            url: Some("https://claude.ai/chat/123?secret=raw".into()),
            workspace_key: None,
            started_at: Some(now + 1_200),
            ended_at: Some(now + 2_000),
            sensitivity: None,
            metadata_json: None,
        },
        SourceEventInput {
            id: Some("evt-code-2".into()),
            source: "terminal".into(),
            event_type: "terminal".into(),
            app: Some("iTerm2".into()),
            title: Some("npm test timeout output".into()),
            url: None,
            workspace_key: Some("/repo/backend".into()),
            started_at: Some(now + 3_000),
            ended_at: Some(now + 4_000),
            sensitivity: None,
            metadata_json: None,
        },
    ] {
        store
            .record_source_event(event)
            .expect("record source event");
    }

    let summary = store
        .materialize_work_memory()
        .expect("materialize work memory");
    assert_eq!(summary.source_events, 3);
    assert_eq!(summary.work_sessions, 1);
    assert_eq!(summary.parallel_streams, 2);
    assert_eq!(summary.graph_edges, 3);

    let today = store.today_snapshot().expect("today snapshot");
    assert_eq!(today.work_sessions.len(), 1);
    assert!(today.work_sessions[0].ai_used);
    assert_eq!(today.work_sessions[0].evidence_event_ids.len(), 3);
    assert_eq!(today.parallel_streams.len(), 2);

    let conn = Connection::open(&db_path).expect("open database");
    let redacted_url: String = conn
        .query_row(
            "SELECT url_redacted FROM source_events WHERE id = 'evt-ai-1'",
            [],
            |row| row.get(0),
        )
        .expect("redacted source url");
    assert_eq!(redacted_url, "https://claude.ai/chat/123");
}

#[test]
fn sessionizer_splits_long_back_to_back_project_contexts() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = chrono::Local::now()
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid local noon")
        .and_local_timezone(chrono::Local)
        .earliest()
        .expect("local noon timestamp")
        .timestamp_millis();

    for event in [
        SourceEventInput {
            id: Some("project-a".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Auth.tsx".into()),
            url: None,
            workspace_key: Some("/repo/project-a".into()),
            started_at: Some(now),
            ended_at: Some(now + 16 * 60_000),
            sensitivity: None,
            metadata_json: None,
        },
        SourceEventInput {
            id: Some("project-b".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Billing.tsx".into()),
            url: None,
            workspace_key: Some("/repo/project-b".into()),
            started_at: Some(now + 16 * 60_000 + 1_000),
            ended_at: Some(now + 25 * 60_000),
            sensitivity: None,
            metadata_json: None,
        },
    ] {
        store
            .record_source_event(event)
            .expect("record source event");
    }

    let summary = store
        .materialize_work_memory()
        .expect("materialize work memory");
    assert_eq!(summary.work_sessions, 2);

    let today = store.today_snapshot().expect("today snapshot");
    let summaries = today
        .work_sessions
        .iter()
        .filter_map(|session| session.summary.as_deref())
        .collect::<Vec<_>>();
    assert!(summaries
        .iter()
        .any(|summary| summary.contains("project-a")));
    assert!(summaries
        .iter()
        .any(|summary| summary.contains("project-b")));
}

#[test]
fn today_snapshot_falls_back_to_recent_source_events_when_sessions_are_not_persisted() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();

    store
        .record_source_event(SourceEventInput {
            id: Some("raw-window-event".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Google Chrome".into()),
            title: Some("ChatGPT - WorkTrace capture".into()),
            url: Some("https://chatgpt.com/c/thread?token=secret".into()),
            workspace_key: Some("chatgpt.com".into()),
            started_at: Some(now - 5_000),
            ended_at: Some(now),
            sensitivity: Some("normal".into()),
            metadata_json: None,
        })
        .expect("source event");

    let snapshot = store.today_snapshot().expect("today snapshot");
    assert_eq!(snapshot.work_sessions.len(), 1);
    assert_eq!(snapshot.parallel_streams.len(), 1);
    assert_eq!(
        snapshot.work_sessions[0].title,
        "ChatGPT - WorkTrace capture"
    );
    assert_eq!(
        snapshot.parallel_streams[0].title,
        "ChatGPT - WorkTrace capture"
    );
}

#[test]
fn today_snapshot_does_not_persist_derived_sessions_on_read() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("daytrail.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();

    store
        .record_source_event(SourceEventInput {
            id: Some("raw-window-event".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Google Chrome".into()),
            title: Some("ChatGPT - DayTrail capture".into()),
            url: Some("https://chatgpt.com/c/thread?token=secret".into()),
            workspace_key: Some("chatgpt.com".into()),
            started_at: Some(now - 5_000),
            ended_at: Some(now),
            sensitivity: Some("normal".into()),
            metadata_json: None,
        })
        .expect("source event");

    let snapshot = store.today_snapshot().expect("today snapshot");
    assert_eq!(snapshot.work_sessions.len(), 1);
    assert_eq!(snapshot.parallel_streams.len(), 1);

    let conn = Connection::open(&db_path).expect("open database");
    let persisted_sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM work_sessions", [], |row| row.get(0))
        .expect("session count");
    let persisted_streams: i64 = conn
        .query_row("SELECT COUNT(*) FROM parallel_streams", [], |row| {
            row.get(0)
        })
        .expect("stream count");
    let persisted_stream_events: i64 = conn
        .query_row("SELECT COUNT(*) FROM stream_events", [], |row| row.get(0))
        .expect("stream event count");
    let persisted_graph_edges: i64 = conn
        .query_row("SELECT COUNT(*) FROM work_graph_edges", [], |row| {
            row.get(0)
        })
        .expect("graph edge count");
    assert_eq!(persisted_sessions, 0);
    assert_eq!(persisted_streams, 0);
    assert_eq!(persisted_stream_events, 0);
    assert_eq!(persisted_graph_edges, 0);
}

#[test]
fn materialize_work_memory_skips_unchanged_source_events_without_rewriting_rows() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("daytrail.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();

    for event in [
        SourceEventInput {
            id: Some("evt-code".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Ledger.tsx".into()),
            url: None,
            workspace_key: Some("/repo/daytrail".into()),
            started_at: Some(now - 15_000),
            ended_at: Some(now - 8_000),
            sensitivity: None,
            metadata_json: None,
        },
        SourceEventInput {
            id: Some("evt-ai".into()),
            source: "active-window".into(),
            event_type: "browser_tab".into(),
            app: Some("Google Chrome".into()),
            title: Some("ChatGPT - ledger review".into()),
            url: Some("https://chatgpt.com/c/thread?token=secret".into()),
            workspace_key: Some("chatgpt.com".into()),
            started_at: Some(now - 7_000),
            ended_at: Some(now),
            sensitivity: None,
            metadata_json: None,
        },
    ] {
        store
            .record_source_event(event)
            .expect("record source event");
    }

    let first = store.materialize_work_memory().expect("first materialize");
    assert_eq!(first.source_events, 2);
    assert_eq!(first.work_sessions, 1);

    let conn = Connection::open(&db_path).expect("open database");
    let first_session_updated_at: i64 = conn
        .query_row("SELECT MAX(updated_at) FROM work_sessions", [], |row| {
            row.get(0)
        })
        .expect("session updated_at");
    let first_edge_created_at: i64 = conn
        .query_row(
            "SELECT MAX(created_at) FROM work_graph_edges WHERE relation = 'session_contains_event'",
            [],
            |row| row.get(0),
        )
        .expect("edge created_at");
    drop(conn);

    thread::sleep(Duration::from_millis(10));

    let second = store.materialize_work_memory().expect("second materialize");
    assert_eq!(second.source_events, first.source_events);
    assert_eq!(second.work_sessions, first.work_sessions);
    assert_eq!(second.parallel_streams, first.parallel_streams);
    assert_eq!(second.graph_edges, first.graph_edges);

    let conn = Connection::open(&db_path).expect("open database");
    let second_session_updated_at: i64 = conn
        .query_row("SELECT MAX(updated_at) FROM work_sessions", [], |row| {
            row.get(0)
        })
        .expect("session updated_at");
    let second_edge_created_at: i64 = conn
        .query_row(
            "SELECT MAX(created_at) FROM work_graph_edges WHERE relation = 'session_contains_event'",
            [],
            |row| row.get(0),
        )
        .expect("edge created_at");

    assert_eq!(second_session_updated_at, first_session_updated_at);
    assert_eq!(second_edge_created_at, first_edge_created_at);
}

#[test]
fn materialized_capture_titles_use_workspace_and_app_labels_not_untitled() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();

    store
        .update_settings(SettingsPatch {
            idle_timeout_minutes: Some(1),
            ..SettingsPatch::default()
        })
        .expect("settings");

    store
        .record_source_event(SourceEventInput {
            id: Some("evt_window_1_GoogleChrome".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("GoogleChrome".into()),
            title: None,
            url: None,
            workspace_key: None,
            started_at: Some(now - 140_000),
            ended_at: Some(now - 139_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record app-only event");
    store
        .record_source_event(SourceEventInput {
            id: Some("evt-terminal-workspace".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("/bin/zsh".into()),
            title: Some("/Users/example/Desktop/Work tracker".into()),
            url: None,
            workspace_key: Some("/Users/example/Desktop/Work tracker".into()),
            started_at: Some(now - 10_000),
            ended_at: Some(now - 8_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record workspace event");

    store
        .materialize_work_memory()
        .expect("materialize work memory");
    let today = store.today_snapshot().expect("today snapshot");
    let titles = today
        .work_sessions
        .iter()
        .map(|session| session.title.as_str())
        .collect::<Vec<_>>();

    assert!(titles.contains(&"Work tracker"));
    assert!(titles.contains(&"Google Chrome"));
    assert!(!titles.contains(&"Untitled work session"));
    assert!(today
        .parallel_streams
        .iter()
        .any(|stream| stream.title == "Work tracker"));
}

#[test]
fn today_snapshot_exposes_session_evidence_ai_usage_and_automation_candidates() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = test_today_noon_ms();

    for (index, title) in [
        "billing-api daily export",
        "billing-api ledger check",
        "billing-api follow-up report",
    ]
    .iter()
    .enumerate()
    {
        store
            .record_source_event(SourceEventInput {
                id: Some(format!("routine-{index}")),
                source: "active-window".into(),
                event_type: "active_window".into(),
                app: Some("Code".into()),
                title: Some((*title).into()),
                url: None,
                workspace_key: Some("/Users/alice/work/billing-api".into()),
                started_at: Some(now - 300_000 + (index as i64 * 20_000)),
                ended_at: Some(now - 290_000 + (index as i64 * 20_000)),
                sensitivity: None,
                metadata_json: None,
            })
            .expect("routine event");
    }

    store
        .record_source_event(SourceEventInput {
            id: Some("ai-chatgpt".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Google Chrome".into()),
            title: Some("ChatGPT - draft client reply".into()),
            url: Some("https://chatgpt.com/c/thread?token=secret".into()),
            workspace_key: Some("chatgpt.com".into()),
            started_at: Some(now - 120_000),
            ended_at: Some(now - 60_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("ai browser event");
    store
        .record_source_event(SourceEventInput {
            id: Some("ai-copilot".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Code".into()),
            title: Some("Copilot generated tests".into()),
            url: None,
            workspace_key: Some("/Users/alice/work/billing-api".into()),
            started_at: Some(now - 50_000),
            ended_at: Some(now),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("ai editor event");

    let today = store.today_snapshot().expect("today snapshot");

    assert!(today
        .source_events
        .iter()
        .any(|event| event.id == "ai-chatgpt"
            && event.url_redacted.as_deref() == Some("https://chatgpt.com/c/thread")));
    assert!(today.ai_usage_summary.total_duration_ms >= 110_000);
    assert!(today
        .ai_usage_summary
        .tools
        .iter()
        .any(|tool| tool.tool == "ChatGPT" && tool.duration_ms >= 60_000));
    assert!(today
        .ai_usage_summary
        .tools
        .iter()
        .any(|tool| tool.tool == "Copilot" && tool.duration_ms >= 50_000));
    assert!(today.automation_candidates.iter().any(|candidate| {
        candidate.title == "billing-api"
            && candidate.occurrences >= 4
            && !candidate.suggested_steps.is_empty()
    }));
    assert!(today
        .capture_health
        .checks
        .iter()
        .any(|check| check.id == "active-window" && check.status == "ok"));
    assert!(today
        .capture_health
        .checks
        .iter()
        .any(|check| check.id == "ai-tools" && check.status == "ok"));
    assert!(today
        .ai_output_ledger
        .iter()
        .any(|item| item.tool == "ChatGPT" && item.source_context == "chatgpt.com"));
    assert_eq!(today.menu_bar_summary.capture_state, "Capturing");
    assert!(
        today.menu_bar_summary.current_work.contains("VS Code")
            || today.menu_bar_summary.current_work.contains("billing-api")
            || today.menu_bar_summary.current_work.contains("ChatGPT")
    );
    assert!(today.app_usage_summary.apps.iter().any(|app| {
        app.app == "VS Code"
            && app.category == "work"
            && app.projects.iter().any(|project| {
                project.label == "billing-api"
                    && project.ai_tools.iter().any(|tool| tool.tool == "Copilot")
            })
    }));
    assert!(today.app_usage_summary.apps.iter().any(|app| {
        app.app == "Google Chrome"
            && app.category == "browser"
            && app.projects.iter().any(|project| {
                project.label == "chatgpt.com"
                    && project.ai_tools.iter().any(|tool| tool.tool == "ChatGPT")
            })
    }));

    let local_date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let export = store
        .export_data_range(ExportRangeInput {
            from_date: Some(local_date.clone()),
            to_date: Some(local_date),
        })
        .expect("date range export");
    assert!(export.source_events.len() >= 5);
    assert!(export
        .ai_usage_summary
        .tools
        .iter()
        .any(|tool| tool.tool == "ChatGPT"));
    assert!(export
        .automation_candidates
        .iter()
        .any(|candidate| candidate.title == "billing-api"));

    let daily_report = store.generate_daily_report().expect("daily report");
    assert!(daily_report.body_markdown.contains("## What happened"));
    assert!(daily_report.body_markdown.contains("## Work sessions"));
    assert!(daily_report.body_markdown.contains("## Apps used"));
    assert!(daily_report.body_markdown.contains("## AI detected"));
    assert!(daily_report.body_markdown.contains("billing-api"));
    assert!(daily_report.body_markdown.contains("Google Chrome"));
    assert!(daily_report.body_markdown.contains("ChatGPT"));
    assert!(daily_report.body_markdown.contains("Copilot"));

    let analysis = store
        .analyze_export_range(ExportRangeInput::default())
        .expect("export analysis");
    assert_eq!(analysis.report_type, "automation_analysis");
    assert!(analysis.body_markdown.contains("Automation"));
}

#[test]
fn separates_multiple_vscode_projects_and_editor_ai_signals() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");
    let now = chrono::Utc::now().timestamp_millis();

    store
        .record_source_event(SourceEventInput {
            id: Some("vscode-cfm-codex".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Code".into()),
            title: Some("deploy-production.yml — CFM-main — Untracked".into()),
            url: None,
            workspace_key: Some("/Users/alice/work/CFM-main".into()),
            started_at: Some(now - 120_000),
            ended_at: Some(now - 60_000),
            sensitivity: None,
            metadata_json: Some(
                r#"{"aiTools":["Codex","Copilot"],"workspaceCandidates":["/Users/alice/work/LMS-production","/Users/alice/work/CFM-main"]}"#
                    .into(),
            ),
        })
        .expect("record CFM VS Code event");
    store
        .record_source_event(SourceEventInput {
            id: Some("vscode-lms".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Code".into()),
            title: Some("App.tsx — LMS-production".into()),
            url: None,
            workspace_key: Some("/Users/alice/work/LMS-production".into()),
            started_at: Some(now - 50_000),
            ended_at: Some(now),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record LMS VS Code event");

    let today = store.today_snapshot().expect("today snapshot");
    let vscode = today
        .app_usage_summary
        .apps
        .iter()
        .find(|app| app.app == "VS Code")
        .expect("VS Code app usage");
    assert!(vscode
        .projects
        .iter()
        .any(|project| project.label == "CFM-main"));
    assert!(vscode
        .projects
        .iter()
        .any(|project| project.label == "LMS-production"));
    let cfm = vscode
        .projects
        .iter()
        .find(|project| project.label == "CFM-main")
        .expect("CFM project");
    assert!(cfm.ai_tools.iter().any(|tool| tool.tool == "Codex"));
    assert!(cfm.ai_tools.iter().any(|tool| tool.tool == "Copilot"));
    assert!(today
        .ai_usage_summary
        .tools
        .iter()
        .any(|tool| tool.tool == "Codex" && tool.duration_ms >= 60_000));
    assert!(today
        .ai_usage_summary
        .tools
        .iter()
        .any(|tool| tool.tool == "Copilot" && tool.duration_ms >= 60_000));
}

#[test]
fn detects_terminal_ai_usage_and_marks_terminal_health_ok_when_signal_exists() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .ingest_terminal_bridge_metadata(TerminalBridgeMetadata {
            cwd: "/Users/alice/work/infra".into(),
            shell: Some("zsh".into()),
            terminal: Some("Warp".into()),
            updated_at: Some(chrono::Utc::now().to_rfc3339()),
            event_type: Some("command".into()),
            last_command: Some("gemini --yolo".into()),
            git_branch: None,
            git_repo: None,
        })
        .expect("terminal bridge metadata");

    let today = store.today_snapshot().expect("today snapshot");
    let terminal_health = today
        .capture_health
        .checks
        .iter()
        .find(|check| check.id == "terminal-bridge")
        .expect("terminal health");
    assert_eq!(terminal_health.status, "ok");
    assert!(terminal_health.detail.contains("gemini"));
    assert!(today
        .ai_usage_summary
        .tools
        .iter()
        .any(|tool| tool.tool == "Gemini"));
    assert!(today.app_usage_summary.apps.iter().any(|app| {
        app.app == "Terminal"
            && app
                .projects
                .iter()
                .any(|project| project.ai_tools.iter().any(|tool| tool.tool == "Gemini"))
    }));
}

#[test]
fn deletes_saved_quick_note_memory_facts() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let note = store
        .add_quick_note(
            "need to plan for kt",
            Some("scratchpad"),
            Some("/repo/worktrace"),
        )
        .expect("quick note");

    let deleted = store.delete_quick_note(note.id).expect("delete note");

    assert_eq!(deleted.deleted_rows, 1);
    assert!(store
        .list_quick_notes(10)
        .expect("notes")
        .iter()
        .all(|item| item.id != note.id));
}

#[test]
fn builds_context_scratchpad_snapshots_and_return_marker() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    store
        .record_source_event(SourceEventInput {
            id: Some("evt-context-1".into()),
            source: "active-window".into(),
            event_type: "editor".into(),
            app: Some("Code".into()),
            title: Some("Fix native bridge".into()),
            url: None,
            workspace_key: Some("/repo/worktrace".into()),
            started_at: Some(10_000),
            ended_at: Some(20_000),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("record source event");
    store
        .materialize_work_memory()
        .expect("materialize work memory");

    let context_id = "context-repo-worktrace";
    let note = store
        .add_scratchpad_note(ScratchpadNoteInput {
            id: Some("note-1".into()),
            context_id: context_id.into(),
            note: "Verify Edge native host path before release".into(),
            pinned: Some(true),
        })
        .expect("scratchpad note");
    assert!(note.pinned);

    let snapshot = store
        .create_state_snapshot(StateSnapshotInput {
            id: Some("snapshot-1".into()),
            context_id: context_id.into(),
            trigger_type: "context_switch".into(),
            snapshot_type: "interrupted_state".into(),
            summary: Some("Stopped while validating native browser bridge".into()),
            terminal_tail: Some("cargo test --all-targets passed".into()),
            git_diff_summary: Some("native messaging and source events touched".into()),
            active_file: Some("src/native_messaging.rs".into()),
            cursor_position: Some("line 90".into()),
            ai_context_summary: None,
            metadata_json: Some(r#"{"safe":true}"#.into()),
        })
        .expect("state snapshot");
    assert_eq!(snapshot.context_id, context_id);

    let marker = store.get_return_marker(context_id).expect("return marker");
    assert_eq!(
        marker
            .context
            .as_ref()
            .map(|context| context.context_key.as_str()),
        Some("/repo/worktrace")
    );
    assert_eq!(
        marker.latest_snapshot.as_ref().map(|item| item.id.as_str()),
        Some("snapshot-1")
    );
    assert_eq!(marker.pinned_notes.len(), 1);
    assert_eq!(marker.recent_sessions.len(), 1);
    assert!(marker
        .suggested_next_action
        .as_deref()
        .unwrap_or_default()
        .contains("Stopped while validating native browser bridge"));
}

#[test]
fn detects_project_from_workspace_storage_and_terminal_metadata() {
    let dir = tempdir().expect("temp dir");
    let storage = dir.path().join("Code/User/workspaceStorage/hash");
    fs::create_dir_all(&storage).expect("workspace storage");
    fs::write(
        storage.join("workspace.json"),
        r#"{"folder":"file:///Users/example/src/worktrace"}"#,
    )
    .expect("workspace json");

    let terminal = dir.path().join("terminal-bridge.json");
    fs::write(
        &terminal,
        serde_json::to_string(&TerminalBridgeMetadata {
            cwd: "/Users/example/src/terminal-project".into(),
            shell: Some("zsh".into()),
            terminal: Some("iTerm2".into()),
            updated_at: Some("2026-05-23T08:00:00Z".into()),
            event_type: None,
            last_command: None,
            git_branch: None,
            git_repo: None,
        })
        .expect("terminal metadata"),
    )
    .expect("terminal json");

    let detected = detect_project_from_sources(ProjectDetectionSources {
        workspace_storage_roots: vec![dir.path().join("Code/User/workspaceStorage")],
        terminal_bridge_metadata_paths: vec![terminal],
    })
    .expect("detect project");

    assert_eq!(detected.path, "/Users/example/src/terminal-project");
    assert_eq!(detected.source, "terminal-bridge");
    assert_eq!(detected.editor_hint.as_deref(), Some("Code"));
}

#[test]
fn detects_most_recent_editor_workspace_storage_when_multiple_projects_are_open() {
    let dir = tempdir().expect("temp dir");
    let root = dir.path().join("Code/User/workspaceStorage");
    let stale = root.join("stale");
    let recent = root.join("recent");
    let stale_project = dir.path().join("LMS-production");
    let recent_project = dir.path().join("CFM-main");
    fs::create_dir_all(&stale).expect("stale workspace storage");
    fs::create_dir_all(&recent).expect("recent workspace storage");
    fs::create_dir_all(&stale_project).expect("stale project folder");
    fs::create_dir_all(&recent_project).expect("recent project folder");
    let stale_url = url::Url::from_file_path(&stale_project).expect("stale file url");
    let recent_url = url::Url::from_file_path(&recent_project).expect("recent file url");
    fs::write(
        stale.join("workspace.json"),
        json!({ "folder": stale_url.as_str() }).to_string(),
    )
    .expect("stale workspace json");
    fs::write(stale.join("state.vscdb"), "old").expect("stale state");
    thread::sleep(Duration::from_millis(25));
    fs::write(
        recent.join("workspace.json"),
        json!({ "folder": recent_url.as_str() }).to_string(),
    )
    .expect("recent workspace json");
    fs::write(recent.join("state.vscdb"), "new").expect("recent state");

    let detected = detect_project_from_sources(ProjectDetectionSources {
        workspace_storage_roots: vec![root],
        terminal_bridge_metadata_paths: vec![],
    })
    .expect("detect recent editor project");

    assert_eq!(detected.path, recent_project.display().to_string());
    assert_eq!(detected.source, "workspace-storage");
}

#[test]
fn today_snapshot_shows_a_new_days_capture_and_excludes_prior_days() {
    // Guards the original "nothing recorded on a new day" report at the query
    // layer: a capture stamped on the current local day must surface in today's
    // snapshot, and a capture from a previous local day must not leak in or mask
    // it. A timezone/day-boundary regression here would hide a fresh day's data.
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("worktrace.sqlite3");
    let store = WorktraceStore::open(&db_path).expect("open store");

    let now = chrono::Local::now().timestamp_millis();
    let prior_day = now - 30 * 60 * 60 * 1000; // 30h ago is always a previous local day

    store
        .record_source_event(SourceEventInput {
            id: Some("today-event".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Code".into()),
            title: Some("Today work".into()),
            url: None,
            workspace_key: Some("/repo/today".into()),
            started_at: Some(now - 2_000),
            ended_at: Some(now),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("today event");
    store
        .record_source_event(SourceEventInput {
            id: Some("prior-day-event".into()),
            source: "active-window".into(),
            event_type: "active_window".into(),
            app: Some("Code".into()),
            title: Some("Yesterday work".into()),
            url: None,
            workspace_key: Some("/repo/yesterday".into()),
            started_at: Some(prior_day - 2_000),
            ended_at: Some(prior_day),
            sensitivity: None,
            metadata_json: None,
        })
        .expect("prior day event");

    let today = store.today_snapshot().expect("today snapshot");

    assert!(
        today
            .source_events
            .iter()
            .any(|event| event.id == "today-event"),
        "a capture stamped today must appear in the new-day snapshot"
    );
    assert!(
        today
            .source_events
            .iter()
            .all(|event| event.id != "prior-day-event"),
        "a capture from a previous day must not appear in today's snapshot"
    );
    assert_eq!(
        today.local_date,
        chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string()
    );
}
