use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, SecondsFormat, TimeZone, Utc};
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, DatabaseName,
    OptionalExtension, Row,
};
use serde_json::Value;
use tauri::Manager;

const SOURCE_EVENT_COALESCE_GAP_MS: i64 = 2 * 60 * 1000;
const RECOVERY_STREAK_RESET_GAP_MS: i64 = 3 * 60 * 1000;
const RECOVERY_SUGGESTED_BREAK_MINUTES: i64 = 3;
/// How long the capture watcher can go without a heartbeat tick before it is
/// considered stalled. The watcher ticks every 2s, but macOS can briefly delay
/// tray-resident background work; helper-process timeouts catch real hangs while
/// this avoids false red alerts during short OS stalls.
const CAPTURE_STALE_AFTER_MS: i64 = 120_000;
const DISPLAY_APP_NAME: &str = "DayTrail";
const DATA_DIR_NAME: &str = "ai.daytrail.desktop";
const DB_FILE_NAME: &str = "daytrail.sqlite3";

use crate::{
    matching::CompiledRule,
    models::{
        ActiveWorkContext, ActiveWorkContextInput, ActivityTaskLink, AgentRun, AgentRunInput,
        AiContextUsage, AiContributionRow, AiOutputLedgerItem, AiToolUsage, AiUsage, AiUsageInput,
        AiUsageSummary, AppProjectUsage, AppUsage, AppUsageSummary, ApplyRulesSummary,
        AutomationCandidate, BrowserBridgeEvent, CalendarEvent, CalendarEventInput,
        CalendarReconciliation, CalendarReconciliationItem, CaptureHealthCheck, CaptureHealthSummary,
        Commitment, CommitmentInput, DatabaseTransferResult, EmailThread, EmailThreadInput,
        ExportPayload, ExportRangeInput, FieldVisit, FieldVisitInput, FileUsage, FocusSessionInput,
        FocusSessionSummary, IdleBlock, IdleBlockInput, InferredWorkBlock, LinkOrigin,
        LinkedActivity, LoopAction, LoopActionInput, LoopRisk, Meeting, MeetingInput, MenuBarSummary,
        NextBestAction, ParallelStreamSummary, PauseState, PlanningItem, PlanningOutput,
        PrivacyDeleteSummary, ProjectContext, QuickNote, RecoveryEvent, RecoveryEventInput,
        RecoveryPrompt, RecoverySummary, ReportOutput, ReturnMarker, ReviewSessionInput,
        ScratchpadNote, ScratchpadNoteInput, SearchResult, Settings, SettingsConfigPayload,
        SettingsPatch, SourceEvent, SourceEventInput, StateSnapshot, StateSnapshotInput,
        StorageLocationInfo, Task, TaskDraft, TaskInput, TaskMatchRule, TaskMatchRuleInput,
        TaskStatus, TerminalBridgeMetadata, TimesheetRow, TodaySnapshot, UnclosedLoopItem,
        WorkMemorySummary, WorkOutput, WorkOutputInput, WorkSessionSummary, WorkspaceContext,
    },
    platform::{
        keychain_key_for_ai_provider, keychain_key_from_ref, set_launch_at_login, KeychainAdapter,
        SystemKeychain,
    },
    project_detection::{default_project_sources, detect_project_from_sources},
    store_materialization::{
        materialization_state_locked, session_graph_edge_count_locked,
        source_event_materialization_fingerprint_locked, upsert_materialization_state_locked,
        work_memory_summary_locked,
    },
};

#[derive(Clone)]
pub struct WorktraceStore {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
    auto_ingest_local_bridges: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatLookback {
    Today,
    Week,
    LastWeek,
    TwoWeeks,
    Month,
    Quarter,
}

fn chat_lookback_for_message(message: &str) -> ChatLookback {
    let msg = message.to_lowercase();
    let wants_history = msg.contains("all time")
        || msg.contains("all data")
        || msg.contains("history")
        || msg.contains("since i started")
        || msg.contains("ever ")
        || msg.contains("from when")
        || msg.contains("since when")
        || msg.contains("how far back")
        || msg.contains("available data")
        || msg.contains("data coverage")
        || msg.contains("earliest data")
        || msg.contains("oldest data");
    if wants_history
        || msg.contains("quarter")
        || msg.contains("90 day")
        || msg.contains("3 month")
    {
        return ChatLookback::Quarter;
    }

    if msg.contains("month")
        || msg.contains("30 day")
        || msg.contains("4 week")
        || msg.contains("this month")
        || msg.contains("last month")
    {
        return ChatLookback::Month;
    }

    if msg.contains("two week")
        || msg.contains("2 week")
        || msg.contains("14 day")
        || msg.contains("fortnight")
    {
        return ChatLookback::TwoWeeks;
    }

    if msg.contains("last week") || msg.contains("previous week") || msg.contains("prior week") {
        return ChatLookback::LastWeek;
    }

    if msg.contains("week")
        || msg.contains("7 day")
        || msg.contains("pattern")
        || msg.contains("trend")
        || msg.contains("average")
        || [
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
        ]
        .iter()
        .any(|d| msg.contains(d))
    {
        return ChatLookback::Week;
    }

    ChatLookback::Today
}

impl WorktraceStore {
    pub fn default_database_path() -> Result<PathBuf> {
        let data_dir = dirs::data_local_dir().context("failed to resolve user data directory")?;
        Ok(data_dir.join(DATA_DIR_NAME).join(DB_FILE_NAME))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_options(path, false)
    }

    fn open_with_options(path: impl AsRef<Path>, auto_ingest_local_bridges: bool) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create sqlite directory {}", parent.display())
            })?;
        }

        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open sqlite database {}", db_path.display()))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
            auto_ingest_local_bridges,
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_default(app: &tauri::AppHandle) -> Result<Self> {
        let app_dir = app
            .path()
            .app_data_dir()
            .context("failed to resolve app data directory")?;
        let db_path = app_dir.join(DB_FILE_NAME);
        Self::open_with_options(db_path, true)
    }

    pub fn open_user_default() -> Result<Self> {
        Self::open_with_options(Self::default_database_path()?, true)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'open',
                due_date TEXT,
                due_at INTEGER,
                notes TEXT,
                priority TEXT,
                source TEXT,
                project_path TEXT,
                client_label TEXT,
                project_label TEXT,
                reminder_sent_at INTEGER,
                completed_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS quick_notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                body TEXT NOT NULL,
                source TEXT,
                project_path TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS bridge_cursors (
                path TEXT PRIMARY KEY,
                bytes_read INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS pause_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                paused INTEGER NOT NULL,
                reason TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS activity_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                source TEXT,
                title TEXT,
                url TEXT,
                project_path TEXT,
                metadata_json TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS source_events (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                event_type TEXT NOT NULL,
                app TEXT,
                title TEXT,
                domain TEXT,
                url_redacted TEXT,
                workspace_key TEXT,
                started_at INTEGER NOT NULL,
                ended_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                sensitivity TEXT NOT NULL DEFAULT 'normal',
                metadata_json TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS materialization_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                source_event_count INTEGER NOT NULL DEFAULT 0,
                max_source_ended_at INTEGER NOT NULL DEFAULT 0,
                max_source_created_at INTEGER NOT NULL DEFAULT 0,
                total_source_duration_ms INTEGER NOT NULL DEFAULT 0,
                source_content_signature INTEGER NOT NULL DEFAULT 0,
                idle_gap_ms INTEGER NOT NULL DEFAULT 300000,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS workspace_contexts (
                id TEXT PRIMARY KEY,
                context_key TEXT NOT NULL UNIQUE,
                context_type TEXT NOT NULL,
                label TEXT,
                git_repo TEXT,
                git_branch TEXT,
                folder_path TEXT,
                domain TEXT,
                email_thread_id TEXT,
                project_id TEXT,
                last_seen_at INTEGER,
                metadata_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS work_sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                project_id TEXT,
                context_id TEXT,
                category TEXT,
                status TEXT,
                started_at INTEGER NOT NULL,
                ended_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                ai_used INTEGER NOT NULL DEFAULT 0,
                confidence REAL NOT NULL DEFAULT 0,
                summary TEXT,
                evidence_json TEXT,
                user_corrected INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS parallel_streams (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                stream_type TEXT,
                project_id TEXT,
                context_id TEXT,
                started_at INTEGER NOT NULL,
                ended_at INTEGER,
                summary TEXT,
                confidence REAL NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS stream_events (
                stream_id TEXT NOT NULL,
                event_id TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 0,
                PRIMARY KEY (stream_id, event_id)
            );

            CREATE TABLE IF NOT EXISTS scratchpad_notes (
                id TEXT PRIMARY KEY,
                context_id TEXT NOT NULL,
                note TEXT NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS state_snapshots (
                id TEXT PRIMARY KEY,
                context_id TEXT NOT NULL,
                trigger_type TEXT NOT NULL,
                snapshot_type TEXT NOT NULL,
                summary TEXT,
                terminal_tail TEXT,
                git_diff_summary TEXT,
                active_file TEXT,
                cursor_position TEXT,
                ai_context_summary TEXT,
                metadata_json TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS clipboard_events (
                id TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                content_type TEXT,
                size_bytes INTEGER,
                source_app TEXT,
                target_app TEXT,
                related_context_id TEXT,
                related_session_id TEXT,
                stored_preview TEXT,
                stored_full_content INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_runs (
                id TEXT PRIMARY KEY,
                context_id TEXT,
                tool_name TEXT,
                command_label TEXT,
                started_at INTEGER NOT NULL,
                ended_at INTEGER,
                status TEXT,
                exit_code INTEGER,
                summary TEXT,
                error_tail TEXT,
                notified INTEGER NOT NULL DEFAULT 0,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS commitments (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source TEXT,
                owner TEXT,
                due_at INTEGER,
                status TEXT NOT NULL DEFAULT 'open',
                confidence REAL NOT NULL DEFAULT 0,
                evidence_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS email_threads (
                id TEXT PRIMARY KEY,
                subject TEXT NOT NULL,
                latest_sender TEXT,
                latest_at INTEGER,
                pending_reply INTEGER NOT NULL DEFAULT 0,
                evidence_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS meetings (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                starts_at INTEGER,
                ends_at INTEGER,
                attendees_json TEXT,
                summary TEXT,
                actions_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS calendar_events (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL DEFAULT 'manual',
                external_id TEXT,
                calendar_name TEXT,
                title TEXT NOT NULL,
                starts_at INTEGER NOT NULL,
                ends_at INTEGER NOT NULL,
                location TEXT,
                status TEXT NOT NULL DEFAULT 'confirmed',
                planned_work_type TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS focus_sessions (
                id TEXT PRIMARY KEY,
                goal TEXT NOT NULL,
                client TEXT,
                project TEXT,
                task TEXT,
                ticket_id TEXT,
                target_ms INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                ended_at INTEGER,
                status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS recovery_events (
                id TEXT PRIMARY KEY,
                event_type TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                ended_at INTEGER,
                duration_ms INTEGER NOT NULL,
                note TEXT,
                evidence_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS field_visits (
                id TEXT PRIMARY KEY,
                client_label TEXT,
                starts_at INTEGER NOT NULL,
                ends_at INTEGER,
                location_label TEXT,
                debrief TEXT,
                status TEXT NOT NULL DEFAULT 'open',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS idle_blocks (
                id TEXT PRIMARY KEY,
                started_at INTEGER NOT NULL,
                ended_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                category TEXT,
                classified INTEGER NOT NULL DEFAULT 0,
                evidence_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS loop_item_actions (
                id TEXT PRIMARY KEY,
                action TEXT NOT NULL,
                snoozed_until INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS ai_usage (
                id TEXT PRIMARY KEY,
                provider TEXT,
                tool_name TEXT,
                thread_title TEXT,
                context_id TEXT,
                prompt_summary TEXT,
                output_summary TEXT,
                started_at INTEGER,
                ended_at INTEGER,
                duration_ms INTEGER,
                metadata_json TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS outputs (
                id TEXT PRIMARY KEY,
                output_type TEXT NOT NULL,
                title TEXT NOT NULL,
                source TEXT,
                ai_assisted INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'drafted',
                evidence_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS decisions (
                id TEXT PRIMARY KEY,
                statement TEXT NOT NULL,
                source TEXT,
                decided_at INTEGER NOT NULL,
                evidence_json TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS reports (
                id TEXT PRIMARY KEY,
                report_type TEXT NOT NULL,
                title TEXT NOT NULL,
                body_markdown TEXT NOT NULL,
                generated_at INTEGER NOT NULL,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS plans (
                id TEXT PRIMARY KEY,
                horizon TEXT NOT NULL,
                title TEXT NOT NULL,
                body_markdown TEXT NOT NULL,
                generated_at INTEGER NOT NULL,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS weekly_reviews (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                body_markdown TEXT NOT NULL,
                generated_at INTEGER NOT NULL,
                metadata_json TEXT
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS work_memory_fts USING fts5(
                entity_type,
                entity_id UNINDEXED,
                title,
                body,
                source,
                created_at UNINDEXED
            );

            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL,
                client_label TEXT,
                repo_path TEXT,
                domain TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS people (
                id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                email TEXT,
                relationship TEXT,
                priority INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS work_graph_edges (
                id TEXT PRIMARY KEY,
                from_type TEXT NOT NULL,
                from_id TEXT NOT NULL,
                to_type TEXT NOT NULL,
                to_id TEXT NOT NULL,
                relation TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 0,
                evidence_json TEXT,
                created_at INTEGER NOT NULL
            );

            -- Durable, user-intent links between recorded activities and tasks.
            -- Kept separate from the inferred work_graph_edges so manual and
            -- rule-based links survive re-materialization of the work graph.
            CREATE TABLE IF NOT EXISTS activity_task_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_event_id TEXT NOT NULL,
                task_id INTEGER NOT NULL,
                origin TEXT NOT NULL DEFAULT 'manual',
                rule_id INTEGER,
                created_at INTEGER NOT NULL,
                UNIQUE (source_event_id, task_id),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
                FOREIGN KEY (source_event_id) REFERENCES source_events(id) ON DELETE CASCADE
            );

            -- Per-task rules that auto-link matching activities.
            CREATE TABLE IF NOT EXISTS task_match_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                field TEXT NOT NULL DEFAULT 'any',
                matcher TEXT NOT NULL DEFAULT 'contains',
                pattern TEXT NOT NULL,
                case_sensitive INTEGER NOT NULL DEFAULT 0,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            -- Singleton row: the user's currently declared work context
            CREATE TABLE IF NOT EXISTS active_work_context (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                client TEXT,
                project TEXT,
                task TEXT,
                ticket_id TEXT,
                billable INTEGER NOT NULL DEFAULT 1,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS proactive_insights (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                insight_type TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                priority TEXT NOT NULL DEFAULT 'medium',
                action_hint TEXT,
                generated_at INTEGER NOT NULL,
                seen_at INTEGER,
                dismissed_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS daily_goals (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL,
                target_type TEXT NOT NULL,
                match_value TEXT NOT NULL,
                daily_target_ms INTEGER NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL
            );
            "#,
        )?;

        Self::migrate_tasks_schema(&conn)?;
        Self::ensure_column(&conn, "tasks", "due_at", "INTEGER")?;
        Self::ensure_column(&conn, "tasks", "notes", "TEXT")?;
        Self::ensure_column(&conn, "tasks", "priority", "TEXT")?;
        Self::ensure_column(&conn, "tasks", "client_label", "TEXT")?;
        Self::ensure_column(&conn, "tasks", "project_label", "TEXT")?;
        Self::ensure_column(&conn, "tasks", "reminder_sent_at", "INTEGER")?;
        Self::ensure_column(&conn, "tasks", "completed_at", "TEXT")?;
        Self::migrate_quick_notes_schema(&conn)?;
        Self::migrate_legacy_compatible_columns(&conn)?;
        Self::migrate_url_redactions(&conn)?;

        conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_tasks_status_due_date
                ON tasks(status, due_date);
            CREATE INDEX IF NOT EXISTS idx_tasks_status_due_at
                ON tasks(status, due_at);
            CREATE INDEX IF NOT EXISTS idx_tasks_status_completed_at
                ON tasks(status, completed_at);
            CREATE INDEX IF NOT EXISTS idx_activity_created_at
                ON activity_events(created_at);
            CREATE INDEX IF NOT EXISTS idx_source_events_time
                ON source_events(started_at, ended_at);
            CREATE INDEX IF NOT EXISTS idx_source_events_workspace
                ON source_events(workspace_key, started_at);
            CREATE INDEX IF NOT EXISTS idx_workspace_contexts_key
                ON workspace_contexts(context_key);
            CREATE INDEX IF NOT EXISTS idx_work_sessions_time
                ON work_sessions(started_at, ended_at);
            CREATE INDEX IF NOT EXISTS idx_parallel_streams_time
                ON parallel_streams(started_at, ended_at);
            CREATE INDEX IF NOT EXISTS idx_calendar_events_time
                ON calendar_events(starts_at, ends_at);
            CREATE INDEX IF NOT EXISTS idx_focus_sessions_time
                ON focus_sessions(started_at, ended_at);
            CREATE INDEX IF NOT EXISTS idx_recovery_events_time
                ON recovery_events(started_at, ended_at);
            CREATE INDEX IF NOT EXISTS idx_scratchpad_context
                ON scratchpad_notes(context_id, updated_at);
            CREATE INDEX IF NOT EXISTS idx_state_snapshots_context
                ON state_snapshots(context_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_clipboard_context
                ON clipboard_events(related_context_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_agent_runs_status
                ON agent_runs(status, started_at);
            CREATE INDEX IF NOT EXISTS idx_commitments_status_due
                ON commitments(status, due_at);
            CREATE INDEX IF NOT EXISTS idx_email_threads_pending
                ON email_threads(pending_reply, latest_at);
            CREATE INDEX IF NOT EXISTS idx_outputs_type_status
                ON outputs(output_type, status);
            CREATE INDEX IF NOT EXISTS idx_work_graph_from
                ON work_graph_edges(from_type, from_id);
            CREATE INDEX IF NOT EXISTS idx_work_graph_to
                ON work_graph_edges(to_type, to_id);
            CREATE INDEX IF NOT EXISTS idx_activity_task_links_task
                ON activity_task_links(task_id);
            CREATE INDEX IF NOT EXISTS idx_activity_task_links_event
                ON activity_task_links(source_event_id);
            CREATE INDEX IF NOT EXISTS idx_task_match_rules_task
                ON task_match_rules(task_id, enabled);
            "#,
        )?;

        let now = now_utc();
        conn.execute(
            "INSERT OR IGNORE INTO pause_state (id, paused, reason, updated_at) VALUES (1, 0, NULL, ?1)",
            params![now],
        )?;
        Ok(())
    }

    fn migrate_url_redactions(conn: &Connection) -> Result<()> {
        let mut source_stmt =
            conn.prepare("SELECT id, url_redacted, metadata_json FROM source_events")?;
        let source_rows = source_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut source_updates = Vec::new();
        for row in source_rows {
            let (id, url, metadata) = row?;
            let redacted_url = url.as_deref().and_then(|value| redact_url(value).1);
            let (redacted_metadata, metadata_changed) =
                redact_metadata_url_fields(metadata.as_deref())?;
            if redacted_url != url || metadata_changed {
                source_updates.push((id, redacted_url, redacted_metadata));
            }
        }
        drop(source_stmt);

        for (id, redacted_url, redacted_metadata) in source_updates {
            conn.execute(
                "UPDATE source_events SET url_redacted = ?1, metadata_json = ?2 WHERE id = ?3",
                params![redacted_url, redacted_metadata, id],
            )?;
        }

        let mut activity_stmt =
            conn.prepare("SELECT id, url, metadata_json FROM activity_events")?;
        let activity_rows = activity_stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut activity_updates = Vec::new();
        for row in activity_rows {
            let (id, url, metadata) = row?;
            let redacted_url = url.as_deref().and_then(|value| redact_url(value).1);
            let (redacted_metadata, metadata_changed) =
                redact_metadata_url_fields(metadata.as_deref())?;
            if redacted_url != url || metadata_changed {
                activity_updates.push((id, redacted_url, redacted_metadata));
            }
        }
        drop(activity_stmt);

        for (id, redacted_url, redacted_metadata) in activity_updates {
            conn.execute(
                "UPDATE activity_events SET url = ?1, metadata_json = ?2 WHERE id = ?3",
                params![redacted_url, redacted_metadata, id],
            )?;
        }

        Ok(())
    }

    fn migrate_legacy_compatible_columns(conn: &Connection) -> Result<()> {
        Self::ensure_column(conn, "source_events", "workspace_key", "TEXT")?;
        conn.execute(
            "UPDATE source_events SET workspace_key = COALESCE(workspace_key, domain, app, source) WHERE workspace_key IS NULL",
            [],
        )?;

        Self::ensure_column(conn, "work_sessions", "context_id", "TEXT")?;
        Self::ensure_column(
            conn,
            "work_sessions",
            "billing_status",
            "TEXT NOT NULL DEFAULT 'draft'",
        )?;
        Self::ensure_column(
            conn,
            "work_sessions",
            "billable",
            "INTEGER NOT NULL DEFAULT 1",
        )?;
        Self::ensure_column(conn, "work_sessions", "client_label", "TEXT")?;
        Self::ensure_column(conn, "work_sessions", "project_label", "TEXT")?;
        Self::ensure_column(conn, "work_sessions", "ticket_id", "TEXT")?;
        Self::ensure_column(conn, "work_sessions", "review_notes", "TEXT")?;

        Self::ensure_column(
            conn,
            "materialization_state",
            "total_source_duration_ms",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::ensure_column(
            conn,
            "materialization_state",
            "source_content_signature",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::ensure_column(
            conn,
            "materialization_state",
            "idle_gap_ms",
            "INTEGER NOT NULL DEFAULT 300000",
        )?;

        Self::ensure_column(conn, "email_threads", "latest_at", "INTEGER")?;
        Self::ensure_column(
            conn,
            "email_threads",
            "pending_reply",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::ensure_column(conn, "email_threads", "evidence_json", "TEXT")?;
        let email_columns = Self::table_columns(conn, "email_threads")?;
        if email_columns.contains_key("latest_received_at") {
            conn.execute(
                "UPDATE email_threads SET latest_at = latest_received_at WHERE latest_at IS NULL AND latest_received_at IS NOT NULL",
                [],
            )?;
        }
        if email_columns.contains_key("reply_required") {
            conn.execute(
                "UPDATE email_threads SET pending_reply = reply_required WHERE reply_required IS NOT NULL",
                [],
            )?;
        }

        Self::ensure_column(conn, "outputs", "source", "TEXT")?;
        Self::ensure_column(conn, "outputs", "ai_assisted", "INTEGER NOT NULL DEFAULT 0")?;
        let output_columns = Self::table_columns(conn, "outputs")?;
        if output_columns.contains_key("source_type") {
            conn.execute(
                "UPDATE outputs SET source = source_type WHERE source IS NULL AND source_type IS NOT NULL",
                [],
            )?;
        }

        Self::ensure_column(conn, "ai_usage", "provider", "TEXT")?;
        Self::ensure_column(conn, "ai_usage", "tool_name", "TEXT")?;
        Self::ensure_column(conn, "ai_usage", "thread_title", "TEXT")?;
        Self::ensure_column(conn, "ai_usage", "context_id", "TEXT")?;
        Self::ensure_column(conn, "ai_usage", "prompt_summary", "TEXT")?;
        Self::ensure_column(conn, "ai_usage", "output_summary", "TEXT")?;
        Self::ensure_column(conn, "ai_usage", "started_at", "INTEGER")?;
        Self::ensure_column(conn, "ai_usage", "ended_at", "INTEGER")?;
        Self::ensure_column(conn, "ai_usage", "duration_ms", "INTEGER")?;
        Self::ensure_column(conn, "ai_usage", "metadata_json", "TEXT")?;
        let ai_usage_columns = Self::table_columns(conn, "ai_usage")?;
        if ai_usage_columns.contains_key("tool") {
            conn.execute(
                "UPDATE ai_usage SET tool_name = tool WHERE tool_name IS NULL AND tool IS NOT NULL",
                [],
            )?;
        }
        if ai_usage_columns.contains_key("project_id") {
            conn.execute(
                "UPDATE ai_usage SET context_id = project_id WHERE context_id IS NULL AND project_id IS NOT NULL",
                [],
            )?;
        }
        if ai_usage_columns.contains_key("summary") {
            conn.execute(
                "UPDATE ai_usage SET prompt_summary = summary WHERE prompt_summary IS NULL AND summary IS NOT NULL",
                [],
            )?;
        }

        Self::ensure_column(conn, "meetings", "starts_at", "INTEGER")?;
        Self::ensure_column(conn, "meetings", "ends_at", "INTEGER")?;
        Self::ensure_column(conn, "meetings", "actions_json", "TEXT")?;
        let meeting_columns = Self::table_columns(conn, "meetings")?;
        if meeting_columns.contains_key("started_at") {
            conn.execute(
                "UPDATE meetings SET starts_at = started_at WHERE starts_at IS NULL AND started_at IS NOT NULL",
                [],
            )?;
        }
        if meeting_columns.contains_key("ended_at") {
            conn.execute(
                "UPDATE meetings SET ends_at = ended_at WHERE ends_at IS NULL AND ended_at IS NOT NULL",
                [],
            )?;
        }
        if meeting_columns.contains_key("action_items_json") {
            conn.execute(
                "UPDATE meetings SET actions_json = action_items_json WHERE actions_json IS NULL AND action_items_json IS NOT NULL",
                [],
            )?;
        }

        Self::ensure_column(conn, "decisions", "statement", "TEXT")?;
        Self::ensure_column(conn, "decisions", "source", "TEXT")?;
        let decision_columns = Self::table_columns(conn, "decisions")?;
        if decision_columns.contains_key("decision") {
            conn.execute(
                "UPDATE decisions SET statement = decision WHERE statement IS NULL AND decision IS NOT NULL",
                [],
            )?;
        }
        if decision_columns.contains_key("source_type") {
            conn.execute(
                "UPDATE decisions SET source = source_type WHERE source IS NULL AND source_type IS NOT NULL",
                [],
            )?;
        }

        Self::ensure_column(conn, "reports", "title", "TEXT")?;
        Self::ensure_column(conn, "reports", "body_markdown", "TEXT")?;
        Self::ensure_column(conn, "reports", "content_markdown", "TEXT")?;
        Self::ensure_column(conn, "reports", "generated_at", "INTEGER")?;
        Self::ensure_column(conn, "reports", "created_at", "INTEGER")?;
        Self::ensure_column(conn, "reports", "updated_at", "INTEGER")?;
        Self::ensure_column(conn, "reports", "metadata_json", "TEXT")?;
        conn.execute(
            "UPDATE reports SET title = COALESCE(title, report_type || ' report') WHERE title IS NULL",
            [],
        )?;
        let report_columns = Self::table_columns(conn, "reports")?;
        if report_columns.contains_key("content_markdown") {
            conn.execute(
                "UPDATE reports SET body_markdown = COALESCE(body_markdown, content_markdown, '') WHERE body_markdown IS NULL",
                [],
            )?;
        } else {
            conn.execute(
                "UPDATE reports SET body_markdown = COALESCE(body_markdown, '') WHERE body_markdown IS NULL",
                [],
            )?;
        }
        conn.execute(
            "UPDATE reports SET content_markdown = COALESCE(content_markdown, body_markdown, '') WHERE content_markdown IS NULL",
            [],
        )?;
        if report_columns.contains_key("created_at") {
            conn.execute(
                "UPDATE reports SET generated_at = COALESCE(generated_at, created_at, strftime('%s', 'now') * 1000) WHERE generated_at IS NULL",
                [],
            )?;
        } else {
            conn.execute(
                "UPDATE reports SET generated_at = COALESCE(generated_at, strftime('%s', 'now') * 1000) WHERE generated_at IS NULL",
                [],
            )?;
        }
        conn.execute(
            "UPDATE reports SET created_at = COALESCE(created_at, generated_at, strftime('%s', 'now') * 1000) WHERE created_at IS NULL",
            [],
        )?;
        conn.execute(
            "UPDATE reports SET updated_at = COALESCE(updated_at, generated_at, created_at, strftime('%s', 'now') * 1000) WHERE updated_at IS NULL",
            [],
        )?;

        Self::ensure_column(conn, "projects", "label", "TEXT")?;
        Self::ensure_column(conn, "projects", "client_label", "TEXT")?;
        Self::ensure_column(conn, "projects", "repo_path", "TEXT")?;
        Self::ensure_column(conn, "projects", "domain", "TEXT")?;
        let project_columns = Self::table_columns(conn, "projects")?;
        if project_columns.contains_key("name") {
            conn.execute(
                "UPDATE projects SET label = COALESCE(label, name, id) WHERE label IS NULL",
                [],
            )?;
        } else {
            conn.execute(
                "UPDATE projects SET label = COALESCE(label, id) WHERE label IS NULL",
                [],
            )?;
        }
        if project_columns.contains_key("client_name") {
            conn.execute(
                "UPDATE projects SET client_label = client_name WHERE client_label IS NULL AND client_name IS NOT NULL",
                [],
            )?;
        }

        Self::ensure_column(conn, "people", "display_name", "TEXT")?;
        Self::ensure_column(conn, "people", "relationship", "TEXT")?;
        Self::ensure_column(conn, "people", "priority", "INTEGER NOT NULL DEFAULT 0")?;
        let people_columns = Self::table_columns(conn, "people")?;
        if people_columns.contains_key("name") {
            conn.execute(
                "UPDATE people SET display_name = COALESCE(display_name, name, id) WHERE display_name IS NULL",
                [],
            )?;
        } else {
            conn.execute(
                "UPDATE people SET display_name = COALESCE(display_name, id) WHERE display_name IS NULL",
                [],
            )?;
        }
        if people_columns.contains_key("role") {
            conn.execute(
                "UPDATE people SET relationship = role WHERE relationship IS NULL AND role IS NOT NULL",
                [],
            )?;
        }

        Self::ensure_column(conn, "work_graph_edges", "evidence_json", "TEXT")?;
        Ok(())
    }

    fn migrate_tasks_schema(conn: &Connection) -> Result<()> {
        let columns = Self::table_columns(conn, "tasks")?;
        let id_is_integer = columns
            .get("id")
            .is_some_and(|column_type| column_type.eq_ignore_ascii_case("INTEGER"));
        let has_current_columns = columns.contains_key("due_date")
            && columns.contains_key("source")
            && columns.contains_key("project_path")
            && id_is_integer;

        if has_current_columns {
            return Ok(());
        }

        let due_date_expr = if columns.contains_key("due_date") {
            "due_date".to_string()
        } else if columns.contains_key("due_at") {
            "CASE WHEN due_at IS NULL THEN NULL ELSE strftime('%Y-%m-%d', CASE WHEN due_at > 9999999999 THEN due_at / 1000 ELSE due_at END, 'unixepoch') END".to_string()
        } else {
            "NULL".to_string()
        };
        let source_expr = if columns.contains_key("source") {
            "source"
        } else if columns.contains_key("source_type") {
            "source_type"
        } else {
            "NULL"
        };
        let project_path_expr = if columns.contains_key("project_path") {
            "project_path"
        } else if columns.contains_key("project_id") {
            "project_id"
        } else {
            "NULL"
        };
        let created_at_expr = Self::legacy_timestamp_expr(&columns, "created_at");
        let updated_at_expr = Self::legacy_timestamp_expr(&columns, "updated_at");

        conn.execute_batch(
            r#"
            ALTER TABLE tasks RENAME TO tasks_legacy_migration;

            CREATE TABLE tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'open',
                due_date TEXT,
                source TEXT,
                project_path TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )?;

        conn.execute(
            &format!(
                r#"
                INSERT INTO tasks (title, status, due_date, source, project_path, created_at, updated_at)
                SELECT
                    COALESCE(NULLIF(TRIM(title), ''), 'Untitled task'),
                    CASE WHEN status IN ('open', 'done') THEN status ELSE 'open' END,
                    {due_date_expr},
                    {source_expr},
                    {project_path_expr},
                    {created_at_expr},
                    {updated_at_expr}
                FROM tasks_legacy_migration
                WHERE title IS NOT NULL
                "#
            ),
            [],
        )?;
        conn.execute_batch("DROP TABLE tasks_legacy_migration;")?;
        Ok(())
    }

    fn migrate_quick_notes_schema(conn: &Connection) -> Result<()> {
        let columns = Self::table_columns(conn, "quick_notes")?;
        let id_is_integer = columns
            .get("id")
            .is_some_and(|column_type| column_type.eq_ignore_ascii_case("INTEGER"));
        if id_is_integer
            && columns.contains_key("body")
            && columns.contains_key("source")
            && columns.contains_key("project_path")
        {
            return Ok(());
        }

        let body_expr = if columns.contains_key("body") {
            "body"
        } else if columns.contains_key("note") {
            "note"
        } else {
            "''"
        };
        let created_at_expr = Self::legacy_timestamp_expr(&columns, "created_at");

        conn.execute_batch(
            r#"
            ALTER TABLE quick_notes RENAME TO quick_notes_legacy_migration;

            CREATE TABLE quick_notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                body TEXT NOT NULL,
                source TEXT,
                project_path TEXT,
                created_at TEXT NOT NULL
            );
            "#,
        )?;

        conn.execute(
            &format!(
                r#"
                INSERT INTO quick_notes (body, source, project_path, created_at)
                SELECT
                    COALESCE(NULLIF(TRIM({body_expr}), ''), 'Untitled note'),
                    NULL,
                    NULL,
                    {created_at_expr}
                FROM quick_notes_legacy_migration
                "#
            ),
            [],
        )?;
        conn.execute_batch("DROP TABLE quick_notes_legacy_migration;")?;
        Ok(())
    }

    fn table_columns(conn: &Connection, table: &str) -> Result<HashMap<String, String>> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut rows = stmt.query([])?;
        let mut columns = HashMap::new();
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            let column_type: String = row.get(2)?;
            columns.insert(name, column_type);
        }
        Ok(columns)
    }

    fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
        if !Self::table_columns(conn, table)?.contains_key(column) {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition};"
            ))?;
        }
        Ok(())
    }

    fn legacy_timestamp_expr(columns: &HashMap<String, String>, column: &str) -> String {
        if columns.contains_key(column) {
            format!(
                "CASE
                    WHEN {column} IS NULL THEN strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                    WHEN typeof({column}) = 'integer' THEN strftime('%Y-%m-%dT%H:%M:%SZ', CASE WHEN {column} > 9999999999 THEN {column} / 1000 ELSE {column} END, 'unixepoch')
                    ELSE CAST({column} AS TEXT)
                END"
            )
        } else {
            "strftime('%Y-%m-%dT%H:%M:%SZ', 'now')".to_string()
        }
    }

    pub fn create_task(&self, input: TaskInput) -> Result<Task> {
        let title = input.title.trim();
        anyhow::ensure!(!title.is_empty(), "task title is required");
        let due_date = input
            .due_date
            .or_else(|| input.due_at.and_then(epoch_ms_to_local_date));
        let priority = input.priority.as_deref().map(normalize_task_priority);

        let now = now_utc();
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO tasks (
                title, status, due_date, due_at, notes, priority, source, project_path,
                client_label, project_label, reminder_sent_at, completed_at, created_at, updated_at
            )
            VALUES (?1, 'open', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?10)
            "#,
            params![
                title,
                due_date,
                input.due_at,
                input
                    .notes
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                priority,
                input.source,
                input.project_path,
                input.client_label,
                input.project_label,
                now
            ],
        )?;
        let id = conn.last_insert_rowid();
        Self::get_task_locked(&conn, id)
    }

    pub fn update_task(&self, id: i64, input: TaskInput) -> Result<Task> {
        let title = input.title.trim();
        anyhow::ensure!(!title.is_empty(), "task title is required");
        let due_date = input
            .due_date
            .or_else(|| input.due_at.and_then(epoch_ms_to_local_date));
        let priority = input.priority.as_deref().map(normalize_task_priority);
        let now = now_utc();
        let conn = self.lock()?;
        conn.execute(
            r#"
            UPDATE tasks
            SET title = ?1,
                due_date = ?2,
                due_at = ?3,
                notes = ?4,
                priority = ?5,
                source = ?6,
                project_path = ?7,
                client_label = ?8,
                project_label = ?9,
                reminder_sent_at = NULL,
                updated_at = ?10
            WHERE id = ?11
            "#,
            params![
                title,
                due_date,
                input.due_at,
                input
                    .notes
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                priority,
                input.source,
                input.project_path,
                input.client_label,
                input.project_label,
                now,
                id
            ],
        )?;
        Self::get_task_locked(&conn, id)
    }

    pub fn draft_tasks_from_text(
        &self,
        text: &str,
        default_priority: Option<String>,
    ) -> Result<Vec<TaskDraft>> {
        let text = text.trim();
        anyhow::ensure!(!text.is_empty(), "task text is required");
        let default_priority = default_priority
            .as_deref()
            .map(normalize_task_priority)
            .unwrap_or_else(|| "high".to_string());
        let fallback = parse_task_drafts_from_text(text, &default_priority);

        let Some(ai_drafts) = self.try_draft_tasks_with_ai(text, &default_priority) else {
            return Ok(fallback);
        };

        Ok(if ai_drafts.is_empty() {
            fallback
        } else {
            ai_drafts
        })
    }

    fn try_draft_tasks_with_ai(
        &self,
        text: &str,
        default_priority: &str,
    ) -> Option<Vec<TaskDraft>> {
        let settings = self.get_settings().ok()?;
        let endpoint = settings.ai_endpoint.trim();
        let model = settings.ai_model.trim();
        if endpoint.is_empty() || model.is_empty() {
            return None;
        }

        let api_key = settings
            .ai_api_key_ref
            .as_deref()
            .and_then(keychain_key_from_ref)
            .and_then(|keychain_key| SystemKeychain.keychain_get(keychain_key).ok().flatten());
        let instruction = format!(
            "Extract backlog tasks from pasted user text. Return JSON only: an array of objects with keys title, notes, priority, clientLabel, projectLabel. Do not add due dates unless explicitly present. Use priority \"{default_priority}\" when the user says the items are urgent or no priority is provided. Do not invent tasks."
        );
        let generated = crate::llm::generate_text(
            &settings.ai_provider,
            endpoint,
            model,
            api_key.as_deref(),
            &instruction,
            text,
        )
        .ok()?;

        parse_task_drafts_from_ai_output(&generated, default_priority)
    }

    pub fn list_tasks(&self, status: Option<TaskStatus>) -> Result<Vec<Task>> {
        let conn = self.lock()?;
        let mut tasks = Vec::new();
        match status {
            Some(status) => {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT id, title, status, due_date, due_at, notes, priority, source,
                           project_path, client_label, project_label, reminder_sent_at,
                           completed_at, created_at, updated_at
                    FROM tasks
                    WHERE status = ?1
                    ORDER BY
                        CASE WHEN status = 'done' THEN 1 ELSE 0 END,
                        CASE WHEN status = 'done' THEN COALESCE(completed_at, updated_at) END DESC,
                        CASE WHEN due_at IS NULL THEN 1 ELSE 0 END,
                        due_at,
                        COALESCE(due_date, '9999-12-31'),
                        created_at DESC
                    "#,
                )?;
                for task in stmt.query_map(params![status.as_db_value()], Self::task_from_row)? {
                    tasks.push(task?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT id, title, status, due_date, due_at, notes, priority, source,
                           project_path, client_label, project_label, reminder_sent_at,
                           completed_at, created_at, updated_at
                    FROM tasks
                    ORDER BY
                        CASE WHEN status = 'done' THEN 1 ELSE 0 END,
                        CASE WHEN status = 'done' THEN COALESCE(completed_at, updated_at) END DESC,
                        CASE WHEN due_at IS NULL THEN 1 ELSE 0 END,
                        due_at,
                        COALESCE(due_date, '9999-12-31'),
                        created_at DESC
                    "#,
                )?;
                for task in stmt.query_map([], Self::task_from_row)? {
                    tasks.push(task?);
                }
            }
        }
        Ok(tasks)
    }

    pub fn complete_task(&self, id: i64) -> Result<Task> {
        let now = now_utc();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE tasks SET status = 'done', completed_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Self::get_task_locked(&conn, id)
    }

    pub fn snooze_task(&self, id: i64, due_at: i64) -> Result<Task> {
        anyhow::ensure!(due_at > 0, "task due time is required");
        let due_date = epoch_ms_to_local_date(due_at);
        let now = now_utc();
        let conn = self.lock()?;
        conn.execute(
            r#"
            UPDATE tasks
            SET status = 'open', due_at = ?1, due_date = ?2, reminder_sent_at = NULL, completed_at = NULL, updated_at = ?3
            WHERE id = ?4
            "#,
            params![due_at, due_date, now, id],
        )?;
        Self::get_task_locked(&conn, id)
    }

    pub fn delete_task(&self, id: i64) -> Result<PrivacyDeleteSummary> {
        let conn = self.lock()?;
        let deleted_rows = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    pub fn list_due_task_reminders(&self, now: i64) -> Result<Vec<Task>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, status, due_date, due_at, notes, priority, source,
                   project_path, client_label, project_label, reminder_sent_at,
                   completed_at, created_at, updated_at
            FROM tasks
            WHERE status = 'open'
              AND due_at IS NOT NULL
              AND due_at <= ?1
              AND reminder_sent_at IS NULL
            ORDER BY due_at
            LIMIT 20
            "#,
        )?;
        let rows = stmt.query_map(params![now], Self::task_from_row)?;
        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
    }

    pub fn mark_task_reminder_sent(&self, id: i64, sent_at: i64) -> Result<Task> {
        let now = now_utc();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE tasks SET reminder_sent_at = ?1, updated_at = ?2 WHERE id = ?3",
            params![sent_at, now, id],
        )?;
        Self::get_task_locked(&conn, id)
    }

    // ── Activity ↔ task links ──────────────────────────────────────────────

    /// Link a recorded activity to a task by hand. Idempotent: re-linking the
    /// same pair returns the existing link.
    pub fn link_activity_to_task(
        &self,
        source_event_id: &str,
        task_id: i64,
    ) -> Result<ActivityTaskLink> {
        let source_event_id = source_event_id.trim();
        anyhow::ensure!(!source_event_id.is_empty(), "activity id is required");
        let conn = self.lock()?;
        let event_exists: bool = conn
            .query_row(
                "SELECT 1 FROM source_events WHERE id = ?1",
                params![source_event_id],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        anyhow::ensure!(event_exists, "activity not found: {source_event_id}");
        let task_exists: bool = conn
            .query_row("SELECT 1 FROM tasks WHERE id = ?1", params![task_id], |_| {
                Ok(true)
            })
            .optional()?
            .unwrap_or(false);
        anyhow::ensure!(task_exists, "task not found: {task_id}");
        Self::insert_link_locked(&conn, source_event_id, task_id, LinkOrigin::Manual, None)?;
        Self::link_by_pair_locked(&conn, source_event_id, task_id)
    }

    /// Remove a manual or rule link between an activity and a task.
    pub fn unlink_activity_from_task(
        &self,
        source_event_id: &str,
        task_id: i64,
    ) -> Result<PrivacyDeleteSummary> {
        let conn = self.lock()?;
        let deleted_rows = conn.execute(
            "DELETE FROM activity_task_links WHERE source_event_id = ?1 AND task_id = ?2",
            params![source_event_id.trim(), task_id],
        )?;
        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    /// All activities linked to a task, newest activity first.
    pub fn list_task_activities(&self, task_id: i64) -> Result<Vec<LinkedActivity>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT e.id, e.source, e.event_type, e.app, e.title, e.domain, e.url_redacted,
                   e.workspace_key, e.started_at, e.ended_at, e.duration_ms, e.sensitivity,
                   e.metadata_json, e.created_at,
                   l.id, l.origin, l.rule_id, l.created_at
            FROM activity_task_links l
            JOIN source_events e ON e.id = l.source_event_id
            WHERE l.task_id = ?1
            ORDER BY e.started_at DESC, e.id DESC
            "#,
        )?;
        let rows = stmt.query_map(params![task_id], Self::linked_activity_from_row)?;
        let mut activities = Vec::new();
        for activity in rows {
            activities.push(activity?);
        }
        Ok(activities)
    }

    /// Build a rich summary of all activity linked to a task.
    pub fn get_task_activity_summary(
        &self,
        task_id: i64,
    ) -> Result<crate::models::TaskActivitySummary> {
        use crate::models::{TaskActivitySummary, TaskAppUsage, TaskWorkSession};
        let linked = self.list_task_activities(task_id)?;

        // --- Merge total time ---
        let event_refs: Vec<&SourceEvent> = linked.iter().map(|a| &a.event).collect();
        let total_ms = merge_event_intervals(&event_refs);

        // --- Per-app aggregation ---
        let mut app_map: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for activity in &linked {
            let app = activity.event.app.clone().unwrap_or_else(|| activity.event.source.clone());
            *app_map.entry(app).or_default() += activity.event.duration_ms;
        }
        let mut apps: Vec<TaskAppUsage> = app_map
            .into_iter()
            .map(|(app, duration_ms)| {
                let category = categorize_app(&app.to_ascii_lowercase()).to_string();
                TaskAppUsage { app, category, duration_ms }
            })
            .collect();
        apps.sort_unstable_by_key(|app| std::cmp::Reverse(app.duration_ms));

        // --- AI tools ---
        let mut ai_tools: Vec<String> = linked
            .iter()
            .flat_map(|a| {
                a.event
                    .metadata_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .and_then(|v| v.get("ai_tools").cloned())
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
                    .unwrap_or_default()
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        ai_tools.sort();

        // --- Group events into work sessions (30-min gap = new session) ---
        let session_gap_ms = 30 * 60 * 1_000_i64;
        let mut sorted_events = linked.clone();
        sorted_events.sort_unstable_by_key(|a| a.event.started_at);

        let mut work_sessions: Vec<TaskWorkSession> = Vec::new();
        let mut current_session: Option<(i64, i64, Vec<String>)> = None; // (start, end, apps)

        for activity in &sorted_events {
            let ev_start = activity.event.started_at;
            let ev_end = activity.event.ended_at.max(ev_start);
            let app = activity
                .event
                .app
                .clone()
                .unwrap_or_else(|| activity.event.source.clone());

            if let Some((sess_start, sess_end, ref mut sess_apps)) = current_session {
                if ev_start <= sess_end + session_gap_ms {
                    // extend session
                    let new_end = ev_end.max(sess_end);
                    if !sess_apps.contains(&app) {
                        sess_apps.push(app.clone());
                    }
                    current_session = Some((sess_start, new_end, sess_apps.clone()));
                    continue;
                } else {
                    let merged_ms = sess_end - sess_start;
                    work_sessions.push(TaskWorkSession {
                        started_at: sess_start,
                        ended_at: sess_end,
                        duration_ms: merged_ms.max(0),
                        apps: sess_apps.clone(),
                    });
                }
            }
            current_session = Some((ev_start, ev_end, vec![app]));
        }
        if let Some((sess_start, sess_end, sess_apps)) = current_session {
            work_sessions.push(TaskWorkSession {
                started_at: sess_start,
                ended_at: sess_end,
                duration_ms: (sess_end - sess_start).max(0),
                apps: sess_apps,
            });
        }
        work_sessions.reverse(); // most recent first

        Ok(TaskActivitySummary {
            task_id,
            total_ms,
            linked_count: linked.len() as i64,
            apps,
            ai_tools,
            work_sessions,
        })
    }

    /// Return scored candidate source events that are not yet linked to the task
    /// but likely relate to it, based on keyword overlap with the task title.
    pub fn suggest_task_links(
        &self,
        task_id: i64,
        limit: usize,
    ) -> Result<Vec<crate::models::TaskLinkSuggestion>> {
        use crate::models::TaskLinkSuggestion;
        // Get task title
        let conn = self.lock()?;
        let title: Option<String> = conn
            .query_row(
                "SELECT title FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .ok();
        let Some(title) = title else {
            return Ok(Vec::new());
        };
        drop(conn);

        // Extract meaningful keywords (>3 chars, not stop words)
        let stop_words = [
            "with", "that", "this", "from", "have", "they", "been", "will", "would", "could",
            "should", "into", "about", "task", "work", "done", "open", "note",
        ];
        let keywords: Vec<String> = title
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|w| w.to_ascii_lowercase())
            .filter(|w| !stop_words.contains(&w.as_str()))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        // Already-linked event ids
        let already_linked: std::collections::HashSet<String> = self
            .list_task_activities(task_id)?
            .into_iter()
            .map(|a| a.event.id.clone())
            .collect();

        // Scan last 30 days
        let thirty_days_ms = 30_i64 * 24 * 60 * 60 * 1_000;
        let from_ms = now_ms() - thirty_days_ms;
        let events = self.list_source_events_between(Some(from_ms), None, 5_000)?;

        let mut scored: Vec<(i32, String, crate::models::TaskLinkSuggestion)> = events
            .into_iter()
            .filter(|e| !already_linked.contains(&e.id) && e.event_type != "git_commit")
            .filter_map(|e| {
                let mut score = 0_i32;
                let mut reasons: Vec<String> = Vec::new();

                let title_lower = e.title.as_deref().unwrap_or("").to_ascii_lowercase();
                let ws_lower = e.workspace_key.as_deref().unwrap_or("").to_ascii_lowercase();
                let domain_lower = e.domain.as_deref().unwrap_or("").to_ascii_lowercase();

                for kw in &keywords {
                    if title_lower.contains(kw.as_str()) {
                        score += 10;
                        if reasons.iter().all(|r: &String| !r.starts_with("title")) {
                            reasons.push(format!("title matches \"{kw}\""));
                        }
                    }
                    if ws_lower.contains(kw.as_str()) {
                        score += 10;
                        if reasons.iter().all(|r: &String| !r.starts_with("project")) {
                            reasons.push(format!("project path matches \"{kw}\""));
                        }
                    }
                    if !domain_lower.is_empty() && domain_lower.contains(kw.as_str()) {
                        score += 5;
                        if reasons.iter().all(|r: &String| !r.starts_with("domain")) {
                            reasons.push(format!("domain matches \"{kw}\""));
                        }
                    }
                }

                if score == 0 {
                    return None;
                }

                let app = e.app.clone().unwrap_or_else(|| e.source.clone());
                let reason = reasons.join(", ");
                Some((
                    score,
                    e.started_at.to_string(),
                    TaskLinkSuggestion {
                        event_id: e.id,
                        app,
                        title: e.title,
                        workspace_key: e.workspace_key,
                        started_at: e.started_at,
                        ended_at: e.ended_at,
                        duration_ms: e.duration_ms,
                        match_reason: reason,
                        score,
                    },
                ))
            })
            .collect();

        scored.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, _, suggestion)| suggestion)
            .collect())
    }

    /// Recent recorded activities, optionally filtered by a case-insensitive
    /// substring over title/url/app/domain. Used by the manual-link picker so
    /// users can attach an existing activity to a task after the fact.
    pub fn search_recent_activities(
        &self,
        query: Option<String>,
        limit: usize,
    ) -> Result<Vec<SourceEvent>> {
        let needle = query
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty());
        let like = needle.as_ref().map(|value| format!("%{value}%"));
        let limit = limit.clamp(1, 200) as i64;
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, source, event_type, app, title, domain, url_redacted, workspace_key,
                   started_at, ended_at, duration_ms, sensitivity, metadata_json, created_at
            FROM source_events
            WHERE ?1 IS NULL
               OR lower(COALESCE(title, '')) LIKE ?1
               OR lower(COALESCE(url_redacted, '')) LIKE ?1
               OR lower(COALESCE(app, '')) LIKE ?1
               OR lower(COALESCE(domain, '')) LIKE ?1
            ORDER BY started_at DESC, id DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![like, limit], Self::source_event_from_row)?;
        let mut events = Vec::new();
        for event in rows {
            events.push(event?);
        }
        Ok(events)
    }

    /// All tasks linked to a given activity.
    pub fn list_activity_tasks(&self, source_event_id: &str) -> Result<Vec<Task>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT t.id, t.title, t.status, t.due_date, t.due_at, t.notes, t.priority, t.source,
                   t.project_path, t.client_label, t.project_label, t.reminder_sent_at,
                   t.completed_at, t.created_at, t.updated_at
            FROM activity_task_links l
            JOIN tasks t ON t.id = l.task_id
            WHERE l.source_event_id = ?1
            ORDER BY t.created_at DESC
            "#,
        )?;
        let rows = stmt.query_map(params![source_event_id.trim()], Self::task_from_row)?;
        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
    }

    /// Rules attached to a task, newest first.
    pub fn list_task_rules(&self, task_id: i64) -> Result<Vec<TaskMatchRule>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, task_id, field, matcher, pattern, case_sensitive, enabled,
                   created_at, updated_at
            FROM task_match_rules
            WHERE task_id = ?1
            ORDER BY created_at DESC, id DESC
            "#,
        )?;
        let rows = stmt.query_map(params![task_id], Self::task_rule_from_row)?;
        let mut rules = Vec::new();
        for rule in rows {
            rules.push(rule?);
        }
        Ok(rules)
    }

    /// Create a rule for a task. The pattern is compiled up front so an invalid
    /// regex/wildcard is rejected before it is stored.
    pub fn create_task_rule(
        &self,
        task_id: i64,
        input: TaskMatchRuleInput,
    ) -> Result<TaskMatchRule> {
        let pattern = input.pattern.trim();
        CompiledRule::compile(input.field, input.matcher, pattern, input.case_sensitive)?;
        let now = now_ms();
        let conn = self.lock()?;
        let task_exists: bool = conn
            .query_row("SELECT 1 FROM tasks WHERE id = ?1", params![task_id], |_| {
                Ok(true)
            })
            .optional()?
            .unwrap_or(false);
        anyhow::ensure!(task_exists, "task not found: {task_id}");
        conn.execute(
            r#"
            INSERT INTO task_match_rules
                (task_id, field, matcher, pattern, case_sensitive, enabled, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
            "#,
            params![
                task_id,
                input.field.as_db_value(),
                input.matcher.as_db_value(),
                pattern,
                input.case_sensitive as i64,
                input.enabled as i64,
                now
            ],
        )?;
        let id = conn.last_insert_rowid();
        Self::task_rule_by_id_locked(&conn, id)
    }

    /// Update an existing rule. Re-validates the pattern.
    pub fn update_task_rule(
        &self,
        rule_id: i64,
        input: TaskMatchRuleInput,
    ) -> Result<TaskMatchRule> {
        let pattern = input.pattern.trim();
        CompiledRule::compile(input.field, input.matcher, pattern, input.case_sensitive)?;
        let now = now_ms();
        let conn = self.lock()?;
        let changed = conn.execute(
            r#"
            UPDATE task_match_rules
            SET field = ?1, matcher = ?2, pattern = ?3, case_sensitive = ?4,
                enabled = ?5, updated_at = ?6
            WHERE id = ?7
            "#,
            params![
                input.field.as_db_value(),
                input.matcher.as_db_value(),
                pattern,
                input.case_sensitive as i64,
                input.enabled as i64,
                now,
                rule_id
            ],
        )?;
        anyhow::ensure!(changed == 1, "rule not found: {rule_id}");
        Self::task_rule_by_id_locked(&conn, rule_id)
    }

    /// Delete a rule. Links it previously created are kept (they become plain
    /// historical links); deleting the link is a separate action.
    pub fn delete_task_rule(&self, rule_id: i64) -> Result<PrivacyDeleteSummary> {
        let conn = self.lock()?;
        let deleted_rows =
            conn.execute("DELETE FROM task_match_rules WHERE id = ?1", params![rule_id])?;
        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    /// Apply rules to already-recorded activities. When `task_id` is `Some`,
    /// only that task's rules run; otherwise every enabled rule runs. New links
    /// are created with `INSERT OR IGNORE`, so re-running is safe and idempotent.
    pub fn apply_task_rules(&self, task_id: Option<i64>) -> Result<ApplyRulesSummary> {
        let conn = self.lock()?;
        let rules = Self::compiled_enabled_rules_locked(&conn, task_id)?;
        if rules.is_empty() {
            return Ok(ApplyRulesSummary {
                linked: 0,
                scanned: 0,
                rules: 0,
            });
        }
        let mut stmt = conn.prepare(
            r#"
            SELECT id, source, event_type, app, title, domain, url_redacted, workspace_key,
                   started_at, ended_at, duration_ms, sensitivity, metadata_json, created_at
            FROM source_events
            "#,
        )?;
        let events: Vec<SourceEvent> = stmt
            .query_map([], Self::source_event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);

        let mut linked = 0usize;
        for event in &events {
            for (rule_task_id, rule_id, compiled) in &rules {
                if compiled.matches(event) {
                    let inserted = Self::insert_link_locked(
                        &conn,
                        &event.id,
                        *rule_task_id,
                        LinkOrigin::Rule,
                        Some(*rule_id),
                    )?;
                    if inserted {
                        linked += 1;
                    }
                }
            }
        }
        Ok(ApplyRulesSummary {
            linked,
            scanned: events.len(),
            rules: rules.len(),
        })
    }

    /// Auto-link a freshly recorded activity against every enabled rule.
    /// Best-effort: an individual broken rule is skipped, never fatal to ingest.
    fn auto_link_event_locked(conn: &Connection, event: &SourceEvent) -> Result<()> {
        let rules = Self::compiled_enabled_rules_locked(conn, None)?;
        for (task_id, rule_id, compiled) in &rules {
            if compiled.matches(event) {
                Self::insert_link_locked(conn, &event.id, *task_id, LinkOrigin::Rule, Some(*rule_id))?;
            }
        }
        Ok(())
    }

    fn compiled_enabled_rules_locked(
        conn: &Connection,
        task_id: Option<i64>,
    ) -> Result<Vec<(i64, i64, CompiledRule)>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, task_id, field, matcher, pattern, case_sensitive, enabled,
                   created_at, updated_at
            FROM task_match_rules
            WHERE enabled = 1 AND (?1 IS NULL OR task_id = ?1)
            "#,
        )?;
        let rules: Vec<TaskMatchRule> = stmt
            .query_map(params![task_id], Self::task_rule_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut compiled = Vec::with_capacity(rules.len());
        for rule in rules {
            // A stored rule should already be valid; skip defensively if not.
            if let Ok(c) = CompiledRule::compile(
                rule.field,
                rule.matcher,
                &rule.pattern,
                rule.case_sensitive,
            ) {
                compiled.push((rule.task_id, rule.id, c));
            }
        }
        Ok(compiled)
    }

    fn insert_link_locked(
        conn: &Connection,
        source_event_id: &str,
        task_id: i64,
        origin: LinkOrigin,
        rule_id: Option<i64>,
    ) -> Result<bool> {
        let changed = conn.execute(
            r#"
            INSERT OR IGNORE INTO activity_task_links
                (source_event_id, task_id, origin, rule_id, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                source_event_id,
                task_id,
                origin.as_db_value(),
                rule_id,
                now_ms()
            ],
        )?;
        Ok(changed == 1)
    }

    fn link_by_pair_locked(
        conn: &Connection,
        source_event_id: &str,
        task_id: i64,
    ) -> Result<ActivityTaskLink> {
        conn.query_row(
            r#"
            SELECT id, source_event_id, task_id, origin, rule_id, created_at
            FROM activity_task_links
            WHERE source_event_id = ?1 AND task_id = ?2
            "#,
            params![source_event_id, task_id],
            Self::activity_task_link_from_row,
        )
        .map_err(Into::into)
    }

    fn task_rule_by_id_locked(conn: &Connection, id: i64) -> Result<TaskMatchRule> {
        conn.query_row(
            r#"
            SELECT id, task_id, field, matcher, pattern, case_sensitive, enabled,
                   created_at, updated_at
            FROM task_match_rules
            WHERE id = ?1
            "#,
            params![id],
            Self::task_rule_from_row,
        )
        .map_err(Into::into)
    }

    fn activity_task_link_from_row(row: &Row<'_>) -> rusqlite::Result<ActivityTaskLink> {
        let origin: String = row.get(3)?;
        let origin = LinkOrigin::try_from(origin.as_str()).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    err.to_string(),
                )),
            )
        })?;
        Ok(ActivityTaskLink {
            id: row.get(0)?,
            source_event_id: row.get(1)?,
            task_id: row.get(2)?,
            origin,
            rule_id: row.get(4)?,
            created_at: row.get(5)?,
        })
    }

    fn task_rule_from_row(row: &Row<'_>) -> rusqlite::Result<TaskMatchRule> {
        let field: String = row.get(2)?;
        let field = crate::matching::MatchField::try_from(field.as_str()).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    err.to_string(),
                )),
            )
        })?;
        let matcher: String = row.get(3)?;
        let matcher = crate::matching::MatcherType::try_from(matcher.as_str()).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    err.to_string(),
                )),
            )
        })?;
        Ok(TaskMatchRule {
            id: row.get(0)?,
            task_id: row.get(1)?,
            field,
            matcher,
            pattern: row.get(4)?,
            case_sensitive: row.get::<_, i64>(5)? == 1,
            enabled: row.get::<_, i64>(6)? == 1,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }

    fn linked_activity_from_row(row: &Row<'_>) -> rusqlite::Result<LinkedActivity> {
        let event = Self::source_event_from_row(row)?;
        let origin: String = row.get(15)?;
        let origin = LinkOrigin::try_from(origin.as_str()).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                15,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    err.to_string(),
                )),
            )
        })?;
        Ok(LinkedActivity {
            event,
            link_id: row.get(14)?,
            origin,
            rule_id: row.get(16)?,
            linked_at: row.get(17)?,
        })
    }

    pub fn create_commitment(&self, input: CommitmentInput) -> Result<Commitment> {
        let title = input.title.trim();
        anyhow::ensure!(!title.is_empty(), "commitment title is required");

        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("commitment-{}", Utc::now().timestamp_micros()));
        let confidence = input.confidence.unwrap_or(0.8).clamp(0.0, 1.0);
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO commitments
                (id, title, source, owner, due_at, status, confidence, evidence_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7, ?8, ?8)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                source = excluded.source,
                owner = excluded.owner,
                due_at = excluded.due_at,
                confidence = excluded.confidence,
                evidence_json = excluded.evidence_json,
                updated_at = excluded.updated_at
            "#,
            params![
                &id,
                title,
                &input.source,
                &input.owner,
                input.due_at,
                confidence,
                &input.evidence_json,
                now,
            ],
        )?;
        Self::get_commitment_locked(&conn, &id)
    }

    pub fn list_open_commitments(&self, limit: usize) -> Result<Vec<Commitment>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, source, owner, due_at, status, confidence, evidence_json, created_at, updated_at
            FROM commitments
            WHERE status = 'open'
            ORDER BY
                CASE WHEN due_at IS NULL THEN 1 ELSE 0 END,
                due_at,
                created_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::commitment_from_row)?;
        let mut commitments = Vec::new();
        for commitment in rows {
            commitments.push(commitment?);
        }
        Ok(commitments)
    }

    pub fn upsert_email_thread(&self, input: EmailThreadInput) -> Result<EmailThread> {
        let id = input.id.trim();
        anyhow::ensure!(!id.is_empty(), "email thread id is required");
        let subject = input.subject.trim();
        anyhow::ensure!(!subject.is_empty(), "email subject is required");

        let now = now_ms();
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO email_threads
                (id, subject, latest_sender, latest_at, pending_reply, evidence_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
            ON CONFLICT(id) DO UPDATE SET
                subject = excluded.subject,
                latest_sender = excluded.latest_sender,
                latest_at = excluded.latest_at,
                pending_reply = excluded.pending_reply,
                evidence_json = excluded.evidence_json,
                updated_at = excluded.updated_at
            "#,
            params![
                id,
                subject,
                &input.latest_sender,
                input.latest_at,
                if input.pending_reply { 1 } else { 0 },
                &input.evidence_json,
                now,
            ],
        )?;
        Self::get_email_thread_locked(&conn, id)
    }

    pub fn list_pending_replies(&self, limit: usize) -> Result<Vec<EmailThread>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, subject, latest_sender, latest_at, pending_reply, evidence_json, created_at, updated_at
            FROM email_threads
            WHERE pending_reply = 1
            ORDER BY latest_at, updated_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::email_thread_from_row)?;
        let mut replies = Vec::new();
        for reply in rows {
            replies.push(reply?);
        }
        Ok(replies)
    }

    pub fn add_quick_note(
        &self,
        body: &str,
        source: Option<&str>,
        project_path: Option<&str>,
    ) -> Result<QuickNote> {
        let body = body.trim();
        anyhow::ensure!(!body.is_empty(), "quick note body is required");

        let now = now_utc();
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO quick_notes (body, source, project_path, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![body, source, project_path, now],
        )?;
        let id = conn.last_insert_rowid();
        Self::get_quick_note_locked(&conn, id)
    }

    pub fn list_quick_notes(&self, limit: usize) -> Result<Vec<QuickNote>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, body, source, project_path, created_at
            FROM quick_notes
            ORDER BY created_at DESC, id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::quick_note_from_row)?;
        let mut notes = Vec::new();
        for note in rows {
            notes.push(note?);
        }
        Ok(notes)
    }

    pub fn delete_quick_note(&self, id: i64) -> Result<PrivacyDeleteSummary> {
        let conn = self.lock()?;
        let deleted_rows = conn.execute("DELETE FROM quick_notes WHERE id = ?1", params![id])?;
        Self::rebuild_work_memory_index_locked(&conn)?;
        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    pub fn get_settings(&self) -> Result<Settings> {
        let conn = self.lock()?;
        let mut settings = Settings::default();
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (key, value) = row?;
            match key.as_str() {
                "idle_timeout_minutes" => {
                    settings.idle_timeout_minutes =
                        value.parse().unwrap_or(settings.idle_timeout_minutes);
                }
                "export_format" => settings.export_format = value,
                "launch_at_login" => {
                    settings.launch_at_login = matches!(value.as_str(), "1" | "true");
                }
                "browser_bridge_enabled" => {
                    settings.browser_bridge_enabled = matches!(value.as_str(), "1" | "true");
                }
                "terminal_bridge_path" => {
                    if !value.trim().is_empty() {
                        settings.terminal_bridge_path = Some(value);
                    }
                }
                "excluded_apps" => {
                    settings.excluded_apps = parse_string_list_setting(&value);
                }
                "excluded_domains" => {
                    settings.excluded_domains = parse_string_list_setting(&value);
                }
                "excluded_projects" => {
                    settings.excluded_projects = parse_string_list_setting(&value);
                }
                "ai_provider" => settings.ai_provider = value,
                "ai_model" => settings.ai_model = value,
                "ai_endpoint" => settings.ai_endpoint = value,
                "ai_api_key_ref" => {
                    if !value.trim().is_empty() {
                        settings.ai_api_key_ref = Some(value);
                    }
                }
                "ai_redact_secrets" => {
                    settings.ai_redact_secrets = matches!(value.as_str(), "1" | "true");
                }
                "full_clipboard_history" => {
                    settings.full_clipboard_history = matches!(value.as_str(), "1" | "true");
                }
                "experience_mode" => {
                    settings.experience_mode = match value.as_str() {
                        "pro" => "pro".to_string(),
                        _ => "simple".to_string(),
                    };
                }
                "show_system_apps" => {
                    settings.show_system_apps = matches!(value.as_str(), "1" | "true");
                }
                "show_raw_events" => {
                    settings.show_raw_events = matches!(value.as_str(), "1" | "true");
                }
                "show_capture_confidence" => {
                    settings.show_capture_confidence = matches!(value.as_str(), "1" | "true");
                }
                "show_ai_details" => {
                    settings.show_ai_details = match value.as_str() {
                        "detailed" => "detailed".to_string(),
                        _ => "summary".to_string(),
                    };
                }
                "data_retention_days" => {
                    settings.data_retention_days =
                        value.parse().unwrap_or(settings.data_retention_days);
                }
                "task_retention_days" => {
                    settings.task_retention_days =
                        value.parse().unwrap_or(settings.task_retention_days);
                }
                "recovery_enabled" => {
                    settings.recovery_enabled = matches!(value.as_str(), "1" | "true");
                }
                "recovery_threshold_minutes" => {
                    settings.recovery_threshold_minutes =
                        value.parse().unwrap_or(settings.recovery_threshold_minutes);
                }
                "work_hours_enabled" => {
                    settings.work_hours_enabled = matches!(value.as_str(), "1" | "true");
                }
                "work_start_hour" => {
                    settings.work_start_hour =
                        value.parse().unwrap_or(settings.work_start_hour);
                }
                "work_end_hour" => {
                    settings.work_end_hour =
                        value.parse().unwrap_or(settings.work_end_hour);
                }
                "min_gap_minutes" => {
                    settings.min_gap_minutes =
                        value.parse().unwrap_or(settings.min_gap_minutes);
                }
                "premium_notifications_enabled" => {
                    settings.premium_notifications_enabled = matches!(value.as_str(), "1" | "true");
                }
                "notification_sound" => {
                    settings.notification_sound = normalize_notification_sound(&value);
                }
                _ => {}
            }
        }
        Ok(settings)
    }

    pub fn ensure_default_launch_at_login(&self) -> Result<()> {
        let default_enabled = Settings::default().launch_at_login;
        if !default_enabled {
            return Ok(());
        }

        let has_explicit_setting = {
            let conn = self.lock()?;
            conn.query_row(
                "SELECT 1 FROM settings WHERE key = 'launch_at_login' LIMIT 1",
                [],
                |_| Ok(()),
            )
            .optional()?
            .is_some()
        };

        if has_explicit_setting {
            return Ok(());
        }

        set_launch_at_login(true)?;
        let conn = self.lock()?;
        let now = now_utc();
        Self::upsert_setting_locked(&conn, "launch_at_login", "true", &now)?;
        Ok(())
    }

    pub fn update_settings(&self, patch: SettingsPatch) -> Result<Settings> {
        let conn = self.lock()?;
        let now = now_utc();

        if let Some(value) = patch.idle_timeout_minutes {
            anyhow::ensure!(value > 0, "idle timeout must be greater than zero");
            Self::upsert_setting_locked(&conn, "idle_timeout_minutes", &value.to_string(), &now)?;
        }
        if let Some(value) = patch.export_format {
            anyhow::ensure!(value == "json", "only json export is currently supported");
            Self::upsert_setting_locked(&conn, "export_format", &value, &now)?;
        }
        if let Some(value) = patch.launch_at_login {
            set_launch_at_login(value)?;
            Self::upsert_setting_locked(
                &conn,
                "launch_at_login",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.browser_bridge_enabled {
            Self::upsert_setting_locked(
                &conn,
                "browser_bridge_enabled",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.terminal_bridge_path {
            Self::upsert_setting_locked(&conn, "terminal_bridge_path", &value, &now)?;
        }
        if let Some(value) = patch.excluded_apps {
            Self::upsert_setting_locked(
                &conn,
                "excluded_apps",
                &serde_json::to_string(&normalize_string_list(value))?,
                &now,
            )?;
        }
        if let Some(value) = patch.excluded_domains {
            Self::upsert_setting_locked(
                &conn,
                "excluded_domains",
                &serde_json::to_string(&normalize_string_list(value))?,
                &now,
            )?;
        }
        if let Some(value) = patch.excluded_projects {
            Self::upsert_setting_locked(
                &conn,
                "excluded_projects",
                &serde_json::to_string(&normalize_string_list(value))?,
                &now,
            )?;
        }
        if let Some(value) = patch.ai_provider {
            let value = clean_report_text(&value);
            anyhow::ensure!(!value.is_empty(), "AI provider is required");
            Self::upsert_setting_locked(&conn, "ai_provider", &value, &now)?;
            let keychain_key = keychain_key_for_ai_provider(&value);
            Self::upsert_setting_locked(
                &conn,
                "ai_api_key_ref",
                &format!("keychain:{keychain_key}"),
                &now,
            )?;
        }
        if let Some(value) = patch.ai_model {
            let value = clean_report_text(&value);
            anyhow::ensure!(!value.is_empty(), "AI model is required");
            Self::upsert_setting_locked(&conn, "ai_model", &value, &now)?;
        }
        if let Some(value) = patch.ai_endpoint {
            let value = value.trim().trim_end_matches('/').to_string();
            anyhow::ensure!(
                value.starts_with("http://") || value.starts_with("https://"),
                "AI endpoint must start with http:// or https://"
            );
            Self::upsert_setting_locked(&conn, "ai_endpoint", &value, &now)?;
        }
        if let Some(value) = patch.ai_redact_secrets {
            Self::upsert_setting_locked(
                &conn,
                "ai_redact_secrets",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.full_clipboard_history {
            Self::upsert_setting_locked(
                &conn,
                "full_clipboard_history",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.experience_mode {
            let value = value.trim();
            anyhow::ensure!(
                matches!(value, "simple" | "pro"),
                "experience mode must be simple or pro"
            );
            Self::upsert_setting_locked(&conn, "experience_mode", value, &now)?;
        }
        if let Some(value) = patch.show_system_apps {
            Self::upsert_setting_locked(
                &conn,
                "show_system_apps",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.show_raw_events {
            Self::upsert_setting_locked(
                &conn,
                "show_raw_events",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.show_capture_confidence {
            Self::upsert_setting_locked(
                &conn,
                "show_capture_confidence",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.show_ai_details {
            let value = value.trim();
            anyhow::ensure!(
                matches!(value, "summary" | "detailed"),
                "AI details mode must be summary or detailed"
            );
            Self::upsert_setting_locked(&conn, "show_ai_details", value, &now)?;
        }
        if let Some(value) = patch.data_retention_days {
            anyhow::ensure!(
                value == 0 || (1..=3650).contains(&value),
                "data retention must be 0 or between 1 and 3650 days"
            );
            Self::upsert_setting_locked(&conn, "data_retention_days", &value.to_string(), &now)?;
        }
        if let Some(value) = patch.task_retention_days {
            anyhow::ensure!(
                value == 0 || (1..=3650).contains(&value),
                "task retention must be 0 or between 1 and 3650 days"
            );
            Self::upsert_setting_locked(&conn, "task_retention_days", &value.to_string(), &now)?;
        }
        if let Some(value) = patch.recovery_enabled {
            Self::upsert_setting_locked(
                &conn,
                "recovery_enabled",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.recovery_threshold_minutes {
            anyhow::ensure!(
                (15..=120).contains(&value),
                "recovery threshold must be between 15 and 120 minutes"
            );
            Self::upsert_setting_locked(
                &conn,
                "recovery_threshold_minutes",
                &value.to_string(),
                &now,
            )?;
        }
        if let Some(value) = patch.work_hours_enabled {
            Self::upsert_setting_locked(
                &conn,
                "work_hours_enabled",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.work_start_hour {
            anyhow::ensure!((0..=23).contains(&value), "work start hour must be 0–23");
            Self::upsert_setting_locked(&conn, "work_start_hour", &value.to_string(), &now)?;
        }
        if let Some(value) = patch.work_end_hour {
            anyhow::ensure!((1..=24).contains(&value), "work end hour must be 1–24");
            Self::upsert_setting_locked(&conn, "work_end_hour", &value.to_string(), &now)?;
        }
        if let Some(value) = patch.min_gap_minutes {
            anyhow::ensure!(
                (5..=120).contains(&value),
                "minimum gap must be between 5 and 120 minutes"
            );
            Self::upsert_setting_locked(&conn, "min_gap_minutes", &value.to_string(), &now)?;
        }
        if let Some(value) = patch.premium_notifications_enabled {
            Self::upsert_setting_locked(
                &conn,
                "premium_notifications_enabled",
                if value { "true" } else { "false" },
                &now,
            )?;
        }
        if let Some(value) = patch.notification_sound {
            let value = normalize_notification_sound(&value);
            anyhow::ensure!(
                matches!(value.as_str(), "daytrail" | "glass" | "subtle" | "none"),
                "notification sound must be daytrail, glass, subtle, or none"
            );
            Self::upsert_setting_locked(&conn, "notification_sound", &value, &now)?;
        }
        drop(conn);
        self.get_settings()
    }

    pub fn set_ai_api_key(&self, provider: &str, api_key: &str) -> Result<Settings> {
        self.set_ai_api_key_with_keychain(provider, api_key, &SystemKeychain)
    }

    pub fn set_ai_api_key_with_keychain(
        &self,
        provider: &str,
        api_key: &str,
        keychain: &dyn KeychainAdapter,
    ) -> Result<Settings> {
        let provider = clean_report_text(provider);
        let api_key = api_key.trim();
        anyhow::ensure!(!provider.is_empty(), "AI provider is required");
        anyhow::ensure!(!api_key.is_empty(), "AI API key is required");

        let keychain_key = keychain_key_for_ai_provider(&provider);
        keychain.keychain_set(&keychain_key, api_key)?;

        let conn = self.lock()?;
        let now = now_utc();
        Self::upsert_setting_locked(&conn, "ai_provider", &provider, &now)?;
        Self::upsert_setting_locked(
            &conn,
            "ai_api_key_ref",
            &format!("keychain:{keychain_key}"),
            &now,
        )?;
        drop(conn);
        self.get_settings()
    }

    pub fn storage_locations(&self) -> Result<StorageLocationInfo> {
        let backup_dir = self.backup_dir()?;
        let database_bytes = file_size(&self.db_path);
        let wal_bytes = file_size(&self.db_path.with_extension("sqlite3-wal"));
        let shm_bytes = file_size(&self.db_path.with_extension("sqlite3-shm"));
        let backup_bytes = dir_size(&backup_dir);
        let total_bytes = database_bytes
            .saturating_add(wal_bytes)
            .saturating_add(shm_bytes)
            .saturating_add(backup_bytes);
        Ok(StorageLocationInfo {
            database_path: self.db_path.display().to_string(),
            backup_dir: backup_dir.display().to_string(),
            database_bytes,
            wal_bytes,
            shm_bytes,
            backup_bytes,
            total_bytes,
            retention_days: self.get_settings()?.data_retention_days,
        })
    }

    pub fn export_settings_config_json(&self) -> Result<String> {
        let mut settings = self.get_settings()?;
        settings.ai_api_key_ref = None;
        let payload = SettingsConfigPayload {
            schema_version: 1,
            exported_at: now_utc(),
            settings,
            secrets_exported: false,
        };
        serde_json::to_string_pretty(&payload).context("failed to serialize settings config")
    }

    pub fn import_settings_config_json(&self, config_json: &str) -> Result<Settings> {
        let payload: SettingsConfigPayload =
            serde_json::from_str(config_json).context("invalid settings config JSON")?;
        anyhow::ensure!(
            payload.schema_version == 1,
            "unsupported settings config schema version: {}",
            payload.schema_version
        );

        let settings = payload.settings;
        let imported = self.update_settings(SettingsPatch {
            idle_timeout_minutes: Some(settings.idle_timeout_minutes),
            export_format: Some(settings.export_format),
            launch_at_login: Some(settings.launch_at_login),
            browser_bridge_enabled: Some(settings.browser_bridge_enabled),
            terminal_bridge_path: Some(settings.terminal_bridge_path.unwrap_or_default()),
            excluded_apps: Some(settings.excluded_apps),
            excluded_domains: Some(settings.excluded_domains),
            excluded_projects: Some(settings.excluded_projects),
            ai_provider: Some(settings.ai_provider),
            ai_model: Some(settings.ai_model),
            ai_endpoint: Some(settings.ai_endpoint),
            ai_redact_secrets: Some(settings.ai_redact_secrets),
            full_clipboard_history: Some(settings.full_clipboard_history),
            experience_mode: Some(settings.experience_mode),
            show_system_apps: Some(settings.show_system_apps),
            show_raw_events: Some(settings.show_raw_events),
            show_capture_confidence: Some(settings.show_capture_confidence),
            show_ai_details: Some(settings.show_ai_details),
            data_retention_days: Some(settings.data_retention_days),
            task_retention_days: Some(settings.task_retention_days),
            recovery_enabled: Some(settings.recovery_enabled),
            recovery_threshold_minutes: Some(settings.recovery_threshold_minutes),
            work_hours_enabled: Some(settings.work_hours_enabled),
            work_start_hour: Some(settings.work_start_hour),
            work_end_hour: Some(settings.work_end_hour),
            min_gap_minutes: Some(settings.min_gap_minutes),
            premium_notifications_enabled: Some(settings.premium_notifications_enabled),
            notification_sound: Some(settings.notification_sound),
        })?;

        {
            let conn = self.lock()?;
            conn.execute("DELETE FROM settings WHERE key = 'ai_api_key_ref'", [])?;
        }

        let mut imported = imported;
        imported.ai_api_key_ref = None;
        Ok(imported)
    }

    pub fn backup_database_to_default(&self) -> Result<DatabaseTransferResult> {
        let backup_dir = self.backup_dir()?;
        fs::create_dir_all(&backup_dir).with_context(|| {
            format!(
                "failed to create database backup directory {}",
                backup_dir.display()
            )
        })?;
        let backup_path = backup_dir.join(format!(
            "daytrail-backup-{}.sqlite3",
            Utc::now().timestamp_millis()
        ));
        self.backup_database_to_path(&backup_path)
    }

    pub fn backup_database_to_path(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<DatabaseTransferResult> {
        let destination = path.as_ref();
        anyhow::ensure!(
            destination
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| !value.trim().is_empty()),
            "backup path must include a file name"
        );
        if let (Ok(source), Ok(destination)) =
            (self.db_path.canonicalize(), destination.canonicalize())
        {
            anyhow::ensure!(
                source != destination,
                "backup path cannot be the active database"
            );
        }
        anyhow::ensure!(
            !destination.exists(),
            "backup file already exists: {}",
            destination.display()
        );
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create backup directory {}", parent.display())
            })?;
        }

        {
            let conn = self.lock()?;
            conn.backup(DatabaseName::Main, destination, None)
                .with_context(|| {
                    format!("failed to back up database to {}", destination.display())
                })?;
        }

        ensure_sqlite_integrity(destination)?;
        let bytes = fs::metadata(destination)
            .with_context(|| format!("failed to read backup metadata {}", destination.display()))?
            .len();
        Ok(DatabaseTransferResult {
            path: destination.display().to_string(),
            bytes,
            generated_at: now_utc(),
            pre_restore_backup_path: None,
        })
    }

    pub fn restore_database_from_path(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<DatabaseTransferResult> {
        let source = path.as_ref();
        anyhow::ensure!(
            source.exists() && source.is_file(),
            "database restore file does not exist: {}",
            source.display()
        );
        if let (Ok(active), Ok(source)) = (self.db_path.canonicalize(), source.canonicalize()) {
            anyhow::ensure!(
                active != source,
                "restore file cannot be the active database"
            );
        }
        ensure_sqlite_integrity(source)?;
        let source_bytes = fs::metadata(source)
            .with_context(|| format!("failed to read restore file metadata {}", source.display()))?
            .len();
        let pre_restore_backup = self.backup_database_to_default()?;

        {
            let mut conn = self.lock()?;
            conn.restore(
                DatabaseName::Main,
                source,
                None::<fn(rusqlite::backup::Progress)>,
            )
            .with_context(|| format!("failed to restore database from {}", source.display()))?;
            conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        }

        self.migrate()?;
        Ok(DatabaseTransferResult {
            path: source.display().to_string(),
            bytes: source_bytes,
            generated_at: now_utc(),
            pre_restore_backup_path: Some(pre_restore_backup.path),
        })
    }

    pub fn try_generate_ai_markdown(
        &self,
        prompt_kind: &str,
        instruction: &str,
        context_markdown: &str,
        keychain: &dyn KeychainAdapter,
    ) -> Option<String> {
        let settings = self.get_settings().ok()?;
        let api_key = settings
            .ai_api_key_ref
            .as_deref()
            .and_then(keychain_key_from_ref)
            .and_then(|keychain_key| keychain.keychain_get(keychain_key).ok().flatten());
        let endpoint = settings.ai_endpoint.trim();
        let model = settings.ai_model.trim();
        if endpoint.is_empty() || model.is_empty() {
            return None;
        }

        let context_markdown = if settings.ai_redact_secrets {
            redact_ai_context(context_markdown)
        } else {
            context_markdown.to_string()
        };
        let instruction =
            format!("{instruction}\n\nPrompt kind: {prompt_kind}\nReturn markdown only.");
        let generated = crate::llm::generate_text(
            &settings.ai_provider,
            endpoint,
            model,
            api_key.as_deref(),
            &instruction,
            &context_markdown,
        )
        .ok()
        .filter(|value| !value.trim().is_empty())?;

        let audit_id = format!(
            "ai-worktrace-{}-{}",
            stable_id_part(prompt_kind),
            Utc::now().timestamp_micros()
        );
        let output_id = format!("output-{audit_id}");
        let context_summary = truncate_for_audit(&context_markdown, 320);
        let output_summary = truncate_for_audit(&generated, 320);
        let _ = self.record_ai_usage(AiUsageInput {
            id: Some(audit_id.clone()),
            provider: Some(settings.ai_provider.clone()),
            tool_name: Some(settings.ai_provider.clone()),
            thread_title: Some(format!("{DISPLAY_APP_NAME} {prompt_kind}")),
            context_id: Some("worktrace:internal".to_string()),
            prompt_summary: Some(context_summary),
            output_summary: Some(output_summary.clone()),
            started_at: Some(now_ms()),
            ended_at: Some(now_ms()),
            duration_ms: Some(0),
            metadata_json: Some(
                serde_json::json!({
                    "source": "worktrace-ai-execution",
                    "promptKind": prompt_kind,
                })
                .to_string(),
            ),
        });
        let _ = self.record_work_output(WorkOutputInput {
            id: Some(output_id),
            output_type: prompt_kind.to_string(),
            title: format!("{DISPLAY_APP_NAME} {prompt_kind}"),
            source: Some(DISPLAY_APP_NAME.to_string()),
            ai_assisted: Some(true),
            status: Some("completed".to_string()),
            evidence_json: Some(
                serde_json::json!({
                    "ids": [audit_id],
                    "outputSummary": output_summary
                })
                .to_string(),
            ),
        });

        Some(generated)
    }

    pub fn pause(&self, reason: &str) -> Result<PauseState> {
        let now = now_utc();
        let reason = reason.trim();
        let reason = if reason.is_empty() {
            None
        } else {
            Some(reason)
        };
        let conn = self.lock()?;
        conn.execute(
            "UPDATE pause_state SET paused = 1, reason = ?1, updated_at = ?2 WHERE id = 1",
            params![reason, now],
        )?;
        Self::pause_state_locked(&conn)
    }

    pub fn resume(&self) -> Result<PauseState> {
        let now = now_utc();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE pause_state SET paused = 0, reason = NULL, updated_at = ?1 WHERE id = 1",
            params![now],
        )?;
        Self::pause_state_locked(&conn)
    }

    pub fn pause_state(&self) -> Result<PauseState> {
        let conn = self.lock()?;
        Self::pause_state_locked(&conn)
    }

    pub fn today_snapshot(&self) -> Result<TodaySnapshot> {
        if self.auto_ingest_local_bridges {
            let _ = self.ingest_local_bridge_files();
        }
        let today_date = Local::now().date_naive();
        let local_date = today_date.format("%Y-%m-%d").to_string();
        let (day_start, day_end) = local_day_bounds_ms(today_date);
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, status, due_date, due_at, notes, priority, source,
                   project_path, client_label, project_label, reminder_sent_at,
                   completed_at, created_at, updated_at
            FROM tasks
            WHERE status = 'open'
            ORDER BY
                CASE WHEN due_at IS NULL THEN 1 ELSE 0 END,
                due_at,
                COALESCE(due_date, '9999-12-31'),
                created_at DESC
            LIMIT 20
            "#,
        )?;
        let rows = stmt.query_map([], Self::task_from_row)?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row?);
        }
        drop(stmt);
        let mut done_stmt = conn.prepare(
            r#"
            SELECT id, title, status, due_date, due_at, notes, priority, source,
                   project_path, client_label, project_label, reminder_sent_at,
                   completed_at, created_at, updated_at
            FROM tasks
            WHERE status = 'done'
            ORDER BY COALESCE(completed_at, updated_at) DESC
            LIMIT 20
            "#,
        )?;
        let done_rows = done_stmt.query_map([], Self::task_from_row)?;
        for row in done_rows {
            tasks.push(row?);
        }
        drop(done_stmt);
        drop(conn);

        let mut work_sessions =
            self.list_work_sessions_between(Some(day_start), Some(day_end), 20)?;
        let mut parallel_streams =
            self.list_parallel_streams_between(Some(day_start), Some(day_end), 20)?;
        if work_sessions.is_empty() || parallel_streams.is_empty() {
            let (fallback_sessions, fallback_streams) =
                self.source_event_fallback_between(20, Some(day_start), Some(day_end))?;
            if work_sessions.is_empty() {
                work_sessions = fallback_sessions;
            }
            if parallel_streams.is_empty() {
                parallel_streams = fallback_streams;
            }
        }
        let quick_notes = self.list_quick_notes(20)?;
        let commitments = self.list_open_commitments(20)?;
        let pending_replies = self.list_pending_replies(20)?;
        let ai_outputs = self.list_work_outputs(20)?;
        let calendar_events =
            self.list_calendar_events_between(Some(day_start), Some(day_end), 100)?;
        let meetings = self.list_meetings(20)?;
        let field_visits = self.list_field_visits(20)?;
        let idle_blocks = self
            .list_idle_blocks_between(Some(day_start), Some(day_end), 20)?
            .into_iter()
            .filter(is_actionable_idle_block)
            .collect::<Vec<_>>();
        let source_events = self.list_today_source_events(10_000)?;
        let calendar_reconciliation =
            build_calendar_reconciliation(&calendar_events, &work_sessions, &source_events);
        let focus_sessions =
            self.list_focus_sessions_between(Some(day_start), Some(day_end), 100)?;
        let recovery_events =
            self.list_recovery_events_between(Some(day_start), Some(day_end), 200)?;
        let settings = self.get_settings()?;
        let recovery_threshold_ms = configured_recovery_threshold_ms(&settings);
        let recovery_summary = build_recovery_summary(
            &source_events,
            &recovery_events,
            Some(day_start),
            Some(day_end),
            recovery_threshold_ms,
        );
        let ai_usage = self.list_ai_usage_between(Some(day_start), Some(day_end), 1_000)?;
        let ai_usage_summary = build_ai_usage_summary(&source_events, &ai_usage, ai_outputs.len());
        let app_usage_summary = build_app_usage_summary(&source_events);
        let daily_goals = self.list_daily_goals().unwrap_or_default();
        let goal_progress = self.build_goal_progress(&daily_goals, &source_events);
        let git_commits: Vec<crate::models::GitCommit> = source_events
            .iter()
            .filter(|e| e.event_type == "git_commit")
            .map(|e| {
                let meta: serde_json::Value = e
                    .metadata_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(serde_json::Value::Null);
                crate::models::GitCommit {
                    id: e.id.clone(),
                    message: e.title.clone().unwrap_or_default(),
                    repo: meta
                        .get("git_repo")
                        .and_then(|v| v.as_str())
                        .or(e.workspace_key.as_deref())
                        .unwrap_or_default()
                        .to_string(),
                    branch: meta.get("git_branch").and_then(|v| v.as_str()).map(str::to_string),
                    captured_at: e.started_at,
                }
            })
            .collect();
        let thirty_days_ms = 30_i64 * 24 * 60 * 60 * 1_000;
        let streak_threshold_ms = 30_i64 * 60 * 1_000; // 30 min
        let streak_summary = self
            .build_streak_summary(day_end - thirty_days_ms, day_end, streak_threshold_ms)
            .unwrap_or(crate::models::StreakSummary {
                current_streak_days: 0,
                longest_streak_days: 0,
                avg_daily_ms: 0,
                active_days_30: 0,
                threshold_ms: streak_threshold_ms,
            });
        let automation_candidates = build_automation_candidates(&source_events);
        let inferred_work_blocks = build_inferred_work_blocks(
            &source_events,
            &calendar_events,
            Some(day_start),
            Some(day_end),
        );
        let loop_risks = self.detect_loop_risks()?;
        let hidden_loop_ids = self.hidden_loop_ids()?;
        let pause_state = self.pause_state()?;
        let next_best_action = self.next_best_action()?;
        let required_permissions_granted =
            crate::permissions::capture_permission_summary().all_required_granted;
        let watcher_heartbeat = crate::active_window::watcher_heartbeat();
        let capture_liveness = crate::active_window::assess_capture_liveness(
            watcher_heartbeat.as_ref(),
            now_ms(),
            CAPTURE_STALE_AFTER_MS,
        );
        let capture_health = build_capture_health_with_permission_state(
            &source_events,
            &settings,
            &pause_state,
            required_permissions_granted,
            capture_liveness,
            watcher_heartbeat.as_ref(),
        );
        let unclosed_loop_inbox = build_unclosed_loop_inbox(
            &tasks,
            &pending_replies,
            &commitments,
            &ai_outputs,
            &meetings,
            &field_visits,
            &idle_blocks,
            &loop_risks,
            &hidden_loop_ids,
        );
        let ai_output_ledger = build_ai_output_ledger(&source_events, &ai_outputs);
        let menu_bar_summary = build_menu_bar_summary(
            &source_events,
            &work_sessions,
            &ai_usage_summary,
            &unclosed_loop_inbox,
            &pause_state,
            next_best_action.as_ref(),
        );

        Ok(TodaySnapshot {
            local_date,
            tasks,
            quick_notes,
            commitments,
            pending_replies,
            ai_outputs,
            calendar_events,
            calendar_reconciliation,
            focus_sessions,
            recovery_summary,
            meetings,
            field_visits,
            idle_blocks,
            source_events,
            work_sessions,
            parallel_streams,
            ai_usage_summary,
            app_usage_summary,
            automation_candidates,
            inferred_work_blocks,
            capture_health,
            unclosed_loop_inbox,
            ai_output_ledger,
            menu_bar_summary,
            loop_risks,
            next_best_action,
            pause_state,
            settings,
            project_context: detect_project_from_sources(default_project_sources()).ok(),
            active_work_context: self.get_active_work_context().ok().flatten(),
            goal_progress,
            git_commits,
            streak_summary,
        })
    }

    pub fn materialize_work_memory(&self) -> Result<WorkMemorySummary> {
        let settings = self.get_settings()?;
        let idle_gap_ms = settings.idle_timeout_minutes.max(1) * 60_000;
        let fingerprint = {
            let conn = self.lock()?;
            let fingerprint = source_event_materialization_fingerprint_locked(&conn, idle_gap_ms)?;
            if materialization_state_locked(&conn)?.as_ref() == Some(&fingerprint) {
                return work_memory_summary_locked(&conn, fingerprint.source_event_count);
            }
            fingerprint
        };
        let events = self.list_source_events_for_materialization()?;

        let sessions = build_sessions_from_source_events(&events, idle_gap_ms);
        let streams = build_streams_from_source_events(&events);
        let conn = self.lock()?;
        let now = now_ms();

        for session in &sessions {
            conn.execute(
                r#"
                INSERT INTO work_sessions
                    (id, title, project_id, context_id, category, status, started_at, ended_at,
                     duration_ms, ai_used, confidence, summary, evidence_json, user_corrected,
                     created_at, updated_at)
                VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, ?13, ?13)
                ON CONFLICT(id) DO UPDATE SET
                    title = excluded.title,
                    context_id = excluded.context_id,
                    category = excluded.category,
                    status = excluded.status,
                    started_at = excluded.started_at,
                    ended_at = excluded.ended_at,
                    duration_ms = excluded.duration_ms,
                    ai_used = excluded.ai_used,
                    confidence = excluded.confidence,
                    summary = excluded.summary,
                    evidence_json = excluded.evidence_json,
                    updated_at = excluded.updated_at
                WHERE work_sessions.title IS NOT excluded.title
                   OR work_sessions.context_id IS NOT excluded.context_id
                   OR work_sessions.category IS NOT excluded.category
                   OR work_sessions.status IS NOT excluded.status
                   OR work_sessions.started_at IS NOT excluded.started_at
                   OR work_sessions.ended_at IS NOT excluded.ended_at
                   OR work_sessions.duration_ms IS NOT excluded.duration_ms
                   OR work_sessions.ai_used IS NOT excluded.ai_used
                   OR work_sessions.confidence IS NOT excluded.confidence
                   OR work_sessions.summary IS NOT excluded.summary
                   OR work_sessions.evidence_json IS NOT excluded.evidence_json
                "#,
                params![
                    &session.id,
                    &session.title,
                    &session.context_id,
                    &session.category,
                    &session.status,
                    session.started_at,
                    session.ended_at,
                    session.duration_ms,
                    if session.ai_used { 1 } else { 0 },
                    session.confidence,
                    &session.summary,
                    serde_json::to_string(&session.evidence_event_ids)?,
                    now,
                ],
            )?;

            for event_id in &session.evidence_event_ids {
                let edge_id = format!("edge-{}-{}", session.id, event_id);
                conn.execute(
                    r#"
                    INSERT INTO work_graph_edges
                        (id, from_type, from_id, to_type, to_id, relation, confidence, evidence_json, created_at)
                    VALUES (?1, 'work_session', ?2, 'source_event', ?3, 'session_contains_event', ?4, ?5, ?6)
                    ON CONFLICT(id) DO UPDATE SET
                        confidence = excluded.confidence,
                        evidence_json = excluded.evidence_json
                    WHERE work_graph_edges.confidence IS NOT excluded.confidence
                       OR work_graph_edges.evidence_json IS NOT excluded.evidence_json
                    "#,
                    params![
                        edge_id,
                        &session.id,
                        event_id,
                        session.confidence,
                        serde_json::to_string(&vec![event_id])?,
                        now,
                    ],
                )?;
            }
        }

        for stream in &streams {
            conn.execute(
                r#"
                INSERT INTO parallel_streams
                    (id, title, stream_type, project_id, context_id, started_at, ended_at,
                     summary, confidence, created_at, updated_at)
                VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
                ON CONFLICT(id) DO UPDATE SET
                    title = excluded.title,
                    stream_type = excluded.stream_type,
                    context_id = excluded.context_id,
                    started_at = excluded.started_at,
                    ended_at = excluded.ended_at,
                    summary = excluded.summary,
                    confidence = excluded.confidence,
                    updated_at = excluded.updated_at
                WHERE parallel_streams.title IS NOT excluded.title
                   OR parallel_streams.stream_type IS NOT excluded.stream_type
                   OR parallel_streams.context_id IS NOT excluded.context_id
                   OR parallel_streams.started_at IS NOT excluded.started_at
                   OR parallel_streams.ended_at IS NOT excluded.ended_at
                   OR parallel_streams.summary IS NOT excluded.summary
                   OR parallel_streams.confidence IS NOT excluded.confidence
                "#,
                params![
                    &stream.id,
                    &stream.title,
                    &stream.stream_type,
                    &stream.context_id,
                    stream.started_at,
                    stream.ended_at,
                    &stream.summary,
                    stream.confidence,
                    now,
                ],
            )?;

            for event_id in &stream.event_ids {
                conn.execute(
                    r#"
                    INSERT INTO stream_events (stream_id, event_id, confidence)
                    VALUES (?1, ?2, ?3)
                    ON CONFLICT(stream_id, event_id) DO UPDATE SET
                        confidence = excluded.confidence
                    WHERE stream_events.confidence IS NOT excluded.confidence
                    "#,
                    params![&stream.id, event_id, stream.confidence],
                )?;
            }
        }

        upsert_materialization_state_locked(&conn, &fingerprint, now)?;
        let graph_edges = session_graph_edge_count_locked(&conn)?;

        Ok(WorkMemorySummary {
            source_events: events.len(),
            work_sessions: sessions.len(),
            parallel_streams: streams.len(),
            graph_edges,
        })
    }

    pub fn ingest_local_bridge_files(&self) -> Result<usize> {
        let mut stored = 0;

        for path in self.editor_bridge_paths()? {
            stored += self.ingest_editor_bridge_file(&path)?;
        }

        for path in self.terminal_bridge_paths()? {
            if let Some(metadata) = read_terminal_bridge_metadata(&path)? {
                self.ingest_terminal_bridge_metadata(metadata)?;
                stored += 1;
            }
        }

        Ok(stored)
    }

    pub fn ingest_editor_bridge_file(&self, path: impl AsRef<Path>) -> Result<usize> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(0);
        }

        let path_key = path.display().to_string();
        let mut file = fs::File::open(path)
            .with_context(|| format!("failed to open editor bridge {}", path.display()))?;
        let file_len = file.metadata()?.len();
        let mut bytes_read = self.bridge_cursor(&path_key)?.max(0) as u64;
        if bytes_read > file_len {
            bytes_read = 0;
        }
        file.seek(SeekFrom::Start(bytes_read))?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .with_context(|| format!("failed to read editor bridge {}", path.display()))?;
        if contents.trim().is_empty() {
            self.set_bridge_cursor(&path_key, file_len)?;
            return Ok(0);
        }

        let mut stored = 0;
        let complete_len = contents.len();

        for line in contents[..complete_len].lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line)
                .with_context(|| format!("invalid editor bridge JSON in {}", path.display()))?;
            stored += self.ingest_editor_bridge_value(value)?;
        }

        self.set_bridge_cursor(&path_key, bytes_read + complete_len as u64)?;
        Ok(stored)
    }

    pub fn ingest_editor_bridge_value(&self, value: Value) -> Result<usize> {
        let message_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match message_type {
            "worktrace.editor_context_batch" => {
                let events = value
                    .get("events")
                    .and_then(Value::as_array)
                    .context("editor bridge batch events are required")?;
                let mut stored = 0;
                for event in events {
                    if self.ingest_editor_context_event(event.clone())? {
                        stored += 1;
                    }
                }
                Ok(stored)
            }
            "worktrace.editor_context" => self.ingest_editor_context_event(value).map(usize::from),
            _ => Ok(0),
        }
    }

    pub fn ingest_terminal_bridge_metadata(&self, metadata: TerminalBridgeMetadata) -> Result<()> {
        if self.pause_state()?.paused {
            return Ok(());
        }
        let cwd = metadata.cwd.trim();
        if cwd.is_empty() {
            return Ok(());
        }
        let settings = self.get_settings()?;
        if is_project_excluded(cwd, &settings.excluded_projects) {
            return Ok(());
        }

        let app = terminal_bridge_app_label(&metadata);
        if is_excluded(&app, &settings.excluded_apps) {
            return Ok(());
        }

        let captured_at = metadata
            .updated_at
            .as_deref()
            .and_then(parse_rfc3339_ms)
            .unwrap_or_else(now_ms);
        let event_type = metadata
            .event_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("terminal_context");
        let title = metadata
            .last_command
            .as_deref()
            .map(redact_terminal_command)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| cwd.to_string());
        let mut sanitized_metadata = metadata.clone();
        sanitized_metadata.terminal = Some(app.clone());
        sanitized_metadata.last_command = metadata
            .last_command
            .as_deref()
            .map(redact_terminal_command);
        let metadata_json = serde_json::to_string(&sanitized_metadata)?;
        self.record_source_event(SourceEventInput {
            id: None,
            source: "terminal-bridge".into(),
            event_type: event_type.to_string(),
            app: Some(app.clone()),
            title: Some(title.clone()),
            url: None,
            workspace_key: Some(cwd.to_string()),
            started_at: Some(captured_at),
            ended_at: Some(captured_at),
            sensitivity: Some("normal".into()),
            metadata_json: Some(metadata_json.clone()),
        })?;
        // Extra event for git commits so they can be surfaced in the timeline
        if let Some(commit_msg) = extract_git_commit_message(metadata.last_command.as_deref()) {
            let git_commit_json = serde_json::json!({
                "commit_message": commit_msg,
                "cwd": cwd,
                "git_branch": metadata.git_branch,
                "git_repo": metadata.git_repo,
            })
            .to_string();
            self.record_source_event(SourceEventInput {
                id: None,
                source: "terminal-bridge".into(),
                event_type: "git_commit".to_string(),
                app: Some(app.clone()),
                title: Some(commit_msg),
                url: None,
                workspace_key: Some(cwd.to_string()),
                started_at: Some(captured_at),
                ended_at: Some(captured_at),
                sensitivity: Some("normal".into()),
                metadata_json: Some(git_commit_json),
            })?;
        }
        self.record_activity(
            event_type,
            Some(&app),
            Some(&title),
            None,
            Some(cwd),
            Some(&metadata_json),
        )
    }

    pub fn upsert_workspace_context(
        &self,
        context_key: &str,
        context_type: &str,
        label: Option<&str>,
        folder_path: Option<&str>,
        domain: Option<&str>,
        metadata_json: Option<&str>,
    ) -> Result<WorkspaceContext> {
        let context_key = context_key.trim();
        anyhow::ensure!(!context_key.is_empty(), "workspace context key is required");
        let context_type = context_type.trim();
        anyhow::ensure!(
            !context_type.is_empty(),
            "workspace context type is required"
        );

        let conn = self.lock()?;
        let id = Self::upsert_workspace_context_locked(
            &conn,
            context_key,
            context_type,
            label,
            folder_path,
            domain,
            metadata_json,
        )?;
        Self::workspace_context_by_id_locked(&conn, &id)
    }

    pub fn add_scratchpad_note(&self, input: ScratchpadNoteInput) -> Result<ScratchpadNote> {
        let context_id = input.context_id.trim();
        anyhow::ensure!(!context_id.is_empty(), "scratchpad context_id is required");
        let note = input.note.trim();
        anyhow::ensure!(!note.is_empty(), "scratchpad note is required");
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("scratchpad-{}", Utc::now().timestamp_micros()));

        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO scratchpad_notes (id, context_id, note, pinned, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?5)
            "#,
            params![
                &id,
                context_id,
                note,
                if input.pinned.unwrap_or(false) { 1 } else { 0 },
                now,
            ],
        )?;
        Self::scratchpad_note_by_id_locked(&conn, &id)
    }

    pub fn list_scratchpad_notes(
        &self,
        context_id: &str,
        limit: usize,
    ) -> Result<Vec<ScratchpadNote>> {
        let context_id = context_id.trim();
        anyhow::ensure!(!context_id.is_empty(), "scratchpad context_id is required");
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, context_id, note, pinned, created_at, updated_at
            FROM scratchpad_notes
            WHERE context_id = ?1
            ORDER BY pinned DESC, updated_at DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(
            params![context_id, limit as i64],
            Self::scratchpad_note_from_row,
        )?;
        let mut notes = Vec::new();
        for note in rows {
            notes.push(note?);
        }
        Ok(notes)
    }

    pub fn create_state_snapshot(&self, input: StateSnapshotInput) -> Result<StateSnapshot> {
        let context_id = input.context_id.trim();
        anyhow::ensure!(!context_id.is_empty(), "snapshot context_id is required");
        let trigger_type = input.trigger_type.trim();
        anyhow::ensure!(
            !trigger_type.is_empty(),
            "snapshot trigger_type is required"
        );
        let snapshot_type = input.snapshot_type.trim();
        anyhow::ensure!(
            !snapshot_type.is_empty(),
            "snapshot snapshot_type is required"
        );
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("snapshot-{}", Utc::now().timestamp_micros()));

        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO state_snapshots
                (id, context_id, trigger_type, snapshot_type, summary, terminal_tail,
                 git_diff_summary, active_file, cursor_position, ai_context_summary,
                 metadata_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
            params![
                &id,
                context_id,
                trigger_type,
                snapshot_type,
                &input.summary,
                &input.terminal_tail,
                &input.git_diff_summary,
                &input.active_file,
                &input.cursor_position,
                &input.ai_context_summary,
                &input.metadata_json,
                now,
            ],
        )?;
        Self::state_snapshot_by_id_locked(&conn, &id)
    }

    pub fn get_return_marker(&self, context_id: &str) -> Result<ReturnMarker> {
        let context_id = context_id.trim();
        anyhow::ensure!(
            !context_id.is_empty(),
            "return marker context_id is required"
        );
        let conn = self.lock()?;
        let context = conn
            .query_row(
                r#"
                SELECT id, context_key, context_type, label, git_repo, git_branch, folder_path,
                       domain, email_thread_id, project_id, last_seen_at, metadata_json,
                       created_at, updated_at
                FROM workspace_contexts
                WHERE id = ?1
                "#,
                params![context_id],
                Self::workspace_context_from_row,
            )
            .optional()?;
        let latest_snapshot = Self::latest_state_snapshot_locked(&conn, context_id)?;
        let pinned_notes = Self::scratchpad_notes_locked(&conn, context_id, true, 10)?;
        let recent_notes = Self::scratchpad_notes_locked(&conn, context_id, false, 10)?;
        let mut recent_sessions = Vec::new();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, status, started_at, ended_at, duration_ms, ai_used,
                   confidence, summary, evidence_json, billing_status, billable,
                   client_label, project_label, ticket_id, review_notes
            FROM work_sessions
            WHERE context_id = ?1
            ORDER BY ended_at DESC
            LIMIT 5
            "#,
        )?;
        for session in stmt.query_map(params![context_id], Self::work_session_from_row)? {
            recent_sessions.push(session?);
        }

        let suggested_next_action = latest_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.summary.as_ref())
            .map(|summary| format!("Resume from: {}", clean_report_text(summary)))
            .or_else(|| {
                pinned_notes
                    .first()
                    .map(|note| format!("Review pinned note: {}", clean_report_text(&note.note)))
            });

        Ok(ReturnMarker {
            context_id: context_id.to_string(),
            context,
            latest_snapshot,
            pinned_notes,
            recent_notes,
            recent_sessions,
            suggested_next_action,
        })
    }

    pub fn export_data(&self) -> Result<ExportPayload> {
        self.export_data_range(ExportRangeInput::default())
    }

    pub fn export_data_range(&self, range: ExportRangeInput) -> Result<ExportPayload> {
        let _ = self.materialize_work_memory();
        let (from_ms, to_ms) = export_range_bounds(&range)?;
        let source_events = self.list_source_events_between(from_ms, to_ms, 10_000)?;
        let work_sessions = self.list_work_sessions_between(from_ms, to_ms, 2_000)?;
        let idle_blocks = self
            .list_idle_blocks_between(from_ms, to_ms, 2_000)?
            .into_iter()
            .filter(is_actionable_idle_block)
            .collect::<Vec<_>>();
        let ai_usage = self.list_ai_usage_between(from_ms, to_ms, 2_000)?;
        let ai_outputs =
            filter_work_outputs_by_range(self.list_work_outputs(1000)?, from_ms, to_ms);
        let calendar_events = self.list_calendar_events_between(from_ms, to_ms, 2_000)?;
        let calendar_reconciliation =
            build_calendar_reconciliation(&calendar_events, &work_sessions, &source_events);
        let focus_sessions = self.list_focus_sessions_between(from_ms, to_ms, 2_000)?;
        let recovery_events = self.list_recovery_events_between(from_ms, to_ms, 2_000)?;
        let settings = self.get_settings()?;
        let recovery_summary = build_recovery_summary(
            &source_events,
            &recovery_events,
            from_ms,
            to_ms,
            configured_recovery_threshold_ms(&settings),
        );
        let ai_usage_summary = build_ai_usage_summary(&source_events, &ai_usage, ai_outputs.len());
        let app_usage_summary = build_app_usage_summary(&source_events);
        let automation_candidates = build_automation_candidates(&source_events);
        let inferred_work_blocks =
            build_inferred_work_blocks(&source_events, &calendar_events, from_ms, to_ms);
        let loop_risks = if range_unbounded(from_ms, to_ms) {
            self.detect_loop_risks()?
        } else {
            Vec::new()
        };
        let tasks = filter_tasks_by_range(self.list_tasks(None)?, from_ms, to_ms);
        let quick_notes = filter_quick_notes_by_range(self.list_quick_notes(1000)?, from_ms, to_ms);
        let commitments =
            filter_commitments_by_range(self.list_open_commitments(1000)?, from_ms, to_ms);
        let pending_replies =
            filter_pending_replies_by_range(self.list_pending_replies(1000)?, from_ms, to_ms);
        let meetings = filter_meetings_by_range(self.list_meetings(1000)?, from_ms, to_ms);
        let field_visits =
            filter_field_visits_by_range(self.list_field_visits(1000)?, from_ms, to_ms);
        let timesheet_rows = build_timesheet_rows(&work_sessions, &source_events, from_ms, to_ms);
        let ai_contribution_rows =
            build_ai_contribution_rows(&source_events, &ai_outputs, &ai_usage);
        let hidden_loop_ids = self.hidden_loop_ids()?;
        let unclosed_loop_inbox = build_unclosed_loop_inbox(
            &tasks,
            &pending_replies,
            &commitments,
            &ai_outputs,
            &meetings,
            &field_visits,
            &idle_blocks,
            &loop_risks,
            &hidden_loop_ids,
        );

        Ok(ExportPayload {
            generated_at: now_utc(),
            from_date: range.from_date,
            to_date: range.to_date,
            timesheet_rows,
            ai_contribution_rows,
            calendar_events,
            calendar_reconciliation,
            focus_sessions,
            recovery_summary,
            recovery_events,
            tasks,
            quick_notes,
            commitments,
            pending_replies,
            outputs: ai_outputs,
            source_events,
            work_sessions,
            idle_blocks,
            ai_usage,
            app_usage_summary,
            ai_usage_summary,
            automation_candidates,
            inferred_work_blocks,
            unclosed_loop_inbox,
            settings: self.get_settings()?,
            pause_state: self.pause_state()?,
            project_context: detect_project_from_sources(default_project_sources()).ok(),
            active_work_context: self.get_active_work_context().ok().flatten(),
        })
    }

    pub fn analyze_export_range(&self, range: ExportRangeInput) -> Result<ReportOutput> {
        let export = self.export_data_range(range)?;
        let generated_at = now_utc();
        let title = "DayTrail Routine and Automation Analysis".to_string();
        let deterministic_markdown = build_export_analysis_markdown(&export);
        let context = serde_json::to_string_pretty(&export)?;
        let ai_markdown = self.try_generate_ai_markdown(
            "DayTrail routine analysis",
            "Analyze this DayTrail raw activity export. Find routine tasks, repeated app/project flows, AI-assisted work patterns, missed loops, and practical automation opportunities. Do not invent facts.",
            &context,
            &SystemKeychain,
        );
        let used_ai = ai_markdown.is_some();
        let body_markdown = ai_markdown.unwrap_or(deterministic_markdown);
        let fallback_reason = (!used_ai).then(|| {
            "Configured AI was unavailable, so DayTrail generated a deterministic source-backed analysis."
                .to_string()
        });
        let report_type = "automation_analysis".to_string();
        let id = format!("automation-analysis-{}", Utc::now().timestamp_micros());

        let conn = self.lock()?;
        let now = now_ms();
        conn.execute(
            r#"
            INSERT INTO reports (
                id, report_type, title, body_markdown, content_markdown,
                generated_at, metadata_json, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?5, ?5)
            "#,
            params![
                id,
                report_type,
                title,
                body_markdown,
                now,
                serde_json::to_string(&export)?
            ],
        )?;

        Ok(ReportOutput {
            generated_at,
            report_type,
            title,
            body_markdown,
            used_ai,
            fallback_reason,
        })
    }

    pub fn generate_daily_report(&self) -> Result<ReportOutput> {
        let mut snapshot = self.today_snapshot()?;
        snapshot.tasks = self.list_tasks(None)?;
        let generated_at = now_utc();
        let title = format!("Daily Work Execution Report - {}", snapshot.local_date);
        let include_system_apps =
            snapshot.settings.experience_mode == "pro" || snapshot.settings.show_system_apps;
        let deterministic_markdown = build_daily_report_markdown(&snapshot, include_system_apps);
        let ai_markdown = self.try_generate_ai_markdown(
            "End-of-day work review",
            "Rewrite this DayTrail factual report into a concise executive markdown report. Do not invent facts. Keep all source-backed tasks, commitments, reply debt, AI deliverables, and risks.",
            &deterministic_markdown,
            &SystemKeychain,
        );
        let used_ai = ai_markdown.is_some();
        let body_markdown = ai_markdown.unwrap_or(deterministic_markdown);
        let fallback_reason = (!used_ai).then(|| {
            "Configured AI was unavailable, so DayTrail generated a deterministic source-backed report."
                .to_string()
        });
        let report_type = "daily".to_string();
        let id = format!("report-{}", Utc::now().timestamp_micros());

        let conn = self.lock()?;
        let now = now_ms();
        conn.execute(
            r#"
            INSERT INTO reports (
                id, report_type, title, body_markdown, content_markdown,
                generated_at, metadata_json, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?5, ?5)
            "#,
            params![
                id,
                report_type,
                title,
                body_markdown,
                now,
                serde_json::to_string(&snapshot)?
            ],
        )?;

        Ok(ReportOutput {
            generated_at,
            report_type,
            title,
            body_markdown,
            used_ai,
            fallback_reason,
        })
    }

    pub fn generate_morning_plan(&self) -> Result<PlanningOutput> {
        self.generate_plan("today")
    }

    pub fn generate_weekly_plan(&self) -> Result<PlanningOutput> {
        self.generate_plan("week")
    }

    pub fn generate_weekly_review(&self) -> Result<ReportOutput> {
        let today = Local::now().date_naive();
        let from_date = today
            .checked_sub_signed(ChronoDuration::days(6))
            .unwrap_or(today)
            .format("%Y-%m-%d")
            .to_string();
        let to_date = today.format("%Y-%m-%d").to_string();
        let export = self.export_data_range(ExportRangeInput {
            from_date: Some(from_date.clone()),
            to_date: Some(to_date.clone()),
        })?;
        let generated_at = now_utc();
        let title = format!("Weekly Work Review - {from_date} to {to_date}");
        let deterministic_markdown = build_weekly_review_markdown_from_export(&export);
        let ai_markdown = self.try_generate_ai_markdown(
            "AI weekly auto-draft",
            "Rewrite this DayTrail weekly evidence digest into a concise weekly update. Do not invent facts. Preserve calendar planned-vs-actual, focus drift, AI work, open loops, and source-backed accomplishments.",
            &deterministic_markdown,
            &SystemKeychain,
        );
        let used_ai = ai_markdown.is_some();
        let body_markdown = ai_markdown.unwrap_or(deterministic_markdown);
        let report_type = "weekly_review".to_string();
        let id = format!("weekly-review-{}", Utc::now().timestamp_micros());

        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO weekly_reviews (id, title, body_markdown, generated_at, metadata_json)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                id,
                title,
                body_markdown,
                now_ms(),
                serde_json::to_string(&export)?
            ],
        )?;

        Ok(ReportOutput {
            generated_at,
            report_type,
            title,
            body_markdown,
            used_ai,
            fallback_reason: (!used_ai).then(|| {
                "Configured AI was unavailable, so DayTrail generated a deterministic weekly evidence draft.".into()
            }),
        })
    }

    fn generate_plan(&self, horizon: &str) -> Result<PlanningOutput> {
        let mut snapshot = self.today_snapshot()?;
        snapshot.tasks = self.list_tasks(None)?;
        let generated_at = now_utc();
        let today = Local::now().date_naive();
        let end_date = if horizon == "week" {
            today + ChronoDuration::days(6)
        } else {
            today
        };
        let end_ms = horizon_end_ms(horizon);
        let mut must_close = Vec::new();
        let mut should_progress = Vec::new();
        let mut can_defer = Vec::new();
        let waiting = Vec::new();
        let mut at_risk = Vec::new();
        let plan_now_ms = now_ms();

        for task in &snapshot.tasks {
            let item = PlanningItem {
                title: task.title.clone(),
                source: task
                    .project_label
                    .clone()
                    .or_else(|| task.source.clone())
                    .unwrap_or_else(|| "task".into()),
                reason: task_due_reason(task),
                due_at: task
                    .due_at
                    .or_else(|| task.due_date.as_deref().and_then(local_date_to_epoch_ms)),
                priority: task_priority_rank(task),
            };

            if item.due_at.is_some_and(|due_at| due_at <= plan_now_ms)
                || task
                    .due_date
                    .as_deref()
                    .is_some_and(|date| date_is_on_or_before(date, end_date))
            {
                must_close.push(item);
            } else if task.due_date.is_some() {
                can_defer.push(item);
            } else {
                should_progress.push(item);
            }
        }

        for commitment in &snapshot.commitments {
            let overdue = commitment.due_at.is_some_and(|due_at| due_at <= now_ms());
            let due_in_horizon = commitment.due_at.is_some_and(|due_at| due_at <= end_ms);
            let item = PlanningItem {
                title: commitment.title.clone(),
                source: commitment
                    .source
                    .clone()
                    .unwrap_or_else(|| "commitment".into()),
                reason: if overdue {
                    "Due time has passed".into()
                } else if due_in_horizon {
                    format!("Due in {horizon} horizon")
                } else {
                    "Open commitment".into()
                },
                due_at: commitment.due_at,
                priority: if overdue { 1 } else { 2 },
            };

            if due_in_horizon {
                must_close.push(item.clone());
            } else {
                should_progress.push(item.clone());
            }
            if overdue {
                at_risk.push(item);
            }
        }

        for thread in &snapshot.pending_replies {
            let item = PlanningItem {
                title: format!("Reply to {}", thread.subject),
                source: thread
                    .latest_sender
                    .clone()
                    .unwrap_or_else(|| "inbox".into()),
                reason: "Latest sender is not you".into(),
                due_at: thread.latest_at,
                priority: 1,
            };
            must_close.push(item.clone());
            at_risk.push(item);
        }

        for output in &snapshot.ai_outputs {
            if output.status == "drafted" || output.status == "needs_review" {
                must_close.push(PlanningItem {
                    title: output.title.clone(),
                    source: output
                        .source
                        .clone()
                        .unwrap_or_else(|| output.output_type.clone()),
                    reason: format!("AI-assisted output is {}", output.status),
                    due_at: Some(output.updated_at),
                    priority: 2,
                });
            }
        }

        for block in &snapshot.idle_blocks {
            if !block.classified {
                at_risk.push(PlanningItem {
                    title: format!(
                        "Classify {}m idle block",
                        block.duration_ms.saturating_div(60_000).max(1)
                    ),
                    source: "idle recovery".into(),
                    reason: "Unclassified work gap can hide offline work".into(),
                    due_at: Some(block.ended_at),
                    priority: 2,
                });
            }
        }

        for stream in &snapshot.parallel_streams {
            if stream.status != "completed" {
                should_progress.push(PlanningItem {
                    title: stream.title.clone(),
                    source: "parallel stream".into(),
                    reason: stream
                        .next_action
                        .clone()
                        .or_else(|| stream.summary.clone())
                        .unwrap_or_else(|| "Active work stream".into()),
                    due_at: stream.ended_at,
                    priority: 3,
                });
            }
        }

        let capacity_summary = build_capacity_summary(&snapshot, horizon);
        let title = if horizon == "week" {
            format!("Weekly Plan - {}", snapshot.local_date)
        } else {
            format!("Morning Plan - {}", snapshot.local_date)
        };
        let body_markdown = build_plan_markdown(
            &title,
            &must_close,
            &should_progress,
            &can_defer,
            &waiting,
            &at_risk,
            &capacity_summary,
        );
        let output = PlanningOutput {
            generated_at,
            horizon: horizon.to_string(),
            title,
            body_markdown,
            must_close,
            should_progress,
            can_defer,
            waiting,
            at_risk,
            capacity_summary,
        };

        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO plans (id, horizon, title, body_markdown, generated_at, metadata_json)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                format!("plan-{horizon}-{}", Utc::now().timestamp_micros()),
                &output.horizon,
                &output.title,
                &output.body_markdown,
                now_ms(),
                serde_json::to_string(&output)?
            ],
        )?;

        Ok(output)
    }

    // ── Proactive insights ─────────────────────────────────────────────────

    pub fn generate_and_store_insights(&self) -> Result<Vec<crate::models::ProactiveInsight>> {
        let three_hours_ago = now_ms() - 3 * 60 * 60 * 1_000_i64;
        {
            let conn = self.lock()?;
            let recent: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proactive_insights WHERE generated_at > ?1",
                params![three_hours_ago],
                |r| r.get(0),
            )?;
            if recent > 0 {
                return Ok(vec![]);
            }
        }

        let settings = self.get_settings()?;
        let endpoint = settings.ai_endpoint.trim().to_string();
        let model = settings.ai_model.trim().to_string();
        if endpoint.is_empty() || model.is_empty() {
            return Ok(vec![]);
        }
        let api_key = settings
            .ai_api_key_ref
            .as_deref()
            .and_then(keychain_key_from_ref)
            .and_then(|k| SystemKeychain.keychain_get(k).ok().flatten());

        // Local endpoints (Ollama, LM Studio) don't require a key. All cloud
        // providers (Anthropic, OpenAI, Gemini, Groq…) do — skip generation
        // rather than waste a request that will return 401.
        let is_local_endpoint = ["localhost", "127.0.0.1", "::1"]
            .iter()
            .any(|h| settings.ai_endpoint.contains(h));
        if api_key.is_none() && !is_local_endpoint {
            return Ok(vec![]);
        }

        let snapshot = self.today_snapshot()?;
        let today = Local::now().date_naive();
        let from_date = (today - ChronoDuration::days(6)).format("%Y-%m-%d").to_string();
        let to_date = today.format("%Y-%m-%d").to_string();
        let week_context = self
            .export_data_range(ExportRangeInput { from_date: Some(from_date), to_date: Some(to_date) })
            .map(|e| build_compact_weekly_context(&e))
            .unwrap_or_default();

        let tasks = self.list_tasks(None).unwrap_or_default();
        let open_tasks: Vec<_> = tasks.iter().filter(|t| t.status == crate::models::TaskStatus::Open).collect();
        let overdue_count = {
            let now = now_ms();
            let today_str = Local::now().format("%Y-%m-%d").to_string();
            open_tasks.iter().filter(|t| {
                t.due_at.is_some_and(|d| d < now)
                    || t.due_date.as_deref().is_some_and(|d| d < today_str.as_str())
            }).count()
        };

        let commitments = self.list_open_commitments(50).unwrap_or_default();
        let commitment_summary = if commitments.is_empty() {
            String::new()
        } else {
            format!("Open commitments: {}", commitments.len())
        };

        let now_str = Local::now().format("%A, %B %d, %Y at %H:%M").to_string();
        let context = format!(
            "Today: {now_str}\n\n{today_snapshot}\n\n{week_context}\n\nOpen tasks: {open}, Overdue: {overdue}\n{commitments}",
            today_snapshot = build_compact_chat_snapshot(&snapshot),
            open = open_tasks.len(),
            overdue = overdue_count,
            commitments = commitment_summary,
        );

        let instruction = "You are DayTrail, an AI work-memory assistant. Analyze the following work data \
and return 1-3 proactive insights that are genuinely interesting and specific to this person's actual data.\n\n\
RULES:\n\
- Every insight MUST cite specific numbers, durations, or names from the data\n\
- Only flag things that are truly notable — skip generic productivity advice\n\
- Priority \"high\" = needs attention now, \"medium\" = interesting pattern, \"low\" = FYI\n\
- insight_type: one of: focus, loop, rhythm, commitment, ai_usage, productivity\n\
- action_hint: 1 short sentence suggesting what to do, or null\n\n\
Return ONLY a valid JSON array — no markdown fences, no other text:\n\
[{\"type\":\"...\",\"title\":\"...\",\"body\":\"...\",\"priority\":\"high|medium|low\",\"action_hint\":\"...or null\"}]";

        let raw = crate::llm::generate_text(
            &settings.ai_provider,
            &endpoint,
            &model,
            api_key.as_deref(),
            instruction,
            &context,
        )?;

        let json_str = raw.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(json_str)
            .unwrap_or_default();

        let now = now_ms();
        let mut stored = Vec::new();
        let conn = self.lock()?;
        for item in &parsed {
            let insight_type = item["type"].as_str().unwrap_or("productivity").to_string();
            let title = item["title"].as_str().unwrap_or("").to_string();
            let body = item["body"].as_str().unwrap_or("").to_string();
            let priority = item["priority"].as_str().unwrap_or("medium").to_string();
            let action_hint: Option<String> = item["action_hint"]
                .as_str()
                .filter(|s| !s.is_empty() && *s != "null")
                .map(str::to_string);

            if title.is_empty() || body.is_empty() {
                continue;
            }

            let id = conn.query_row(
                "INSERT INTO proactive_insights (insight_type, title, body, priority, action_hint, generated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
                params![insight_type, title, body, priority, action_hint, now],
                |r| r.get::<_, i64>(0),
            )?;

            stored.push(crate::models::ProactiveInsight {
                id,
                insight_type,
                title,
                body,
                priority,
                action_hint,
                generated_at: now,
                seen_at: None,
                dismissed_at: None,
            });
        }

        Ok(stored)
    }

    pub fn list_proactive_insights(&self) -> Result<Vec<crate::models::ProactiveInsight>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, insight_type, title, body, priority, action_hint, generated_at, seen_at, dismissed_at
             FROM proactive_insights
             WHERE dismissed_at IS NULL
             ORDER BY generated_at DESC
             LIMIT 30",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(crate::models::ProactiveInsight {
                id: r.get(0)?,
                insight_type: r.get(1)?,
                title: r.get(2)?,
                body: r.get(3)?,
                priority: r.get(4)?,
                action_hint: r.get(5)?,
                generated_at: r.get(6)?,
                seen_at: r.get(7)?,
                dismissed_at: r.get(8)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    pub fn dismiss_insight(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE proactive_insights SET dismissed_at = ?1 WHERE id = ?2",
            params![now_ms(), id],
        )?;
        Ok(())
    }

    pub fn mark_insights_seen(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE proactive_insights SET seen_at = ?1 WHERE seen_at IS NULL AND dismissed_at IS NULL",
            params![now_ms()],
        )?;
        Ok(())
    }

    pub fn count_unseen_insights(&self) -> Result<i64> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT COUNT(*) FROM proactive_insights WHERE seen_at IS NULL AND dismissed_at IS NULL",
            [],
            |r| r.get(0),
        ).map_err(Into::into)
    }

    // ── Chat ───────────────────────────────────────────────────────────────

    pub fn handle_chat_query(
        &self,
        message: &str,
        history: &[crate::models::ChatMessage],
    ) -> Result<crate::models::ChatResponse> {
        let (context, sources) = self.build_chat_context(message)?;

        let now_str = Local::now().format("%A, %B %d, %Y at %H:%M").to_string();
        let instruction = format!(
            "You are DayTrail's personal work assistant. DayTrail automatically tracks everything \
the user does on their computer — apps used, time spent, tasks, commitments, meetings, \
and AI tool usage.\n\n\
Answer using ONLY the data provided below. Rules:\n\
- Cite exact numbers, durations, app names, and task titles from the data\n\
- Compute totals and percentages when it helps answer the question\n\
- If the data isn't enough to answer, say what's missing and suggest a better question\n\
- Be concise: 2-5 sentences or a short markdown list\n\
- Never give generic productivity advice — everything must reference the actual data\n\n\
Today is {now_str}."
        );

        let mut full_context = String::new();

        if !history.is_empty() {
            full_context.push_str("## Previous messages in this conversation\n");
            for msg in history.iter().rev().take(8).rev() {
                let role = if msg.role == "user" { "You" } else { "DayTrail" };
                full_context.push_str(&format!("**{}:** {}\n\n", role, msg.content));
            }
            full_context.push_str("---\n\n");
        }

        full_context.push_str(&context);
        full_context.push_str(&format!("\n\n## Current question\n{message}"));

        let ai_answer = self.try_generate_ai_markdown(
            "chat",
            &instruction,
            &full_context,
            &SystemKeychain,
        );

        let used_ai = ai_answer.is_some();
        let message_out = ai_answer.unwrap_or_else(|| {
            "AI is not configured yet. Go to **Settings → AI Provider** to connect a model (Claude, GPT-4, Gemini, or a local Ollama model), and I'll be able to answer questions about your work data.".to_string()
        });

        Ok(crate::models::ChatResponse {
            message: message_out,
            data_sources: sources,
            used_ai,
        })
    }

    fn build_chat_data_coverage(&self) -> Result<Option<String>> {
        let conn = self.lock()?;
        let mut ranges: Vec<(&str, i64, i64, i64)> = Vec::new();

        let source_events: (Option<i64>, Option<i64>, i64) = conn.query_row(
            "SELECT MIN(started_at), MAX(ended_at), COUNT(*) FROM source_events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if let (Some(start), Some(end), count) = source_events {
            if count > 0 {
                ranges.push(("app/window/browser events", start, end, count));
            }
        }

        let work_sessions: (Option<i64>, Option<i64>, i64) = conn.query_row(
            "SELECT MIN(started_at), MAX(ended_at), COUNT(*) FROM work_sessions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if let (Some(start), Some(end), count) = work_sessions {
            if count > 0 {
                ranges.push(("work sessions", start, end, count));
            }
        }

        let ai_usage: (Option<i64>, Option<i64>, i64) = conn.query_row(
            "SELECT MIN(COALESCE(started_at, created_at)), MAX(COALESCE(ended_at, started_at, created_at)), COUNT(*) FROM ai_usage",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if let (Some(start), Some(end), count) = ai_usage {
            if count > 0 {
                ranges.push(("AI usage records", start, end, count));
            }
        }

        if ranges.is_empty() {
            return Ok(None);
        }

        let first = ranges.iter().map(|(_, start, _, _)| *start).min().unwrap_or(0);
        let last = ranges.iter().map(|(_, _, end, _)| *end).max().unwrap_or(0);
        let mut out = format!(
            "## Available data coverage\n\nDayTrail's local database currently has tracked data from {} to {}.\n",
            format_local_date_from_ms(first),
            format_local_date_from_ms(last)
        );
        for (label, start, end, count) in ranges {
            out.push_str(&format!(
                "- {}: {} records ({} to {})\n",
                label,
                count,
                format_local_date_from_ms(start),
                format_local_date_from_ms(end)
            ));
        }

        Ok(Some(out))
    }

    fn build_chat_context(&self, message: &str) -> Result<(String, Vec<String>)> {
        let msg = message.to_lowercase();
        let mut sections: Vec<String> = Vec::new();
        let mut sources: Vec<String> = Vec::new();
        let lookback = chat_lookback_for_message(message);

        let wants_tasks = msg.contains("task") || msg.contains("todo") || msg.contains(" due")
            || msg.contains("overdue") || msg.contains("priority") || msg.contains("backlog")
            || msg.contains("what should i") || msg.contains("next step")
            || msg.contains("focus on");

        let wants_loops = msg.contains("open loop") || msg.contains("outstanding")
            || msg.contains("pending") || msg.contains("commitment")
            || msg.contains("reply debt") || msg.contains("unfinished")
            || msg.contains("promised");

        let wants_billing = msg.contains("bill") || msg.contains("invoice")
            || msg.contains("charge") || msg.contains("client");

        if let Some(coverage) = self.build_chat_data_coverage()? {
            sections.push(coverage);
            sources.push("available data coverage".to_string());
        }

        // Always include today — it's compact and answers most questions.
        let snapshot = self.today_snapshot()?;
        sections.push(build_compact_chat_snapshot(&snapshot));
        sources.push("today's activity".to_string());

        let today = Local::now().date_naive();

        if lookback == ChatLookback::Quarter {
            let from_date = (today - ChronoDuration::days(89)).format("%Y-%m-%d").to_string();
            let to_date = today.format("%Y-%m-%d").to_string();
            if let Ok(export) = self.export_data_range(ExportRangeInput {
                from_date: Some(from_date.clone()),
                to_date: Some(to_date.clone()),
            }) {
                sections.push(build_compact_weekly_context(&export));
                sources.push(format!("last 90 days ({from_date} → {to_date})"));
            }
        } else if lookback == ChatLookback::Month {
            let from_date = (today - ChronoDuration::days(29)).format("%Y-%m-%d").to_string();
            let to_date = today.format("%Y-%m-%d").to_string();
            if let Ok(export) = self.export_data_range(ExportRangeInput {
                from_date: Some(from_date.clone()),
                to_date: Some(to_date.clone()),
            }) {
                sections.push(build_compact_weekly_context(&export));
                sources.push(format!("last 30 days ({from_date} → {to_date})"));
            }
        } else if lookback == ChatLookback::TwoWeeks {
            let from_date = (today - ChronoDuration::days(13)).format("%Y-%m-%d").to_string();
            let to_date = today.format("%Y-%m-%d").to_string();
            if let Ok(export) = self.export_data_range(ExportRangeInput {
                from_date: Some(from_date.clone()),
                to_date: Some(to_date.clone()),
            }) {
                sections.push(build_compact_weekly_context(&export));
                sources.push(format!("last 14 days ({from_date} → {to_date})"));
            }
        } else if lookback == ChatLookback::LastWeek {
            // Calendar week Mon–Sun that ended before this week.
            let days_since_monday = today.weekday().num_days_from_monday() as i64;
            let this_monday = today - ChronoDuration::days(days_since_monday);
            let last_monday = this_monday - ChronoDuration::days(7);
            let last_sunday = this_monday - ChronoDuration::days(1);
            let from_date = last_monday.format("%Y-%m-%d").to_string();
            let to_date = last_sunday.format("%Y-%m-%d").to_string();
            if let Ok(export) = self.export_data_range(ExportRangeInput {
                from_date: Some(from_date.clone()),
                to_date: Some(to_date.clone()),
            }) {
                sections.push(build_compact_weekly_context(&export));
                sources.push(format!("last week ({from_date} → {to_date})"));
            }
        } else if lookback == ChatLookback::Week {
            let from_date = (today - ChronoDuration::days(6)).format("%Y-%m-%d").to_string();
            let to_date = today.format("%Y-%m-%d").to_string();
            if let Ok(export) = self.export_data_range(ExportRangeInput {
                from_date: Some(from_date.clone()),
                to_date: Some(to_date.clone()),
            }) {
                sections.push(build_compact_weekly_context(&export));
                sources.push(format!("last 7 days ({from_date} → {to_date})"));
            }
        }

        if wants_tasks {
            let tasks = self.list_tasks(None)?;
            let open: Vec<_> = tasks
                .iter()
                .filter(|t| t.status == crate::models::TaskStatus::Open)
                .collect();
            if !open.is_empty() {
                let now = now_ms();
                let mut task_lines = vec!["## All Open Tasks".to_string()];
                for t in &open {
                    let today_str = Local::now().format("%Y-%m-%d").to_string();
                    let overdue = t.due_at.is_some_and(|d| d < now)
                        || t.due_date.as_deref().is_some_and(|d| d < today_str.as_str());
                    let project = t
                        .project_label
                        .as_deref()
                        .or(t.client_label.as_deref())
                        .map(|p| format!(" [{}]", p))
                        .unwrap_or_default();
                    let due = t
                        .due_date
                        .as_deref()
                        .map(|d| format!(" due:{}", d))
                        .unwrap_or_default();
                    task_lines.push(format!(
                        "- {}{}{}{}\n",
                        t.title,
                        project,
                        due,
                        if overdue { " **OVERDUE**" } else { "" }
                    ));
                }
                sections.push(task_lines.join("\n"));
                sources.push("tasks".to_string());
            }
        }

        if wants_loops {
            let commitments = self.list_open_commitments(20)?;
            if !commitments.is_empty() {
                let now = now_ms();
                let mut lines = vec!["## Open Commitments".to_string()];
                for c in &commitments {
                    let overdue = c.due_at.is_some_and(|d| d < now);
                    lines.push(format!(
                        "- {}{}\n",
                        c.title,
                        if overdue { " **OVERDUE**" } else { "" }
                    ));
                }
                sections.push(lines.join("\n"));
                sources.push("commitments".to_string());
            }

            if !snapshot.loop_risks.is_empty() {
                let mut lines = vec!["## Open Loop Risks".to_string()];
                for r in snapshot.loop_risks.iter().take(10) {
                    lines.push(format!("- {} [{}]: {}\n", r.title, r.risk_type, r.reason));
                }
                sections.push(lines.join("\n"));
                if !sources.contains(&"loop risks".to_string()) {
                    sources.push("loop risks".to_string());
                }
            }
        }

        if wants_billing {
            let billable: Vec<_> = snapshot
                .work_sessions
                .iter()
                .filter(|s| s.billable)
                .collect();
            if !billable.is_empty() {
                let mut lines = vec!["## Billable Sessions (Today)".to_string()];
                let total_ms: i64 = billable.iter().map(|s| s.duration_ms.max(0)).sum();
                for s in &billable {
                    let client = s
                        .client_label
                        .as_deref()
                        .unwrap_or("no client");
                    lines.push(format!(
                        "- {} | {} | {} | {}\n",
                        s.title,
                        client,
                        format_duration_words(s.duration_ms),
                        s.billing_status
                    ));
                }
                lines.push(format!("\nTotal billable today: {}", format_duration_words(total_ms)));
                sections.push(lines.join("\n"));
                if !sources.contains(&"billing data".to_string()) {
                    sources.push("billing data".to_string());
                }
            }
        }

        Ok((sections.join("\n\n"), sources))
    }

    pub fn search_work_memory(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let Some(fts_query) = build_fts_query(query) else {
            return Ok(Vec::new());
        };
        let limit = limit.clamp(1, 50) as i64;
        let conn = self.lock()?;
        Self::rebuild_work_memory_index_locked(&conn)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT entity_type,
                   entity_id,
                   title,
                   snippet(work_memory_fts, 3, '', '', '...', 12),
                   NULLIF(source, ''),
                   bm25(work_memory_fts) AS score
            FROM work_memory_fts
            WHERE work_memory_fts MATCH ?1
            ORDER BY score
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![fts_query, limit], |row| {
            Ok(SearchResult {
                entity_type: row.get(0)?,
                entity_id: row.get(1)?,
                title: row.get(2)?,
                snippet: row.get(3)?,
                source: row.get(4)?,
                score: row.get(5)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    fn rebuild_work_memory_index_locked(conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM work_memory_fts", [])?;

        {
            let mut stmt = conn.prepare(
                "SELECT id, title, source, project_path, created_at FROM tasks ORDER BY id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (id, title, source, project_path, created_at) = row?;
                let body = [source.as_deref(), project_path.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                Self::insert_search_document_locked(
                    conn,
                    "task",
                    &id.to_string(),
                    &title,
                    &body,
                    source.as_deref(),
                    &created_at,
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                "SELECT id, body, source, project_path, created_at FROM quick_notes ORDER BY id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (id, body, source, project_path, created_at) = row?;
                Self::insert_search_document_locked(
                    conn,
                    "quick_note",
                    &id.to_string(),
                    &first_words(&body, 8),
                    &body,
                    source.as_deref().or(project_path.as_deref()),
                    &created_at,
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                "SELECT id, title, source, owner, evidence_json, created_at FROM commitments",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?;
            for row in rows {
                let (id, title, source, owner, evidence, created_at) = row?;
                let body = [owner.as_deref(), evidence.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                Self::insert_search_document_locked(
                    conn,
                    "commitment",
                    &id,
                    &title,
                    &body,
                    source.as_deref(),
                    &created_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                "SELECT id, subject, latest_sender, evidence_json, created_at FROM email_threads",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?;
            for row in rows {
                let (id, subject, latest_sender, evidence, created_at) = row?;
                Self::insert_search_document_locked(
                    conn,
                    "email_thread",
                    &id,
                    &subject,
                    evidence.as_deref().unwrap_or(""),
                    latest_sender.as_deref(),
                    &created_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, title, status, summary, created_at
                FROM work_sessions
                ORDER BY created_at
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?;
            for row in rows {
                let (id, title, status, summary, created_at) = row?;
                Self::insert_search_document_locked(
                    conn,
                    "work_session",
                    &id,
                    &title,
                    summary.as_deref().unwrap_or(""),
                    status.as_deref(),
                    &created_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, title, app, domain, url_redacted, metadata_json, created_at
                FROM source_events
                ORDER BY created_at
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?;
            for row in rows {
                let (id, title, app, domain, url, metadata, created_at) = row?;
                let title = title.unwrap_or_else(|| {
                    domain
                        .clone()
                        .or_else(|| app.clone())
                        .unwrap_or_else(|| "Captured source event".into())
                });
                let body = [domain.as_deref(), url.as_deref(), metadata.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                Self::insert_search_document_locked(
                    conn,
                    "source_event",
                    &id,
                    &title,
                    &body,
                    app.as_deref(),
                    &created_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                "SELECT id, title, body_markdown, report_type, generated_at FROM reports",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?;
            for row in rows {
                let (id, title, body, report_type, generated_at) = row?;
                Self::insert_search_document_locked(
                    conn,
                    "report",
                    &id,
                    &title,
                    &body,
                    Some(&report_type),
                    &generated_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt =
                conn.prepare("SELECT id, title, body_markdown, generated_at FROM weekly_reviews")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                let (id, title, body, generated_at) = row?;
                Self::insert_search_document_locked(
                    conn,
                    "weekly_review",
                    &id,
                    &title,
                    &body,
                    Some("weekly_review"),
                    &generated_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, title, source, calendar_name, location, planned_work_type, starts_at
                FROM calendar_events
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?;
            for row in rows {
                let (id, title, source, calendar_name, location, planned_work_type, starts_at) =
                    row?;
                let body = [
                    calendar_name.as_deref(),
                    location.as_deref(),
                    planned_work_type.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
                Self::insert_search_document_locked(
                    conn,
                    "calendar_event",
                    &id,
                    &title,
                    &body,
                    Some(&source),
                    &starts_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, goal, client, project, task, ticket_id, started_at
                FROM focus_sessions
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?;
            for row in rows {
                let (id, goal, client, project, task, ticket_id, started_at) = row?;
                let body = [
                    client.as_deref(),
                    project.as_deref(),
                    task.as_deref(),
                    ticket_id.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
                Self::insert_search_document_locked(
                    conn,
                    "focus_session",
                    &id,
                    &goal,
                    &body,
                    project.as_deref(),
                    &started_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt =
                conn.prepare("SELECT id, title, summary, actions_json, created_at FROM meetings")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?;
            for row in rows {
                let (id, title, summary, actions, created_at) = row?;
                let body = [summary.as_deref(), actions.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                Self::insert_search_document_locked(
                    conn,
                    "meeting",
                    &id,
                    &title,
                    &body,
                    Some("meeting"),
                    &created_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                "SELECT id, output_type, title, source, evidence_json, created_at FROM outputs",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?;
            for row in rows {
                let (id, output_type, title, source, evidence, created_at) = row?;
                Self::insert_search_document_locked(
                    conn,
                    "work_output",
                    &id,
                    &title,
                    evidence.as_deref().unwrap_or(""),
                    source.as_deref().or(Some(output_type.as_str())),
                    &created_at.to_string(),
                )?;
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, provider, tool_name, thread_title, prompt_summary, output_summary, created_at
                FROM ai_usage
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?;
            for row in rows {
                let (id, provider, tool, thread_title, prompt, output, created_at) = row?;
                let title = thread_title
                    .or_else(|| prompt.clone())
                    .unwrap_or_else(|| "AI usage".to_string());
                let body = [prompt.as_deref(), output.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                Self::insert_search_document_locked(
                    conn,
                    "ai_usage",
                    &id,
                    &title,
                    &body,
                    tool.as_deref().or(provider.as_deref()),
                    &created_at.to_string(),
                )?;
            }
        }

        Ok(())
    }

    fn insert_search_document_locked(
        conn: &Connection,
        entity_type: &str,
        entity_id: &str,
        title: &str,
        body: &str,
        source: Option<&str>,
        created_at: &str,
    ) -> Result<()> {
        conn.execute(
            r#"
            INSERT INTO work_memory_fts (entity_type, entity_id, title, body, source, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                entity_type,
                entity_id,
                clean_report_text(title),
                clean_report_text(body),
                source.map(clean_report_text).unwrap_or_default(),
                created_at
            ],
        )?;
        Ok(())
    }

    pub fn record_ai_usage(&self, input: AiUsageInput) -> Result<AiUsage> {
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("ai-usage-{}", Utc::now().timestamp_micros()));
        anyhow::ensure!(
            input
                .tool_name
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
                || input
                    .thread_title
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()),
            "AI usage requires tool_name or thread_title"
        );

        let conn = self.lock()?;
        let columns = Self::table_columns(&conn, "ai_usage")?;
        let tool_value = input
            .tool_name
            .as_deref()
            .or(input.provider.as_deref())
            .or(input.thread_title.as_deref())
            .unwrap_or("AI")
            .trim()
            .to_string();
        let started_at = input.started_at.unwrap_or(now);
        let ended_at = input.ended_at.unwrap_or(started_at);
        let duration_ms = input
            .duration_ms
            .unwrap_or_else(|| ended_at.saturating_sub(started_at));
        let summary_value = input
            .prompt_summary
            .as_deref()
            .or(input.output_summary.as_deref())
            .or(input.thread_title.as_deref())
            .map(ToOwned::to_owned);
        let metadata_value = input.metadata_json.clone().or_else(|| {
            serde_json::to_string(&serde_json::json!({
                "provider": input.provider.as_deref(),
                "toolName": input.tool_name.as_deref(),
                "contextId": input.context_id.as_deref(),
            }))
            .ok()
        });

        let mut names = Vec::new();
        let mut values = Vec::new();
        macro_rules! push_value {
            ($column:expr, $value:expr) => {
                if $column == "id" || columns.contains_key($column) {
                    names.push($column.to_string());
                    values.push($value);
                }
            };
        }
        macro_rules! push_text {
            ($column:expr, $value:expr) => {
                push_value!(
                    $column,
                    $value
                        .filter(|item| !item.trim().is_empty())
                        .map(|item| SqlValue::Text(item.to_string()))
                        .unwrap_or(SqlValue::Null)
                );
            };
        }

        push_value!("id", SqlValue::Text(id.clone()));
        push_text!("provider", input.provider.as_deref());
        push_text!("tool_name", input.tool_name.as_deref());
        push_text!("thread_title", input.thread_title.as_deref());
        push_text!("context_id", input.context_id.as_deref());
        push_text!("prompt_summary", input.prompt_summary.as_deref());
        push_text!("output_summary", input.output_summary.as_deref());
        push_text!("tool", Some(tool_value.as_str()));
        push_text!("usage_category", Some("observed"));
        push_value!("started_at", SqlValue::Integer(started_at));
        push_value!("ended_at", SqlValue::Integer(ended_at));
        push_value!("duration_ms", SqlValue::Integer(duration_ms));
        push_text!("project_id", input.context_id.as_deref());
        push_value!("confidence", SqlValue::Real(1.0));
        push_text!("summary", summary_value.as_deref());
        push_text!("metadata_json", metadata_value.as_deref());
        push_value!("created_at", SqlValue::Integer(now));

        let placeholders = (1..=names.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let updates = names
            .iter()
            .filter(|name| name.as_str() != "id")
            .map(|name| match name.as_str() {
                "created_at" | "started_at" => {
                    format!("{name} = MIN(ai_usage.{name}, excluded.{name})")
                }
                "ended_at" | "duration_ms" => {
                    format!("{name} = MAX(ai_usage.{name}, excluded.{name})")
                }
                "confidence" => format!("{name} = MAX(ai_usage.{name}, excluded.{name})"),
                _ => format!("{name} = COALESCE(excluded.{name}, ai_usage.{name})"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "INSERT INTO ai_usage ({}) VALUES ({}) ON CONFLICT(id) DO UPDATE SET {}",
            names.join(", "),
            placeholders,
            updates
        );

        conn.execute(&sql, params_from_iter(values))?;
        Self::ai_usage_by_id_locked(&conn, &id)
    }

    pub fn list_ai_usage(&self, limit: usize) -> Result<Vec<AiUsage>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, provider, tool_name, thread_title, context_id, prompt_summary,
                   output_summary, started_at, ended_at, duration_ms, metadata_json, created_at
            FROM ai_usage
            ORDER BY created_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::ai_usage_from_row)?;
        let mut usage = Vec::new();
        for item in rows {
            usage.push(item?);
        }
        Ok(usage)
    }

    pub fn record_work_output(&self, input: WorkOutputInput) -> Result<WorkOutput> {
        let output_type = input.output_type.trim();
        anyhow::ensure!(!output_type.is_empty(), "output type is required");
        let title = input.title.trim();
        anyhow::ensure!(!title.is_empty(), "output title is required");
        let status = input.status.unwrap_or_else(|| "drafted".to_string());
        anyhow::ensure!(
            ["drafted", "needs_review", "sent", "shared", "archived"].contains(&status.as_str()),
            "unsupported output status: {status}"
        );
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("output-{}", Utc::now().timestamp_micros()));

        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO outputs
                (id, output_type, title, source, ai_assisted, status, evidence_json,
                 created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
            "#,
            params![
                &id,
                output_type,
                title,
                &input.source,
                if input.ai_assisted.unwrap_or(false) {
                    1
                } else {
                    0
                },
                status,
                &input.evidence_json,
                now,
            ],
        )?;
        Self::work_output_by_id_locked(&conn, &id)
    }

    pub fn list_work_outputs(&self, limit: usize) -> Result<Vec<WorkOutput>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, output_type, title, source, ai_assisted, status, evidence_json,
                   created_at, updated_at
            FROM outputs
            ORDER BY updated_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::work_output_from_row)?;
        let mut outputs = Vec::new();
        for output in rows {
            outputs.push(output?);
        }
        Ok(outputs)
    }

    pub fn upsert_meeting(&self, input: MeetingInput) -> Result<Meeting> {
        let title = input.title.trim();
        anyhow::ensure!(!title.is_empty(), "meeting title is required");
        if let (Some(start), Some(end)) = (input.starts_at, input.ends_at) {
            anyhow::ensure!(end >= start, "meeting ends_at must be after starts_at");
        }
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("meeting-{}", Utc::now().timestamp_micros()));
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO meetings
                (id, title, starts_at, ends_at, attendees_json, summary, actions_json,
                 created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                starts_at = excluded.starts_at,
                ends_at = excluded.ends_at,
                attendees_json = excluded.attendees_json,
                summary = excluded.summary,
                actions_json = excluded.actions_json,
                updated_at = excluded.updated_at
            "#,
            params![
                &id,
                title,
                input.starts_at,
                input.ends_at,
                &input.attendees_json,
                &input.summary,
                &input.actions_json,
                now,
            ],
        )?;
        Self::meeting_by_id_locked(&conn, &id)
    }

    pub fn list_meetings(&self, limit: usize) -> Result<Vec<Meeting>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, starts_at, ends_at, attendees_json, summary, actions_json,
                   created_at, updated_at
            FROM meetings
            ORDER BY COALESCE(starts_at, created_at) DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::meeting_from_row)?;
        let mut meetings = Vec::new();
        for meeting in rows {
            meetings.push(meeting?);
        }
        Ok(meetings)
    }

    pub fn upsert_calendar_event(&self, input: CalendarEventInput) -> Result<CalendarEvent> {
        let title = input.title.trim();
        anyhow::ensure!(!title.is_empty(), "calendar event title is required");
        anyhow::ensure!(
            input.ends_at >= input.starts_at,
            "calendar event ends_at must be after starts_at"
        );
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .or_else(|| {
                input
                    .external_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("calendar-{}", stable_id_part(value)))
            })
            .unwrap_or_else(|| format!("calendar-{}", Utc::now().timestamp_micros()));
        let source = input
            .source
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("manual")
            .to_string();
        let status = input
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("confirmed")
            .to_string();
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO calendar_events
                (id, source, external_id, calendar_name, title, starts_at, ends_at,
                 location, status, planned_work_type, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
            ON CONFLICT(id) DO UPDATE SET
                source = excluded.source,
                external_id = excluded.external_id,
                calendar_name = excluded.calendar_name,
                title = excluded.title,
                starts_at = excluded.starts_at,
                ends_at = excluded.ends_at,
                location = excluded.location,
                status = excluded.status,
                planned_work_type = excluded.planned_work_type,
                updated_at = excluded.updated_at
            "#,
            params![
                &id,
                source,
                &input.external_id,
                &input.calendar_name,
                title,
                input.starts_at,
                input.ends_at,
                &input.location,
                status,
                &input.planned_work_type,
                now,
            ],
        )?;
        Self::calendar_event_by_id_locked(&conn, &id)
    }

    pub fn list_calendar_events(&self, limit: usize) -> Result<Vec<CalendarEvent>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, source, external_id, calendar_name, title, starts_at, ends_at,
                   location, status, planned_work_type, created_at, updated_at
            FROM calendar_events
            ORDER BY starts_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::calendar_event_from_row)?;
        let mut events = Vec::new();
        for event in rows {
            events.push(event?);
        }
        Ok(events)
    }

    pub fn list_calendar_events_for_dates(
        &self,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<Vec<CalendarEvent>> {
        let (from_ms, to_ms) = date_range_to_ms(from_date, to_date)?;
        self.list_calendar_events_between(from_ms, to_ms, 2_000)
    }

    pub fn upsert_focus_session(&self, input: FocusSessionInput) -> Result<FocusSessionSummary> {
        let goal = input.goal.trim();
        anyhow::ensure!(!goal.is_empty(), "focus goal is required");
        anyhow::ensure!(input.target_ms > 0, "focus target_ms must be positive");
        if let Some(end) = input.ended_at {
            anyhow::ensure!(
                end >= input.started_at,
                "focus session ended_at must be after started_at"
            );
        }
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("focus-{}", Utc::now().timestamp_micros()));
        let status = input
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                if input.ended_at.is_some() {
                    "completed"
                } else {
                    "active"
                }
            })
            .to_string();
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO focus_sessions
                (id, goal, client, project, task, ticket_id, target_ms, started_at,
                 ended_at, status, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
            ON CONFLICT(id) DO UPDATE SET
                goal = excluded.goal,
                client = excluded.client,
                project = excluded.project,
                task = excluded.task,
                ticket_id = excluded.ticket_id,
                target_ms = excluded.target_ms,
                started_at = excluded.started_at,
                ended_at = excluded.ended_at,
                status = excluded.status,
                updated_at = excluded.updated_at
            "#,
            params![
                &id,
                goal,
                &input.client,
                &input.project,
                &input.task,
                &input.ticket_id,
                input.target_ms,
                input.started_at,
                input.ended_at,
                status,
                now,
            ],
        )?;
        let focus = Self::focus_session_base_by_id_locked(&conn, &id)?;
        drop(conn);
        let from_ms = Some(focus.started_at);
        let to_ms = Some(focus.ended_at.unwrap_or_else(now_ms));
        let events = self.list_source_events_between(from_ms, to_ms, 10_000)?;
        Ok(summarize_focus_session(&focus, &events, now_ms()))
    }

    pub fn list_focus_sessions(&self, limit: usize) -> Result<Vec<FocusSessionSummary>> {
        let bases = self.list_focus_session_bases_between(None, None, limit)?;
        let from_ms = bases.iter().map(|session| session.started_at).min();
        let to_ms = bases
            .iter()
            .map(|session| session.ended_at.unwrap_or_else(now_ms))
            .max();
        let events = self.list_source_events_between(from_ms, to_ms, 10_000)?;
        Ok(bases
            .iter()
            .map(|focus| summarize_focus_session(focus, &events, now_ms()))
            .collect())
    }

    pub fn list_focus_sessions_for_dates(
        &self,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<Vec<FocusSessionSummary>> {
        let (from_ms, to_ms) = date_range_to_ms(from_date, to_date)?;
        self.list_focus_sessions_between(from_ms, to_ms, 2_000)
    }

    pub fn record_recovery_event(&self, input: RecoveryEventInput) -> Result<RecoveryEvent> {
        let event_type = input.event_type.trim().to_ascii_lowercase();
        anyhow::ensure!(
            matches!(
                event_type.as_str(),
                "prompted" | "taken" | "skipped" | "snoozed" | "started"
            ) || event_type.ends_with("_prompted"),
            "unsupported recovery event_type: {event_type}"
        );
        if let Some(end) = input.ended_at {
            anyhow::ensure!(
                end >= input.started_at,
                "recovery event ended_at must be after started_at"
            );
        }
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("recovery-{}", Utc::now().timestamp_micros()));
        let duration_ms = input
            .ended_at
            .map(|end| end.saturating_sub(input.started_at))
            .unwrap_or(0);
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO recovery_events
                (id, event_type, started_at, ended_at, duration_ms, note, evidence_json,
                 created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
            ON CONFLICT(id) DO UPDATE SET
                event_type = excluded.event_type,
                started_at = excluded.started_at,
                ended_at = excluded.ended_at,
                duration_ms = excluded.duration_ms,
                note = excluded.note,
                evidence_json = excluded.evidence_json,
                updated_at = excluded.updated_at
            "#,
            params![
                &id,
                event_type,
                input.started_at,
                input.ended_at,
                duration_ms,
                &input.note,
                &input.evidence_json,
                now,
            ],
        )?;
        Self::recovery_event_by_id_locked(&conn, &id)
    }

    pub fn list_recovery_events_for_dates(
        &self,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<Vec<RecoveryEvent>> {
        let (from_ms, to_ms) = date_range_to_ms(from_date, to_date)?;
        self.list_recovery_events_between(from_ms, to_ms, 2_000)
    }

    pub fn upsert_field_visit(&self, input: FieldVisitInput) -> Result<FieldVisit> {
        let starts_at = input.starts_at.unwrap_or_else(now_ms);
        if let Some(end) = input.ends_at {
            anyhow::ensure!(
                end >= starts_at,
                "field visit ends_at must be after starts_at"
            );
        }
        let status = input.status.unwrap_or_else(|| "open".to_string());
        anyhow::ensure!(
            ["open", "completed", "cancelled"].contains(&status.as_str()),
            "unsupported field visit status: {status}"
        );
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("field-visit-{}", Utc::now().timestamp_micros()));
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO field_visits
                (id, client_label, starts_at, ends_at, location_label, debrief, status,
                 created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
            ON CONFLICT(id) DO UPDATE SET
                client_label = excluded.client_label,
                starts_at = excluded.starts_at,
                ends_at = excluded.ends_at,
                location_label = excluded.location_label,
                debrief = excluded.debrief,
                status = excluded.status,
                updated_at = excluded.updated_at
            "#,
            params![
                &id,
                &input.client_label,
                starts_at,
                input.ends_at,
                &input.location_label,
                &input.debrief,
                status,
                now,
            ],
        )?;
        Self::field_visit_by_id_locked(&conn, &id)
    }

    pub fn list_field_visits(&self, limit: usize) -> Result<Vec<FieldVisit>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, client_label, starts_at, ends_at, location_label, debrief, status,
                   created_at, updated_at
            FROM field_visits
            ORDER BY starts_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::field_visit_from_row)?;
        let mut visits = Vec::new();
        for visit in rows {
            visits.push(visit?);
        }
        Ok(visits)
    }

    pub fn upsert_idle_block(&self, input: IdleBlockInput) -> Result<IdleBlock> {
        anyhow::ensure!(
            input.ended_at >= input.started_at,
            "idle block ended_at must be after started_at"
        );
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("idle-{}", Utc::now().timestamp_micros()));
        let category = input
            .category
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let classified = input.classified.unwrap_or_else(|| category.is_some());
        let duration_ms = input.ended_at.saturating_sub(input.started_at);
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO idle_blocks
                (id, started_at, ended_at, duration_ms, category, classified,
                 evidence_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
            ON CONFLICT(id) DO UPDATE SET
                started_at = excluded.started_at,
                ended_at = excluded.ended_at,
                duration_ms = excluded.duration_ms,
                category = excluded.category,
                classified = excluded.classified,
                evidence_json = excluded.evidence_json,
                updated_at = excluded.updated_at
            "#,
            params![
                &id,
                input.started_at,
                input.ended_at,
                duration_ms,
                &category,
                if classified { 1 } else { 0 },
                &input.evidence_json,
                now,
            ],
        )?;
        Self::idle_block_by_id_locked(&conn, &id)
    }

    pub fn record_idle_gap_candidate(
        &self,
        kind: &str,
        started_at: i64,
        ended_at: i64,
    ) -> Result<Option<IdleBlock>> {
        anyhow::ensure!(
            ended_at >= started_at,
            "idle gap ended_at must be after started_at"
        );
        let duration_ms = ended_at.saturating_sub(started_at);
        let settings = self.get_settings()?;
        let min_gap_ms = settings.min_gap_minutes.max(1) * 60_000;
        if duration_ms < min_gap_ms {
            return Ok(None);
        }
        if self.pause_state()?.paused {
            return Ok(None);
        }

        // Auto-classify gaps that start entirely outside work hours so users
        // are never asked about activity at 1 am, 2 am, etc.
        let off_hours = settings.work_hours_enabled
            && is_outside_work_hours(started_at, settings.work_start_hour, settings.work_end_hour);

        let conn = self.lock()?;
        let covered_count: i64 = conn.query_row(
            r#"
            SELECT COUNT(*)
            FROM idle_blocks
            WHERE classified = 1
              AND started_at < ?2
              AND ended_at > ?1
            "#,
            params![started_at, ended_at],
            |row| row.get(0),
        )?;
        if covered_count > 0 {
            return Ok(None);
        }

        let kind_id = stable_id_part(kind);
        let id = format!(
            "idle-auto-{}-{}-{}",
            if kind_id.is_empty() {
                "return"
            } else {
                kind_id.as_str()
            },
            started_at.div_euclid(60_000),
            ended_at.div_euclid(60_000)
        );
        let existing_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM idle_blocks WHERE id = ?1",
            params![&id],
            |row| row.get(0),
        )?;
        if existing_count > 0 {
            return Ok(None);
        }
        drop(conn);

        self.upsert_idle_block(IdleBlockInput {
            id: Some(id),
            started_at,
            ended_at,
            category: if off_hours { Some("off_hours".to_string()) } else { None },
            classified: Some(off_hours),
            evidence_json: Some(
                serde_json::json!({
                    "source": "auto_idle_gap",
                    "kind": kind,
                    "durationMs": duration_ms,
                    "offHours": off_hours,
                })
                .to_string(),
            ),
        })
        .map(Some)
    }

    pub fn list_idle_blocks(&self, limit: usize) -> Result<Vec<IdleBlock>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, started_at, ended_at, duration_ms, category, classified, evidence_json,
                   created_at, updated_at
            FROM idle_blocks
            ORDER BY started_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::idle_block_from_row)?;
        let mut blocks = Vec::new();
        for block in rows {
            blocks.push(block?);
        }
        Ok(blocks)
    }

    pub fn delete_idle_block(&self, id: &str) -> Result<bool> {
        let id = id.trim();
        anyhow::ensure!(!id.is_empty(), "idle block id is required");
        let conn = self.lock()?;
        let changed = conn.execute("DELETE FROM idle_blocks WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    // ── Daily goals ───────────────────────────────────────────────────────────

    pub fn list_daily_goals(&self) -> Result<Vec<crate::models::DailyGoal>> {
        use crate::models::DailyGoal;
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, label, target_type, match_value, daily_target_ms, active, created_at
             FROM daily_goals ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DailyGoal {
                id: row.get(0)?,
                label: row.get(1)?,
                target_type: row.get(2)?,
                match_value: row.get(3)?,
                daily_target_ms: row.get(4)?,
                active: row.get::<_, i64>(5)? != 0,
                created_at: row.get(6)?,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn upsert_daily_goal(&self, input: crate::models::DailyGoalInput) -> Result<crate::models::DailyGoal> {
        use crate::models::DailyGoal;
        let id = format!("goal-{}", Utc::now().timestamp_micros());
        let now = now_ms();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO daily_goals (id, label, target_type, match_value, daily_target_ms, active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
            params![id, input.label, input.target_type, input.match_value, input.daily_target_ms, now],
        )?;
        Ok(DailyGoal {
            id,
            label: input.label,
            target_type: input.target_type,
            match_value: input.match_value,
            daily_target_ms: input.daily_target_ms,
            active: true,
            created_at: now,
        })
    }

    pub fn delete_daily_goal(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM daily_goals WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn build_goal_progress(
        &self,
        goals: &[crate::models::DailyGoal],
        source_events: &[SourceEvent],
    ) -> Vec<crate::models::GoalProgress> {
        use crate::models::GoalProgress;
        goals
            .iter()
            .filter(|g| g.active)
            .map(|goal| {
                let matching: Vec<&SourceEvent> = source_events
                    .iter()
                    .filter(|e| goal_event_matches(goal, e))
                    .collect();
                let achieved_ms = merge_event_intervals(&matching);
                let progress_ratio = if goal.daily_target_ms > 0 {
                    achieved_ms as f64 / goal.daily_target_ms as f64
                } else {
                    0.0
                };
                GoalProgress {
                    goal_id: goal.id.clone(),
                    label: goal.label.clone(),
                    target_type: goal.target_type.clone(),
                    match_value: goal.match_value.clone(),
                    daily_target_ms: goal.daily_target_ms,
                    achieved_ms,
                    progress_ratio,
                    met: achieved_ms >= goal.daily_target_ms,
                }
            })
            .collect()
    }

    pub fn build_streak_summary(
        &self,
        from_ms: i64,
        to_ms: i64,
        threshold_ms: i64,
    ) -> Result<crate::models::StreakSummary> {
        use crate::models::StreakSummary;
        // One query: sum duration_ms grouped by calendar day (UTC seconds)
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT date(started_at / 1000, 'unixepoch', 'localtime') AS day,
                   SUM(duration_ms) AS total_ms
            FROM source_events
            WHERE started_at >= ?1 AND started_at < ?2
              AND event_type != 'git_commit'
            GROUP BY day
            ORDER BY day ASC
            "#,
        )?;
        let rows = stmt.query_map(params![from_ms, to_ms], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut day_totals: Vec<(String, i64)> = Vec::new();
        for row in rows {
            day_totals.push(row?);
        }

        let today_str = {
            let now = chrono::Local::now();
            now.format("%Y-%m-%d").to_string()
        };

        let active_days_30 = day_totals.iter().filter(|(_, ms)| *ms >= threshold_ms).count() as i64;
        let avg_daily_ms = if active_days_30 > 0 {
            day_totals
                .iter()
                .filter(|(_, ms)| *ms >= threshold_ms)
                .map(|(_, ms)| ms)
                .sum::<i64>()
                / active_days_30
        } else {
            0
        };

        // Current streak: walk backwards from today
        let mut current_streak_days = 0_i64;
        let day_set: std::collections::HashMap<&str, i64> =
            day_totals.iter().map(|(d, ms)| (d.as_str(), *ms)).collect();
        let mut check_date = chrono::Local::now().date_naive();
        loop {
            let ds = check_date.format("%Y-%m-%d").to_string();
            if let Some(&ms) = day_set.get(ds.as_str()) {
                if ms >= threshold_ms {
                    current_streak_days += 1;
                    check_date = check_date
                        .checked_sub_days(chrono::Days::new(1))
                        .unwrap_or(check_date);
                    continue;
                }
            }
            break;
        }

        // Longest ever streak in the window
        let mut longest_streak_days = 0_i64;
        let mut run = 0_i64;
        let mut prev_date: Option<chrono::NaiveDate> = None;
        for (day_str, ms) in &day_totals {
            if *ms < threshold_ms {
                if run > longest_streak_days {
                    longest_streak_days = run;
                }
                run = 0;
                prev_date = None;
                continue;
            }
            if let Ok(d) = chrono::NaiveDate::parse_from_str(day_str, "%Y-%m-%d") {
                let consecutive = prev_date
                    .map(|p| p.checked_add_days(chrono::Days::new(1)) == Some(d))
                    .unwrap_or(true);
                if consecutive {
                    run += 1;
                } else {
                    if run > longest_streak_days {
                        longest_streak_days = run;
                    }
                    run = 1;
                }
                prev_date = Some(d);
            }
        }
        if run > longest_streak_days {
            longest_streak_days = run;
        }
        let _ = today_str;
        Ok(StreakSummary {
            current_streak_days,
            longest_streak_days,
            avg_daily_ms,
            active_days_30,
            threshold_ms,
        })
    }

    // ── Active work context ───────────────────────────────────────────────────

    pub fn get_active_work_context(&self) -> Result<Option<ActiveWorkContext>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT client, project, task, ticket_id, billable, updated_at
             FROM active_work_context WHERE id = 1",
        )?;
        let mut rows = stmt.query_map([], |row| {
            Ok(ActiveWorkContext {
                client: row.get(0)?,
                project: row.get(1)?,
                task: row.get(2)?,
                ticket_id: row.get(3)?,
                billable: row.get::<_, i64>(4).map(|v| v != 0).unwrap_or(true),
                updated_at: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn set_active_work_context(
        &self,
        input: ActiveWorkContextInput,
    ) -> Result<ActiveWorkContext> {
        let now = now_ms();
        let billable = input.billable.unwrap_or(true) as i64;
        let conn = self.lock()?;
        conn.execute(
            r#"INSERT INTO active_work_context (id, client, project, task, ticket_id, billable, updated_at)
               VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
               ON CONFLICT(id) DO UPDATE SET
                 client = excluded.client,
                 project = excluded.project,
                 task = excluded.task,
                 ticket_id = excluded.ticket_id,
                 billable = excluded.billable,
                 updated_at = excluded.updated_at"#,
            params![input.client, input.project, input.task, input.ticket_id, billable, now],
        )?;
        Ok(ActiveWorkContext {
            client: input.client,
            project: input.project,
            task: input.task,
            ticket_id: input.ticket_id,
            billable: input.billable.unwrap_or(true),
            updated_at: now,
        })
    }

    pub fn clear_active_work_context(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM active_work_context WHERE id = 1", [])?;
        Ok(())
    }

    pub fn record_loop_action(&self, input: LoopActionInput) -> Result<LoopAction> {
        let id = input.id.trim();
        anyhow::ensure!(!id.is_empty(), "loop item id is required");
        let action = input.action.trim().to_ascii_lowercase();
        anyhow::ensure!(
            matches!(action.as_str(), "closed" | "snoozed" | "ignored"),
            "loop action must be closed, snoozed, or ignored"
        );
        let now = now_ms();
        let snoozed_until = if action == "snoozed" {
            Some(input.snoozed_until.unwrap_or(now + 24 * 60 * 60 * 1000))
        } else {
            None
        };
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO loop_item_actions
                (id, action, snoozed_until, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?4)
            ON CONFLICT(id) DO UPDATE SET
                action = excluded.action,
                snoozed_until = excluded.snoozed_until,
                updated_at = excluded.updated_at
            "#,
            params![id, action, snoozed_until, now],
        )?;
        Self::loop_action_by_id_locked(&conn, id)
    }

    fn hidden_loop_ids(&self) -> Result<HashSet<String>> {
        let conn = self.lock()?;
        Self::hidden_loop_ids_locked(&conn)
    }

    pub fn clear_clipboard_history(&self) -> Result<PrivacyDeleteSummary> {
        let conn = self.lock()?;
        let deleted_rows = conn.execute("DELETE FROM clipboard_events", [])?;
        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    pub fn delete_context_data(&self, context_id: &str) -> Result<PrivacyDeleteSummary> {
        let context_id = context_id.trim();
        anyhow::ensure!(!context_id.is_empty(), "context_id is required");
        let conn = self.lock()?;
        let context_key: Option<String> = conn
            .query_row(
                "SELECT context_key FROM workspace_contexts WHERE id = ?1",
                params![context_id],
                |row| row.get(0),
            )
            .optional()?;
        let mut deleted_rows = 0;

        deleted_rows += conn.execute(
            "DELETE FROM stream_events WHERE stream_id IN (SELECT id FROM parallel_streams WHERE context_id = ?1)",
            params![context_id],
        )?;
        deleted_rows += conn.execute(
            "DELETE FROM parallel_streams WHERE context_id = ?1",
            params![context_id],
        )?;
        deleted_rows += conn.execute(
            "DELETE FROM work_sessions WHERE context_id = ?1",
            params![context_id],
        )?;
        deleted_rows += conn.execute(
            "DELETE FROM scratchpad_notes WHERE context_id = ?1",
            params![context_id],
        )?;
        deleted_rows += conn.execute(
            "DELETE FROM state_snapshots WHERE context_id = ?1",
            params![context_id],
        )?;
        deleted_rows += conn.execute(
            "DELETE FROM ai_usage WHERE context_id = ?1",
            params![context_id],
        )?;
        deleted_rows += conn.execute(
            "DELETE FROM work_graph_edges WHERE from_id = ?1 OR to_id = ?1",
            params![context_id],
        )?;
        if let Some(key) = context_key {
            deleted_rows += conn.execute(
                "DELETE FROM source_events WHERE workspace_key = ?1 OR domain = ?1 OR app = ?1",
                params![key],
            )?;
        }
        deleted_rows += conn.execute(
            "DELETE FROM workspace_contexts WHERE id = ?1",
            params![context_id],
        )?;

        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    pub fn purge_captured_data(&self) -> Result<PrivacyDeleteSummary> {
        let conn = self.lock()?;
        let mut deleted_rows = 0;
        for table in [
            "stream_events",
            "work_graph_edges",
            "source_events",
            "workspace_contexts",
            "work_sessions",
            "parallel_streams",
            "scratchpad_notes",
            "state_snapshots",
            "clipboard_events",
            "agent_runs",
            "activity_events",
            "activity_task_links",
            "task_match_rules",
            "tasks",
            "quick_notes",
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
            "projects",
            "people",
        ] {
            deleted_rows += conn.execute(&format!("DELETE FROM {table}"), [])?;
        }
        conn.execute("DELETE FROM materialization_state", [])?;
        Self::rebuild_work_memory_index_locked(&conn)?;
        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    pub fn prune_captured_data_older_than_days(&self, days: i64) -> Result<PrivacyDeleteSummary> {
        anyhow::ensure!(days > 0, "retention days must be greater than zero");
        anyhow::ensure!(days <= 3650, "retention days cannot exceed 3650");
        let cutoff_ms = now_ms().saturating_sub(days.saturating_mul(24 * 60 * 60 * 1000));
        self.prune_captured_data_before(cutoff_ms)
    }

    pub fn prune_completed_tasks_older_than_days(&self, days: i64) -> Result<PrivacyDeleteSummary> {
        anyhow::ensure!(days > 0, "task retention days must be greater than zero");
        anyhow::ensure!(days <= 3650, "task retention days cannot exceed 3650");
        let cutoff_ms = now_ms().saturating_sub(days.saturating_mul(24 * 60 * 60 * 1000));
        let cutoff_iso = Utc
            .timestamp_millis_opt(cutoff_ms)
            .single()
            .map(|date| date.to_rfc3339_opts(SecondsFormat::Secs, true))
            .unwrap_or_else(now_utc);

        let conn = self.lock()?;
        // completed_at is stored as UTC RFC3339 text (for example, 2026-06-02T14:00:00Z).
        // Lexicographic comparison is safe only while that normalized format is preserved.
        let deleted_rows = conn.execute(
            r#"
            DELETE FROM tasks
            WHERE status = 'done'
              AND COALESCE(completed_at, updated_at, created_at) < ?1
            "#,
            params![cutoff_iso],
        )?;
        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    pub fn apply_retention_policy(&self) -> Result<PrivacyDeleteSummary> {
        let settings = self.get_settings()?;
        let mut deleted_rows = 0;

        if settings.data_retention_days > 0 {
            deleted_rows += self
                .prune_captured_data_older_than_days(settings.data_retention_days)?
                .deleted_rows;
        }

        if settings.task_retention_days > 0 {
            deleted_rows += self
                .prune_completed_tasks_older_than_days(settings.task_retention_days)?
                .deleted_rows;
        }

        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    fn prune_captured_data_before(&self, cutoff_ms: i64) -> Result<PrivacyDeleteSummary> {
        let conn = self.lock()?;
        let mut deleted_rows = 0;
        let cutoff_iso = Utc
            .timestamp_millis_opt(cutoff_ms)
            .single()
            .map(|date| date.to_rfc3339_opts(SecondsFormat::Secs, true))
            .unwrap_or_else(now_utc);

        deleted_rows += conn.execute(
            "DELETE FROM stream_events WHERE event_id IN (SELECT id FROM source_events WHERE ended_at < ?1)",
            params![cutoff_ms],
        )?;
        for (table, column) in [
            ("activity_events", "created_at"),
            ("tasks", "updated_at"),
            ("quick_notes", "created_at"),
        ] {
            deleted_rows +=
                Self::delete_where_text_before_locked(&conn, table, column, &cutoff_iso)?;
        }
        for (table, column_expr) in [
            ("source_events", "ended_at"),
            ("workspace_contexts", "updated_at"),
            ("work_sessions", "ended_at"),
            ("parallel_streams", "COALESCE(ended_at, started_at)"),
            ("scratchpad_notes", "created_at"),
            ("state_snapshots", "created_at"),
            ("clipboard_events", "created_at"),
            ("agent_runs", "COALESCE(ended_at, started_at)"),
            ("commitments", "updated_at"),
            ("email_threads", "updated_at"),
            ("meetings", "COALESCE(ends_at, starts_at, created_at)"),
            ("field_visits", "COALESCE(ends_at, starts_at, created_at)"),
            ("idle_blocks", "ended_at"),
            ("recovery_events", "COALESCE(ended_at, started_at)"),
            ("loop_item_actions", "updated_at"),
            ("ai_usage", "COALESCE(ended_at, started_at, created_at)"),
            ("outputs", "updated_at"),
            ("decisions", "decided_at"),
            ("reports", "generated_at"),
            ("plans", "generated_at"),
            ("weekly_reviews", "generated_at"),
            ("projects", "updated_at"),
            ("people", "updated_at"),
            ("work_graph_edges", "created_at"),
        ] {
            deleted_rows += Self::delete_where_before_locked(&conn, table, column_expr, cutoff_ms)?;
        }
        deleted_rows += conn.execute(
            "DELETE FROM stream_events WHERE stream_id NOT IN (SELECT id FROM parallel_streams) OR event_id NOT IN (SELECT id FROM source_events)",
            [],
        )?;
        if deleted_rows > 0 {
            conn.execute("DELETE FROM materialization_state", [])?;
            Self::rebuild_work_memory_index_locked(&conn)?;
        }
        Ok(PrivacyDeleteSummary { deleted_rows })
    }

    fn delete_where_before_locked(
        conn: &Connection,
        table: &str,
        column_expr: &str,
        cutoff_ms: i64,
    ) -> Result<usize> {
        conn.execute(
            &format!("DELETE FROM {table} WHERE {column_expr} < ?1"),
            params![cutoff_ms],
        )
        .map_err(Into::into)
    }

    fn delete_where_text_before_locked(
        conn: &Connection,
        table: &str,
        column: &str,
        cutoff_iso: &str,
    ) -> Result<usize> {
        conn.execute(
            &format!("DELETE FROM {table} WHERE {column} < ?1"),
            params![cutoff_iso],
        )
        .map_err(Into::into)
    }

    pub fn list_work_sessions(&self, limit: usize) -> Result<Vec<WorkSessionSummary>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, status, started_at, ended_at, duration_ms, ai_used,
                   confidence, summary, evidence_json,
                   billing_status, billable, client_label, project_label, ticket_id, review_notes
            FROM work_sessions
            ORDER BY COALESCE(ended_at, started_at) DESC, started_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::work_session_from_row)?;
        let mut sessions = Vec::new();
        for session in rows {
            sessions.push(session?);
        }
        Ok(sessions)
    }

    pub fn review_session(&self, input: ReviewSessionInput) -> Result<WorkSessionSummary> {
        let conn = self.lock()?;
        conn.execute(
            r#"UPDATE work_sessions
               SET billing_status = COALESCE(?2, billing_status),
                   billable       = COALESCE(?3, billable),
                   client_label   = COALESCE(?4, client_label),
                   project_label  = COALESCE(?5, project_label),
                   ticket_id      = COALESCE(?6, ticket_id),
                   review_notes   = COALESCE(?7, review_notes),
                   updated_at     = ?8
               WHERE id = ?1"#,
            params![
                input.session_id,
                input.billing_status,
                input.billable.map(|b| if b { 1_i64 } else { 0_i64 }),
                input.client_label,
                input.project_label,
                input.ticket_id,
                input.review_notes,
                now_ms(),
            ],
        )?;
        let session = conn.query_row(
            r#"SELECT id, title, status, started_at, ended_at, duration_ms, ai_used,
                      confidence, summary, evidence_json,
                      billing_status, billable, client_label, project_label, ticket_id, review_notes
               FROM work_sessions WHERE id = ?1"#,
            params![input.session_id],
            Self::work_session_from_row,
        )?;
        Ok(session)
    }

    pub fn list_sessions_for_review(
        &self,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<Vec<WorkSessionSummary>> {
        let (from_ms, to_ms) = date_range_to_ms(from_date, to_date)?;
        self.list_work_sessions_between(from_ms, to_ms, 500)
    }

    pub fn export_timesheet_markdown(
        &self,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<String> {
        let (from_ms, to_ms) = date_range_to_ms(from_date, to_date)?;
        let sessions = self.list_work_sessions_between(from_ms, to_ms, 500)?;
        let events = self.list_source_events_between(from_ms, to_ms, 10_000)?;
        let rows = build_timesheet_rows(&sessions, &events, from_ms, to_ms);

        let mut md = String::new();
        md.push_str("# Timesheet\n\n");

        // group by date
        let mut current_date = String::new();
        let mut date_total_ms: i64 = 0;
        let mut grand_total_ms: i64 = 0;

        for row in &rows {
            if row.billing_status == "excluded" {
                continue;
            }
            if row.local_date != current_date {
                if !current_date.is_empty() {
                    md.push_str(&format!(
                        "\n**Day total: {}**\n\n",
                        format_duration_md(date_total_ms)
                    ));
                }
                current_date = row.local_date.clone();
                date_total_ms = 0;
                md.push_str(&format!("## {}\n\n", current_date));
                md.push_str(
                    "| Time | Duration | Title | Client / Project | Ticket | Billable | Apps |\n",
                );
                md.push_str(
                    "|------|----------|-------|-----------------|--------|----------|------|\n",
                );
            }
            date_total_ms += row.duration_ms;
            grand_total_ms += row.duration_ms;
            let time_label = format_time_label(row.started_at);
            let duration = format_duration_md(row.duration_ms);
            let client_project = row
                .client_label
                .clone()
                .or_else(|| row.project_label.clone())
                .unwrap_or_else(|| row.project_or_client.clone());
            let ticket = row.ticket_id.clone().unwrap_or_default();
            let billable = if row.billable { "✓" } else { "–" };
            let title = row.title.replace('|', "\\|");
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                time_label, duration, title, client_project, ticket, billable, row.app
            ));
        }
        if !current_date.is_empty() {
            md.push_str(&format!(
                "\n**Day total: {}**\n\n",
                format_duration_md(date_total_ms)
            ));
        }
        md.push_str(&format!(
            "\n---\n**Grand total: {}**\n",
            format_duration_md(grand_total_ms)
        ));
        Ok(md)
    }

    pub fn list_parallel_streams(&self, limit: usize) -> Result<Vec<ParallelStreamSummary>> {
        self.list_parallel_streams_between(None, None, limit)
    }

    pub fn list_parallel_streams_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<ParallelStreamSummary>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, started_at, ended_at, summary
            FROM parallel_streams
            WHERE (?1 IS NULL OR COALESCE(ended_at, started_at) >= ?1)
              AND (?2 IS NULL OR started_at < ?2)
            ORDER BY started_at DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(params![from_ms, to_ms, limit as i64], |row| {
            let id: String = row.get(0)?;
            let event_ids = Self::stream_event_ids_locked(&conn, &id).unwrap_or_default();
            Ok(ParallelStreamSummary {
                id,
                title: row.get(1)?,
                status: "active".to_string(),
                started_at: row.get(2)?,
                ended_at: row.get(3)?,
                summary: row.get(4)?,
                event_ids,
                next_action: Some("Review latest unresolved event and close the loop.".to_string()),
            })
        })?;

        let mut streams = Vec::new();
        for stream in rows {
            streams.push(stream?);
        }
        Ok(streams)
    }

    fn source_event_fallback_between(
        &self,
        limit: usize,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<(Vec<WorkSessionSummary>, Vec<ParallelStreamSummary>)> {
        let settings = self.get_settings()?;
        let idle_gap_ms = settings.idle_timeout_minutes.max(1) * 60_000;
        let events = self.list_source_events_for_materialization_between(from_ms, to_ms)?;
        let mut sessions: Vec<WorkSessionSummary> =
            build_sessions_from_source_events(&events, idle_gap_ms)
                .into_iter()
                .map(work_session_summary_from_materialized)
                .collect();
        let mut streams: Vec<ParallelStreamSummary> = build_streams_from_source_events(&events)
            .into_iter()
            .map(parallel_stream_summary_from_materialized)
            .collect();

        sessions.sort_by_key(|session| std::cmp::Reverse(session.started_at));
        streams.sort_by_key(|stream| std::cmp::Reverse(stream.started_at));
        sessions.truncate(limit);
        streams.truncate(limit);
        Ok((sessions, streams))
    }

    fn list_today_source_events(&self, limit: usize) -> Result<Vec<SourceEvent>> {
        let today = Local::now().date_naive();
        let (day_start, day_end) = local_day_bounds_ms(today);
        self.list_source_events_between(Some(day_start), Some(day_end), limit)
    }

    pub fn list_source_events_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<SourceEvent>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, source, event_type, app, title, domain, url_redacted, workspace_key,
                   started_at, ended_at, duration_ms, sensitivity, metadata_json, created_at
            FROM source_events
            WHERE (?1 IS NULL OR ended_at >= ?1)
              AND (?2 IS NULL OR started_at < ?2)
            ORDER BY started_at DESC, ended_at DESC, id DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![from_ms, to_ms, limit as i64],
            Self::source_event_from_row,
        )?;
        let mut events = Vec::new();
        for event in rows {
            events.push(event?);
        }
        Ok(events)
    }

    pub fn list_calendar_events_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<CalendarEvent>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, source, external_id, calendar_name, title, starts_at, ends_at,
                   location, status, planned_work_type, created_at, updated_at
            FROM calendar_events
            WHERE (?1 IS NULL OR ends_at >= ?1)
              AND (?2 IS NULL OR starts_at < ?2)
            ORDER BY starts_at DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![from_ms, to_ms, limit as i64],
            Self::calendar_event_from_row,
        )?;
        let mut events = Vec::new();
        for event in rows {
            events.push(event?);
        }
        Ok(events)
    }

    pub fn list_focus_sessions_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<FocusSessionSummary>> {
        let bases = self.list_focus_session_bases_between(from_ms, to_ms, limit)?;
        let event_from_ms = bases.iter().map(|session| session.started_at).min();
        let event_to_ms = bases
            .iter()
            .map(|session| session.ended_at.unwrap_or_else(now_ms))
            .max();
        let events = self.list_source_events_between(event_from_ms, event_to_ms, 10_000)?;
        let now = now_ms();
        Ok(bases
            .iter()
            .map(|focus| summarize_focus_session(focus, &events, now))
            .collect())
    }

    pub fn list_recovery_events_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<RecoveryEvent>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, event_type, started_at, ended_at, duration_ms, note,
                   evidence_json, created_at, updated_at
            FROM recovery_events
            WHERE (?1 IS NULL OR COALESCE(ended_at, started_at) >= ?1)
              AND (?2 IS NULL OR started_at < ?2)
            ORDER BY started_at DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![from_ms, to_ms, limit as i64],
            Self::recovery_event_from_row,
        )?;
        let mut events = Vec::new();
        for event in rows {
            events.push(event?);
        }
        Ok(events)
    }

    fn list_focus_session_bases_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<FocusSessionRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, goal, client, project, task, ticket_id, target_ms, started_at,
                   ended_at, status, created_at, updated_at
            FROM focus_sessions
            WHERE (?1 IS NULL OR COALESCE(ended_at, started_at) >= ?1)
              AND (?2 IS NULL OR started_at < ?2)
            ORDER BY started_at DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![from_ms, to_ms, limit as i64],
            Self::focus_session_base_from_row,
        )?;
        let mut sessions = Vec::new();
        for session in rows {
            sessions.push(session?);
        }
        Ok(sessions)
    }

    pub fn list_work_sessions_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<WorkSessionSummary>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, status, started_at, ended_at, duration_ms, ai_used,
                   confidence, summary, evidence_json,
                   billing_status, billable, client_label, project_label, ticket_id, review_notes
            FROM work_sessions
            WHERE (?1 IS NULL OR ended_at >= ?1)
              AND (?2 IS NULL OR started_at < ?2)
            ORDER BY COALESCE(ended_at, started_at) DESC, started_at DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![from_ms, to_ms, limit as i64],
            Self::work_session_from_row,
        )?;
        let mut sessions = Vec::new();
        for session in rows {
            sessions.push(session?);
        }
        Ok(sessions)
    }

    pub fn list_idle_blocks_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<IdleBlock>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, started_at, ended_at, duration_ms, category, classified, evidence_json,
                   created_at, updated_at
            FROM idle_blocks
            WHERE (?1 IS NULL OR ended_at >= ?1)
              AND (?2 IS NULL OR started_at < ?2)
            ORDER BY started_at DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![from_ms, to_ms, limit as i64],
            Self::idle_block_from_row,
        )?;
        let mut blocks = Vec::new();
        for block in rows {
            blocks.push(block?);
        }
        Ok(blocks)
    }

    pub fn list_ai_usage_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<AiUsage>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, provider, tool_name, thread_title, context_id, prompt_summary,
                   output_summary, started_at, ended_at, duration_ms, metadata_json, created_at
            FROM ai_usage
            WHERE (?1 IS NULL OR COALESCE(ended_at, created_at) >= ?1)
              AND (?2 IS NULL OR COALESCE(started_at, created_at) < ?2)
            ORDER BY created_at DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![from_ms, to_ms, limit as i64],
            Self::ai_usage_from_row,
        )?;
        let mut usage = Vec::new();
        for item in rows {
            usage.push(item?);
        }
        Ok(usage)
    }

    pub fn record_agent_run(&self, input: AgentRunInput) -> Result<AgentRun> {
        let now = now_ms();
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("agent-run-{}", Utc::now().timestamp_micros()));
        let started_at = input.started_at.unwrap_or(now);
        if let Some(ended_at) = input.ended_at {
            anyhow::ensure!(
                ended_at >= started_at,
                "agent run ended_at must be after started_at"
            );
        }
        let status = input.status.filter(|value| !value.trim().is_empty());
        anyhow::ensure!(
            input
                .tool_name
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
                || input
                    .command_label
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()),
            "agent run requires tool_name or command_label"
        );

        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO agent_runs
                (id, context_id, tool_name, command_label, started_at, ended_at, status,
                 exit_code, summary, error_tail, notified, metadata_json)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(id) DO UPDATE SET
                context_id = excluded.context_id,
                tool_name = excluded.tool_name,
                command_label = excluded.command_label,
                started_at = excluded.started_at,
                ended_at = excluded.ended_at,
                status = excluded.status,
                exit_code = excluded.exit_code,
                summary = excluded.summary,
                error_tail = excluded.error_tail,
                notified = excluded.notified,
                metadata_json = excluded.metadata_json
            "#,
            params![
                &id,
                &input.context_id,
                &input.tool_name,
                &input.command_label,
                started_at,
                input.ended_at,
                &status,
                input.exit_code,
                &input.summary,
                &input.error_tail,
                if input.notified.unwrap_or(false) {
                    1
                } else {
                    0
                },
                &input.metadata_json,
            ],
        )?;
        Self::agent_run_by_id_locked(&conn, &id)
    }

    pub fn detect_loop_risks(&self) -> Result<Vec<LoopRisk>> {
        let conn = self.lock()?;
        Self::detect_loop_risks_locked(&conn)
    }

    pub fn next_best_action(&self) -> Result<Option<NextBestAction>> {
        let conn = self.lock()?;
        let now = now_ms();
        if let Some((id, subject)) = conn
            .query_row(
                r#"
                SELECT id, subject
                FROM email_threads
                WHERE pending_reply = 1
                  AND NOT EXISTS (
                      SELECT 1
                      FROM loop_item_actions
                      WHERE loop_item_actions.id = 'reply-' || email_threads.id
                        AND (
                            action IN ('closed', 'ignored')
                            OR (action = 'snoozed' AND snoozed_until IS NOT NULL AND snoozed_until > ?1)
                        )
                  )
                ORDER BY latest_at, updated_at DESC
                LIMIT 1
                "#,
                params![now],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
        {
            return Ok(Some(NextBestAction {
                title: format!("Reply to {subject}"),
                reason: "A source record marked this thread as needing a reply".to_string(),
                source_type: "email_thread".to_string(),
                source_id: id,
                priority: 100,
            }));
        }

        if let Some((id, title, due_at)) = conn
            .query_row(
                r#"
                SELECT id, title, due_at
                FROM commitments
                WHERE status = 'open'
                  AND NOT EXISTS (
                      SELECT 1
                      FROM loop_item_actions
                      WHERE loop_item_actions.id = 'promise-' || commitments.id
                        AND (
                            action IN ('closed', 'ignored')
                            OR (action = 'snoozed' AND snoozed_until IS NOT NULL AND snoozed_until > ?1)
                        )
                  )
                ORDER BY
                    CASE WHEN due_at IS NULL THEN 1 ELSE 0 END,
                    due_at,
                    created_at DESC
                LIMIT 1
                "#,
                params![now],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()?
        {
            return Ok(Some(NextBestAction {
                title,
                reason: due_at
                    .map(|value| format!("Open commitment is due at {value}"))
                    .unwrap_or_else(|| "Open commitment is waiting for closure".to_string()),
                source_type: "commitment".to_string(),
                source_id: id,
                priority: 95,
            }));
        }

        if let Some((id, title, output_type)) = conn
            .query_row(
                r#"
                SELECT id, title, output_type
                FROM outputs
                WHERE (status IN ('drafted', 'needs_review') OR status IS NULL)
                  AND NOT EXISTS (
                      SELECT 1
                      FROM loop_item_actions
                      WHERE loop_item_actions.id = 'ai-output-' || outputs.id
                        AND (
                            action IN ('closed', 'ignored')
                            OR (action = 'snoozed' AND snoozed_until IS NOT NULL AND snoozed_until > ?1)
                        )
                  )
                ORDER BY updated_at DESC
                LIMIT 1
                "#,
                params![now],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
        {
            return Ok(Some(NextBestAction {
                title,
                reason: format!("Drafted {output_type} output still needs closure"),
                source_type: "output".to_string(),
                source_id: id,
                priority: 90,
            }));
        }

        let hidden_loop_ids = Self::hidden_loop_ids_locked(&conn)?;
        if let Some(risk) = Self::detect_loop_risks_locked(&conn)?
            .into_iter()
            .filter(|risk| {
                !hidden_loop_ids.contains(&format!("risk-{}-{}", risk.risk_type, risk.id))
            })
            .find(|risk| risk.priority >= 90)
        {
            return Ok(Some(NextBestAction {
                title: risk.title,
                reason: risk.reason,
                source_type: format!("loop_risk:{}", risk.risk_type),
                source_id: risk.id,
                priority: risk.priority,
            }));
        }

        let today = Local::now().date_naive();
        let (day_start, day_end) = local_day_bounds_ms(today);
        if let Some((id, duration_ms)) = conn
            .query_row(
                r#"
                SELECT id, duration_ms
                FROM idle_blocks
                WHERE classified = 0
                  AND ended_at >= ?1
                  AND started_at < ?2
                  AND COALESCE(evidence_json, '') NOT LIKE '%source_event_gap%'
                  AND NOT EXISTS (
                      SELECT 1
                      FROM loop_item_actions
                      WHERE loop_item_actions.id = 'idle-' || idle_blocks.id
                        AND (
                            action IN ('closed', 'ignored')
                            OR (action = 'snoozed' AND snoozed_until IS NOT NULL AND snoozed_until > ?3)
                        )
                  )
                ORDER BY started_at DESC
                LIMIT 1
                "#,
                params![day_start, day_end, now],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
        {
            return Ok(Some(NextBestAction {
                title: "Classify idle or away block".to_string(),
                reason: format!("{} minutes are not yet explained", duration_ms / 60_000),
                source_type: "idle_block".to_string(),
                source_id: id,
                priority: 85,
            }));
        }

        if let Some((id, title, due_date, due_at, priority)) = conn
            .query_row(
                r#"
                SELECT id, title, due_date, due_at, priority
                FROM tasks
                WHERE status = 'open'
                ORDER BY
                    CASE
                        WHEN due_at IS NOT NULL AND due_at <= ?1 THEN 0
                        WHEN due_at IS NOT NULL THEN 1
                        ELSE 2
                    END,
                    CASE priority
                        WHEN 'high' THEN 0
                        WHEN 'medium' THEN 1
                        WHEN 'low' THEN 2
                        ELSE 3
                    END,
                    due_at,
                    due_date,
                    created_at DESC
                LIMIT 1
                "#,
                params![now],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()?
        {
            return Ok(Some(NextBestAction {
                title,
                reason: due_at
                    .map(|value| {
                        format!("Task reminder is due {}", format_task_due_at_label(value))
                    })
                    .or_else(|| due_date.map(|date| format!("Open task is due on {date}")))
                    .map(|base| {
                        priority
                            .as_deref()
                            .map(|priority| format!("{base} · {priority} priority"))
                            .unwrap_or(base)
                    })
                    .unwrap_or_else(|| "Open task is waiting for closure".to_string()),
                source_type: "task".to_string(),
                source_id: id.to_string(),
                priority: 80,
            }));
        }

        Ok(None)
    }

    pub fn record_activity(
        &self,
        event_type: &str,
        source: Option<&str>,
        title: Option<&str>,
        url: Option<&str>,
        project_path: Option<&str>,
        metadata_json: Option<&str>,
    ) -> Result<()> {
        if self.pause_state()?.paused {
            return Ok(());
        }
        let settings = self.get_settings()?;
        if source.is_some_and(|value| is_excluded(value, &settings.excluded_apps)) {
            return Ok(());
        }
        if url
            .and_then(|value| redact_url(value).0)
            .is_some_and(|domain| is_excluded(&domain, &settings.excluded_domains))
        {
            return Ok(());
        }
        if project_path
            .filter(|value| !value.trim().is_empty())
            .is_some_and(|value| is_project_excluded(value, &settings.excluded_projects))
        {
            return Ok(());
        }

        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO activity_events
                (event_type, source, title, url, project_path, metadata_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                event_type,
                source,
                title,
                url,
                project_path,
                metadata_json,
                now_utc()
            ],
        )?;
        Ok(())
    }

    pub fn record_active_window(
        &self,
        app_name: &str,
        window_title: Option<&str>,
        metadata_json: Option<&str>,
        observed_duration: Option<Duration>,
    ) -> Result<()> {
        self.record_active_window_context(
            app_name,
            window_title,
            None,
            None,
            metadata_json,
            observed_duration,
        )
    }

    pub fn record_active_window_context(
        &self,
        app_name: &str,
        window_title: Option<&str>,
        url: Option<&str>,
        workspace_key: Option<&str>,
        metadata_json: Option<&str>,
        observed_duration: Option<Duration>,
    ) -> Result<()> {
        if self.pause_state()?.paused {
            return Ok(());
        }
        // Skip system idle/lock-screen apps — they are not work.
        if is_idle_system_app(app_name) {
            return Ok(());
        }
        let settings = self.get_settings()?;
        if active_window_context_is_excluded(app_name, url, workspace_key, &settings) {
            return Ok(());
        }

        let now = now_ms();
        let observed_ms = observed_duration
            .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or_default();
        let started_at = now.saturating_sub(observed_ms);
        let title = window_title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(app_name);
        let source_event = self.record_source_event(SourceEventInput {
            id: None,
            source: "active-window".to_string(),
            event_type: "active_window".to_string(),
            app: Some(app_name.to_string()),
            title: Some(title.to_string()),
            url: url.map(ToOwned::to_owned),
            workspace_key: workspace_key.map(ToOwned::to_owned),
            started_at: Some(started_at),
            ended_at: Some(now),
            sensitivity: Some("normal".to_string()),
            metadata_json: metadata_json.map(ToOwned::to_owned),
        })?;

        for tool_name in detect_ai_tools(
            source_event.domain.as_deref(),
            source_event.title.as_deref(),
            source_event.url_redacted.as_deref(),
            source_event.metadata_json.as_deref(),
        ) {
            self.record_ai_usage(AiUsageInput {
                id: Some(format!(
                    "ai-{}-{}",
                    stable_id_part(tool_name),
                    source_event.id
                )),
                provider: Some(tool_name.to_string()),
                tool_name: Some(tool_name.to_string()),
                thread_title: source_event.title.clone(),
                context_id: source_event
                    .workspace_key
                    .as_deref()
                    .or(source_event.domain.as_deref())
                    .map(context_id_from_key),
                prompt_summary: None,
                output_summary: None,
                started_at: Some(source_event.started_at),
                ended_at: Some(source_event.ended_at),
                duration_ms: Some(source_event.duration_ms),
                metadata_json: source_event.metadata_json.clone(),
            })?;
        }

        self.record_activity(
            "active_window",
            Some(app_name),
            Some(title),
            source_event.url_redacted.as_deref(),
            source_event.workspace_key.as_deref(),
            metadata_json,
        )
    }

    pub fn active_window_context_is_excluded(
        &self,
        app_name: &str,
        url: Option<&str>,
        workspace_key: Option<&str>,
    ) -> Result<bool> {
        let settings = self.get_settings()?;
        Ok(active_window_context_is_excluded(
            app_name,
            url,
            workspace_key,
            &settings,
        ))
    }

    pub fn record_source_event(&self, input: SourceEventInput) -> Result<SourceEvent> {
        let caller_provided_id = input.id.as_ref().is_some_and(|id| !id.trim().is_empty());
        let source = input.source.trim();
        anyhow::ensure!(!source.is_empty(), "source event source is required");
        let event_type = input.event_type.trim();
        anyhow::ensure!(!event_type.is_empty(), "source event type is required");

        let now = now_ms();
        let started_at = input.started_at.unwrap_or(now);
        let ended_at = input.ended_at.unwrap_or(started_at);
        anyhow::ensure!(
            ended_at >= started_at,
            "source event ended_at must be greater than or equal to started_at"
        );
        let (domain, url_redacted) = input.url.as_deref().map(redact_url).unwrap_or((None, None));
        let event = SourceEvent {
            id: input
                .id
                .filter(|id| !id.trim().is_empty())
                .unwrap_or_else(|| format!("source-event-{}", Utc::now().timestamp_micros())),
            source: source.to_string(),
            event_type: event_type.to_string(),
            app: input.app.filter(|value| !value.trim().is_empty()),
            title: input.title.filter(|value| !value.trim().is_empty()),
            domain,
            url_redacted,
            workspace_key: input.workspace_key.filter(|value| !value.trim().is_empty()),
            started_at,
            ended_at,
            duration_ms: ended_at.saturating_sub(started_at),
            sensitivity: input.sensitivity.unwrap_or_else(|| "normal".to_string()),
            metadata_json: input.metadata_json,
            created_at: now,
        };
        let settings = self.get_settings()?;
        if event
            .app
            .as_deref()
            .is_some_and(|value| is_excluded(value, &settings.excluded_apps))
        {
            return Ok(event);
        }
        if event
            .domain
            .as_deref()
            .is_some_and(|value| is_excluded(value, &settings.excluded_domains))
        {
            return Ok(event);
        }
        if event
            .workspace_key
            .as_deref()
            .is_some_and(|value| is_project_excluded(value, &settings.excluded_projects))
        {
            return Ok(event);
        }

        let conn = self.lock()?;
        if !caller_provided_id {
            if let Some(coalesced) = Self::coalesce_source_event_locked(&conn, &event)? {
                if let Some(parts) = workspace_context_parts_from_source_event(&coalesced) {
                    Self::upsert_workspace_context_locked(
                        &conn,
                        parts.context_key,
                        parts.context_type,
                        parts.label,
                        parts.folder_path,
                        parts.domain,
                        None,
                    )?;
                }
                Self::auto_link_event_locked(&conn, &coalesced)?;
                return Ok(coalesced);
            }
        }
        conn.execute(
            r#"
            INSERT INTO source_events
                (id, source, event_type, app, title, domain, url_redacted, workspace_key,
                 started_at, ended_at, duration_ms, sensitivity, metadata_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
            params![
                &event.id,
                &event.source,
                &event.event_type,
                &event.app,
                &event.title,
                &event.domain,
                &event.url_redacted,
                &event.workspace_key,
                event.started_at,
                event.ended_at,
                event.duration_ms,
                &event.sensitivity,
                &event.metadata_json,
                event.created_at,
            ],
        )?;
        if let Some(parts) = workspace_context_parts_from_source_event(&event) {
            Self::upsert_workspace_context_locked(
                &conn,
                parts.context_key,
                parts.context_type,
                parts.label,
                parts.folder_path,
                parts.domain,
                None,
            )?;
        }
        Self::auto_link_event_locked(&conn, &event)?;
        Ok(event)
    }

    pub fn ingest_browser_event(&self, event: BrowserBridgeEvent) -> Result<bool> {
        let (domain, url_redacted) = event.url.as_deref().map(redact_url).unwrap_or((None, None));
        if self.pause_state()?.paused {
            return Ok(false);
        }
        let settings = self.get_settings()?;
        if !settings.browser_bridge_enabled || event.incognito.unwrap_or(false) {
            return Ok(false);
        }
        if domain
            .as_deref()
            .is_some_and(|value| is_excluded(value, &settings.excluded_domains))
        {
            return Ok(false);
        }

        let sanitized_event = BrowserBridgeEvent {
            url: url_redacted.clone(),
            title: event.title.clone(),
            source: event.source.clone(),
            captured_at: event.captured_at.clone(),
            tab_id: event.tab_id,
            window_id: event.window_id,
            incognito: event.incognito,
        };
        let captured_at = event
            .captured_at
            .as_deref()
            .and_then(parse_rfc3339_ms)
            .unwrap_or_else(now_ms);

        let source_event = self.record_source_event(SourceEventInput {
            id: None,
            source: event
                .source
                .clone()
                .unwrap_or_else(|| "browser".to_string()),
            event_type: "browser_tab".to_string(),
            app: Some("browser".to_string()),
            title: event.title.clone(),
            url: event.url.clone(),
            workspace_key: domain,
            started_at: Some(captured_at),
            ended_at: Some(captured_at),
            sensitivity: Some("normal".to_string()),
            metadata_json: Some(serde_json::to_string(&sanitized_event)?),
        })?;

        for tool_name in detect_ai_tools(
            source_event.domain.as_deref(),
            source_event.title.as_deref(),
            source_event.url_redacted.as_deref(),
            source_event.metadata_json.as_deref(),
        ) {
            self.record_ai_usage(AiUsageInput {
                id: Some(format!(
                    "ai-{}-{}",
                    stable_id_part(tool_name),
                    source_event.id
                )),
                provider: Some(tool_name.to_string()),
                tool_name: Some(tool_name.to_string()),
                thread_title: source_event.title.clone(),
                context_id: source_event
                    .workspace_key
                    .as_deref()
                    .map(context_id_from_key),
                prompt_summary: None,
                output_summary: None,
                started_at: Some(source_event.started_at),
                ended_at: Some(source_event.ended_at),
                duration_ms: Some(source_event.duration_ms),
                metadata_json: source_event.metadata_json.clone(),
            })?;
        }

        self.record_activity(
            "browser_tab",
            event.source.as_deref(),
            event.title.as_deref(),
            url_redacted.as_deref(),
            None,
            Some(&serde_json::to_string(&sanitized_event)?),
        )?;
        Ok(true)
    }

    pub fn ingest_editor_context_event(&self, event: Value) -> Result<bool> {
        if self.pause_state()?.paused {
            return Ok(false);
        }

        let settings = self.get_settings()?;
        let app = json_string(&event, &["app"]).unwrap_or_else(|| "Visual Studio Code".into());
        if is_excluded(&app, &settings.excluded_apps) {
            return Ok(false);
        }

        let document = event.get("document").unwrap_or(&Value::Null);
        let workspace = event.get("workspace").unwrap_or(&Value::Null);
        let workspace_key = workspace
            .get("folders")
            .and_then(Value::as_array)
            .and_then(|folders| folders.iter().find_map(Value::as_str))
            .map(ToOwned::to_owned)
            .or_else(|| json_string(document, &["filePath"]))
            .or_else(|| json_string(document, &["uri"]))
            .or_else(|| json_string(workspace, &["name"]));

        if workspace_key
            .as_deref()
            .is_some_and(|value| is_project_excluded(value, &settings.excluded_projects))
        {
            return Ok(false);
        }

        let title = json_string(document, &["fileName"])
            .or_else(|| {
                json_string(document, &["filePath"]).map(|value| file_name_from_path(&value))
            })
            .or_else(|| json_string(workspace, &["name"]))
            .unwrap_or_else(|| "Editor context".into());
        let source = json_string(&event, &["source"]).unwrap_or_else(|| "vscode-extension".into());
        let event_type =
            json_string(&event, &["eventType"]).unwrap_or_else(|| "active_editor_changed".into());
        let sensitivity = json_string(&event, &["sensitivity"]).unwrap_or_else(|| "normal".into());
        let captured_at = json_string(&event, &["capturedAt"])
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
            .map(|value| value.timestamp_millis())
            .unwrap_or_else(now_ms);
        let uri = json_string(document, &["uri"]);
        let cursor_position = document
            .get("cursor")
            .map(serde_json::to_string)
            .transpose()?;
        let mut sanitized_event = event.clone();
        if let Some(uri) = uri.as_deref() {
            if let Some(redacted_uri) = redact_url(uri).1 {
                if let Some(document) = sanitized_event
                    .get_mut("document")
                    .and_then(Value::as_object_mut)
                {
                    document.insert("uri".into(), Value::String(redacted_uri));
                }
            }
        }
        let metadata_json = serde_json::to_string(&sanitized_event)?;

        self.record_source_event(SourceEventInput {
            id: None,
            source,
            event_type: "editor_context".into(),
            app: Some(app.clone()),
            title: Some(title.clone()),
            url: uri,
            workspace_key: workspace_key.clone(),
            started_at: Some(captured_at),
            ended_at: Some(captured_at),
            sensitivity: Some(sensitivity),
            metadata_json: Some(metadata_json.clone()),
        })?;

        let context_key = workspace_key.as_deref().unwrap_or(&app);
        self.create_state_snapshot(StateSnapshotInput {
            id: None,
            context_id: context_id_from_key(context_key),
            trigger_type: event_type,
            snapshot_type: "active_editor".into(),
            summary: Some(format!("Editing {}", clean_report_text(&title))),
            terminal_tail: None,
            git_diff_summary: None,
            active_file: json_string(document, &["filePath"])
                .or_else(|| json_string(document, &["fileName"])),
            cursor_position,
            ai_context_summary: None,
            metadata_json: Some(metadata_json),
        })?;

        Ok(true)
    }

    pub fn store_project_context(&self, context: &ProjectContext) -> Result<()> {
        self.upsert_workspace_context(
            &context.path,
            "workspace",
            Some(&context.path),
            Some(&context.path),
            None,
            Some(&serde_json::to_string(context)?),
        )?;
        self.record_activity(
            "project_detected",
            Some(&context.source),
            None,
            None,
            Some(&context.path),
            Some(&serde_json::to_string(context)?),
        )
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("sqlite store lock poisoned"))
    }

    fn backup_dir(&self) -> Result<PathBuf> {
        let parent = self
            .db_path
            .parent()
            .context("active database path has no parent directory")?;
        Ok(parent.join("backups"))
    }

    fn editor_bridge_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".daytrail/editor-bridge.jsonl"));
            paths.push(home.join(".worktrace/editor-bridge.jsonl"));
        }
        Ok(paths)
    }

    fn terminal_bridge_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        if let Some(path) = self.get_settings()?.terminal_bridge_path {
            paths.push(PathBuf::from(path));
        }
        paths.extend(default_project_sources().terminal_bridge_metadata_paths);
        dedupe_paths(paths)
    }

    fn bridge_cursor(&self, path: &str) -> Result<i64> {
        let conn = self.lock()?;
        Ok(conn
            .query_row(
                "SELECT bytes_read FROM bridge_cursors WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    fn set_bridge_cursor(&self, path: &str, bytes_read: u64) -> Result<()> {
        let bytes_read = i64::try_from(bytes_read).unwrap_or(i64::MAX);
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO bridge_cursors (path, bytes_read, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(path) DO UPDATE SET
                bytes_read = excluded.bytes_read,
                updated_at = excluded.updated_at
            "#,
            params![path, bytes_read, now_utc()],
        )?;
        Ok(())
    }

    fn upsert_setting_locked(conn: &Connection, key: &str, value: &str, now: &str) -> Result<()> {
        conn.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
            "#,
            params![key, value, now],
        )?;
        Ok(())
    }

    fn get_task_locked(conn: &Connection, id: i64) -> Result<Task> {
        conn.query_row(
            r#"
            SELECT id, title, status, due_date, due_at, notes, priority, source,
                   project_path, client_label, project_label, reminder_sent_at,
                   completed_at, created_at, updated_at
            FROM tasks
            WHERE id = ?1
            "#,
            params![id],
            Self::task_from_row,
        )
        .optional()?
        .with_context(|| format!("task not found: {id}"))
    }

    fn get_quick_note_locked(conn: &Connection, id: i64) -> Result<QuickNote> {
        conn.query_row(
            r#"
            SELECT id, body, source, project_path, created_at
            FROM quick_notes
            WHERE id = ?1
            "#,
            params![id],
            Self::quick_note_from_row,
        )
        .optional()?
        .with_context(|| format!("quick note not found: {id}"))
    }

    fn get_commitment_locked(conn: &Connection, id: &str) -> Result<Commitment> {
        conn.query_row(
            r#"
            SELECT id, title, source, owner, due_at, status, confidence, evidence_json, created_at, updated_at
            FROM commitments
            WHERE id = ?1
            "#,
            params![id],
            Self::commitment_from_row,
        )
        .optional()?
        .with_context(|| format!("commitment not found: {id}"))
    }

    fn get_email_thread_locked(conn: &Connection, id: &str) -> Result<EmailThread> {
        conn.query_row(
            r#"
            SELECT id, subject, latest_sender, latest_at, pending_reply, evidence_json, created_at, updated_at
            FROM email_threads
            WHERE id = ?1
            "#,
            params![id],
            Self::email_thread_from_row,
        )
        .optional()?
        .with_context(|| format!("email thread not found: {id}"))
    }

    fn coalesce_source_event_locked(
        conn: &Connection,
        event: &SourceEvent,
    ) -> Result<Option<SourceEvent>> {
        let previous: Option<(String, i64, i64)> = conn
            .query_row(
                r#"
                SELECT id, started_at, ended_at
                FROM source_events
                WHERE source = ?1
                  AND event_type = ?2
                  AND app IS ?3
                  AND title IS ?4
                  AND domain IS ?5
                  AND url_redacted IS ?6
                  AND workspace_key IS ?7
                  AND sensitivity = ?8
                  AND ?9 >= started_at
                  AND (?9 - ended_at) <= ?10
                ORDER BY ended_at DESC
                LIMIT 1
                "#,
                params![
                    &event.source,
                    &event.event_type,
                    &event.app,
                    &event.title,
                    &event.domain,
                    &event.url_redacted,
                    &event.workspace_key,
                    &event.sensitivity,
                    event.started_at,
                    SOURCE_EVENT_COALESCE_GAP_MS,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;

        let Some((id, previous_started_at, previous_ended_at)) = previous else {
            return Ok(None);
        };
        let started_at = previous_started_at.min(event.started_at);
        let ended_at = previous_ended_at.max(event.ended_at);
        conn.execute(
            r#"
            UPDATE source_events
            SET started_at = ?1,
                ended_at = ?2,
                duration_ms = ?3,
                metadata_json = COALESCE(?4, metadata_json)
            WHERE id = ?5
            "#,
            params![
                started_at,
                ended_at,
                ended_at.saturating_sub(started_at),
                &event.metadata_json,
                &id,
            ],
        )?;
        Self::source_event_by_id_locked(conn, &id).map(Some)
    }

    fn source_event_by_id_locked(conn: &Connection, id: &str) -> Result<SourceEvent> {
        conn.query_row(
            r#"
            SELECT id, source, event_type, app, title, domain, url_redacted, workspace_key,
                   started_at, ended_at, duration_ms, sensitivity, metadata_json, created_at
            FROM source_events
            WHERE id = ?1
            "#,
            params![id],
            Self::source_event_from_row,
        )
        .optional()?
        .with_context(|| format!("source event not found: {id}"))
    }

    fn agent_run_by_id_locked(conn: &Connection, id: &str) -> Result<AgentRun> {
        conn.query_row(
            r#"
            SELECT id, context_id, tool_name, command_label, started_at, ended_at, status,
                   exit_code, summary, error_tail, notified, metadata_json
            FROM agent_runs
            WHERE id = ?1
            "#,
            params![id],
            Self::agent_run_from_row,
        )
        .optional()?
        .with_context(|| format!("agent run not found: {id}"))
    }

    fn detect_loop_risks_locked(conn: &Connection) -> Result<Vec<LoopRisk>> {
        let mut risks = Vec::new();
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, subject, latest_sender, evidence_json
                FROM email_threads
                WHERE pending_reply = 1
                ORDER BY COALESCE(latest_at, updated_at), updated_at DESC
                LIMIT 20
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(LoopRisk {
                    id: format!("reply-{}", row.get::<_, String>(0)?),
                    risk_type: "reply_debt".into(),
                    title: format!("Reply to {}", row.get::<_, String>(1)?),
                    source: row
                        .get::<_, Option<String>>(2)?
                        .unwrap_or_else(|| "inbox".into()),
                    reason: "A source record marked this thread as needing a reply".into(),
                    priority: 100,
                    evidence_json: row.get(3)?,
                })
            })?;
            for row in rows {
                risks.push(row?);
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, title, output_type, status, evidence_json
                FROM outputs
                WHERE status IN ('drafted', 'needs_review') OR status IS NULL
                ORDER BY updated_at DESC
                LIMIT 20
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                let status = row
                    .get::<_, Option<String>>(3)?
                    .unwrap_or_else(|| "drafted".into());
                Ok(LoopRisk {
                    id: format!("output-{}", row.get::<_, String>(0)?),
                    risk_type: "ai_output_open".into(),
                    title: row.get(1)?,
                    source: row.get(2)?,
                    reason: format!("AI-assisted output is still {status}"),
                    priority: 92,
                    evidence_json: row.get(4)?,
                })
            })?;
            for row in rows {
                risks.push(row?);
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, title, source, evidence_json
                FROM commitments
                WHERE status = 'open' AND due_at IS NOT NULL AND due_at <= ?1
                ORDER BY due_at
                LIMIT 20
                "#,
            )?;
            let rows = stmt.query_map(params![now_ms()], |row| {
                Ok(LoopRisk {
                    id: format!("commitment-{}", row.get::<_, String>(0)?),
                    risk_type: "promise_overdue".into(),
                    title: row.get(1)?,
                    source: row
                        .get::<_, Option<String>>(2)?
                        .unwrap_or_else(|| "commitment".into()),
                    reason: "Promise due time has passed without closure".into(),
                    priority: 95,
                    evidence_json: row.get(3)?,
                })
            })?;
            for row in rows {
                risks.push(row?);
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, title, actions_json
                FROM meetings
                WHERE actions_json IS NOT NULL AND TRIM(actions_json) NOT IN ('', '[]')
                ORDER BY COALESCE(ends_at, updated_at) DESC
                LIMIT 20
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(LoopRisk {
                    id: format!("meeting-{}", row.get::<_, String>(0)?),
                    risk_type: "meeting_actions_open".into(),
                    title: row.get(1)?,
                    source: "meeting".into(),
                    reason: "Meeting has captured actions that need closure".into(),
                    priority: 86,
                    evidence_json: row.get(2)?,
                })
            })?;
            for row in rows {
                risks.push(row?);
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, client_label, location_label, debrief, status
                FROM field_visits
                WHERE status != 'completed' OR debrief IS NULL OR TRIM(debrief) = ''
                ORDER BY starts_at DESC
                LIMIT 20
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                let client_label: Option<String> = row.get(1)?;
                let location_label: Option<String> = row.get(2)?;
                Ok(LoopRisk {
                    id: format!("field-{}", row.get::<_, String>(0)?),
                    risk_type: "field_debrief_missing".into(),
                    title: client_label
                        .or(location_label)
                        .unwrap_or_else(|| "Client visit".into()),
                    source: "field_visit".into(),
                    reason: "Field/client visit needs debrief and follow-up extraction".into(),
                    priority: 84,
                    evidence_json: None,
                })
            })?;
            for row in rows {
                risks.push(row?);
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, duration_ms, evidence_json
                FROM idle_blocks
                WHERE classified = 0
                  AND COALESCE(evidence_json, '') NOT LIKE '%source_event_gap%'
                  AND NOT EXISTS (
                      SELECT 1
                      FROM loop_item_actions
                      WHERE loop_item_actions.id = 'risk-idle_unclassified-idle-' || idle_blocks.id
                        AND (
                            action IN ('closed', 'ignored')
                            OR (action = 'snoozed' AND snoozed_until IS NOT NULL AND snoozed_until > ?1)
                        )
                  )
                ORDER BY started_at DESC
                LIMIT 20
                "#,
            )?;
            let rows = stmt.query_map(params![now_ms()], |row| {
                let duration_ms: i64 = row.get(1)?;
                Ok(LoopRisk {
                    id: format!("idle-{}", row.get::<_, String>(0)?),
                    risk_type: "idle_unclassified".into(),
                    title: format!(
                        "Classify {}m away block",
                        duration_ms.saturating_div(60_000).max(1)
                    ),
                    source: "idle_recovery".into(),
                    reason: "Away time may hide meeting, travel, call, or planning work".into(),
                    priority: 82,
                    evidence_json: row.get(2)?,
                })
            })?;
            for row in rows {
                risks.push(row?);
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, tool_name, command_label, status, error_tail, metadata_json
                FROM agent_runs
                WHERE notified = 0 AND status IN ('failed', 'stalled', 'timeout')
                ORDER BY COALESCE(ended_at, started_at) DESC
                LIMIT 20
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                let tool_name: Option<String> = row.get(1)?;
                let command_label: Option<String> = row.get(2)?;
                let status: Option<String> = row.get(3)?;
                Ok(LoopRisk {
                    id: format!("agent-{}", row.get::<_, String>(0)?),
                    risk_type: "ghost_agent".into(),
                    title: command_label
                        .clone()
                        .or(tool_name.clone())
                        .unwrap_or_else(|| "Local agent run".into()),
                    source: tool_name.unwrap_or_else(|| "agent".into()),
                    reason: format!(
                        "Local agent/test/build {} while you were elsewhere",
                        status.unwrap_or_else(|| "stalled".into())
                    ),
                    priority: 90,
                    evidence_json: row.get(5)?,
                })
            })?;
            for row in rows {
                risks.push(row?);
            }
        }

        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, active_file, summary, git_diff_summary, metadata_json
                FROM state_snapshots
                WHERE lower(COALESCE(summary, '') || ' ' || COALESCE(git_diff_summary, '') || ' ' || COALESCE(metadata_json, ''))
                      LIKE '%hypothesis%'
                   OR lower(COALESCE(summary, '') || ' ' || COALESCE(git_diff_summary, '') || ' ' || COALESCE(metadata_json, ''))
                      LIKE '%commented out%'
                   OR lower(COALESCE(summary, '') || ' ' || COALESCE(git_diff_summary, '') || ' ' || COALESCE(metadata_json, ''))
                      LIKE '%temporary%'
                ORDER BY created_at DESC
                LIMIT 10
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                let active_file: Option<String> = row.get(1)?;
                let summary: Option<String> = row.get(2)?;
                let git_diff_summary: Option<String> = row.get(3)?;
                Ok(LoopRisk {
                    id: format!("stale-{}", row.get::<_, String>(0)?),
                    risk_type: "stale_hypothesis".into(),
                    title: active_file.unwrap_or_else(|| "Experimental change".into()),
                    source: "state_snapshot".into(),
                    reason: summary.or(git_diff_summary).unwrap_or_else(|| {
                        "Experimental clue may need revert or validation".into()
                    }),
                    priority: 88,
                    evidence_json: row.get(4)?,
                })
            })?;
            for row in rows {
                risks.push(row?);
            }
        }

        risks.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.title.cmp(&right.title))
        });
        risks.truncate(50);
        Ok(risks)
    }

    fn upsert_workspace_context_locked(
        conn: &Connection,
        context_key: &str,
        context_type: &str,
        label: Option<&str>,
        folder_path: Option<&str>,
        domain: Option<&str>,
        metadata_json: Option<&str>,
    ) -> Result<String> {
        let now = now_ms();
        let stable = stable_id_part(context_key);
        let id = if stable.is_empty() {
            format!("context-{}", Utc::now().timestamp_micros())
        } else {
            format!("context-{stable}")
        };
        conn.execute(
            r#"
            INSERT INTO workspace_contexts
                (id, context_key, context_type, label, folder_path, domain, last_seen_at,
                 metadata_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, ?7)
            ON CONFLICT(context_key) DO UPDATE SET
                context_type = excluded.context_type,
                label = COALESCE(excluded.label, workspace_contexts.label),
                folder_path = COALESCE(excluded.folder_path, workspace_contexts.folder_path),
                domain = COALESCE(excluded.domain, workspace_contexts.domain),
                last_seen_at = excluded.last_seen_at,
                metadata_json = COALESCE(excluded.metadata_json, workspace_contexts.metadata_json),
                updated_at = excluded.updated_at
            "#,
            params![
                &id,
                context_key,
                context_type,
                label,
                folder_path,
                domain,
                now,
                metadata_json,
            ],
        )?;
        let existing_id = conn.query_row(
            "SELECT id FROM workspace_contexts WHERE context_key = ?1",
            params![context_key],
            |row| row.get::<_, String>(0),
        )?;
        Ok(existing_id)
    }

    fn workspace_context_by_id_locked(conn: &Connection, id: &str) -> Result<WorkspaceContext> {
        conn.query_row(
            r#"
            SELECT id, context_key, context_type, label, git_repo, git_branch, folder_path,
                   domain, email_thread_id, project_id, last_seen_at, metadata_json,
                   created_at, updated_at
            FROM workspace_contexts
            WHERE id = ?1
            "#,
            params![id],
            Self::workspace_context_from_row,
        )
        .optional()?
        .with_context(|| format!("workspace context not found: {id}"))
    }

    fn scratchpad_note_by_id_locked(conn: &Connection, id: &str) -> Result<ScratchpadNote> {
        conn.query_row(
            r#"
            SELECT id, context_id, note, pinned, created_at, updated_at
            FROM scratchpad_notes
            WHERE id = ?1
            "#,
            params![id],
            Self::scratchpad_note_from_row,
        )
        .optional()?
        .with_context(|| format!("scratchpad note not found: {id}"))
    }

    fn state_snapshot_by_id_locked(conn: &Connection, id: &str) -> Result<StateSnapshot> {
        conn.query_row(
            r#"
            SELECT id, context_id, trigger_type, snapshot_type, summary, terminal_tail,
                   git_diff_summary, active_file, cursor_position, ai_context_summary,
                   metadata_json, created_at
            FROM state_snapshots
            WHERE id = ?1
            "#,
            params![id],
            Self::state_snapshot_from_row,
        )
        .optional()?
        .with_context(|| format!("state snapshot not found: {id}"))
    }

    fn ai_usage_by_id_locked(conn: &Connection, id: &str) -> Result<AiUsage> {
        conn.query_row(
            r#"
            SELECT id, provider, tool_name, thread_title, context_id, prompt_summary,
                   output_summary, started_at, ended_at, duration_ms, metadata_json, created_at
            FROM ai_usage
            WHERE id = ?1
            "#,
            params![id],
            Self::ai_usage_from_row,
        )
        .optional()?
        .with_context(|| format!("AI usage not found: {id}"))
    }

    fn work_output_by_id_locked(conn: &Connection, id: &str) -> Result<WorkOutput> {
        conn.query_row(
            r#"
            SELECT id, output_type, title, source, ai_assisted, status, evidence_json,
                   created_at, updated_at
            FROM outputs
            WHERE id = ?1
            "#,
            params![id],
            Self::work_output_from_row,
        )
        .optional()?
        .with_context(|| format!("work output not found: {id}"))
    }

    fn meeting_by_id_locked(conn: &Connection, id: &str) -> Result<Meeting> {
        conn.query_row(
            r#"
            SELECT id, title, starts_at, ends_at, attendees_json, summary, actions_json,
                   created_at, updated_at
            FROM meetings
            WHERE id = ?1
            "#,
            params![id],
            Self::meeting_from_row,
        )
        .optional()?
        .with_context(|| format!("meeting not found: {id}"))
    }

    fn calendar_event_by_id_locked(conn: &Connection, id: &str) -> Result<CalendarEvent> {
        conn.query_row(
            r#"
            SELECT id, source, external_id, calendar_name, title, starts_at, ends_at,
                   location, status, planned_work_type, created_at, updated_at
            FROM calendar_events
            WHERE id = ?1
            "#,
            params![id],
            Self::calendar_event_from_row,
        )
        .optional()?
        .with_context(|| format!("calendar event not found: {id}"))
    }

    fn focus_session_base_by_id_locked(conn: &Connection, id: &str) -> Result<FocusSessionRecord> {
        conn.query_row(
            r#"
            SELECT id, goal, client, project, task, ticket_id, target_ms, started_at,
                   ended_at, status, created_at, updated_at
            FROM focus_sessions
            WHERE id = ?1
            "#,
            params![id],
            Self::focus_session_base_from_row,
        )
        .optional()?
        .with_context(|| format!("focus session not found: {id}"))
    }

    fn field_visit_by_id_locked(conn: &Connection, id: &str) -> Result<FieldVisit> {
        conn.query_row(
            r#"
            SELECT id, client_label, starts_at, ends_at, location_label, debrief, status,
                   created_at, updated_at
            FROM field_visits
            WHERE id = ?1
            "#,
            params![id],
            Self::field_visit_from_row,
        )
        .optional()?
        .with_context(|| format!("field visit not found: {id}"))
    }

    fn idle_block_by_id_locked(conn: &Connection, id: &str) -> Result<IdleBlock> {
        conn.query_row(
            r#"
            SELECT id, started_at, ended_at, duration_ms, category, classified, evidence_json,
                   created_at, updated_at
            FROM idle_blocks
            WHERE id = ?1
            "#,
            params![id],
            Self::idle_block_from_row,
        )
        .optional()?
        .with_context(|| format!("idle block not found: {id}"))
    }

    fn loop_action_by_id_locked(conn: &Connection, id: &str) -> Result<LoopAction> {
        conn.query_row(
            r#"
            SELECT id, action, snoozed_until, created_at, updated_at
            FROM loop_item_actions
            WHERE id = ?1
            "#,
            params![id],
            Self::loop_action_from_row,
        )
        .optional()?
        .with_context(|| format!("loop action not found: {id}"))
    }

    fn hidden_loop_ids_locked(conn: &Connection) -> Result<HashSet<String>> {
        let now = now_ms();
        let mut stmt = conn.prepare(
            r#"
            SELECT id
            FROM loop_item_actions
            WHERE action IN ('closed', 'ignored')
               OR (action = 'snoozed' AND snoozed_until IS NOT NULL AND snoozed_until > ?1)
            "#,
        )?;
        let rows = stmt.query_map(params![now], |row| row.get::<_, String>(0))?;
        let mut ids = HashSet::new();
        for id in rows {
            ids.insert(id?);
        }
        Ok(ids)
    }

    fn latest_state_snapshot_locked(
        conn: &Connection,
        context_id: &str,
    ) -> Result<Option<StateSnapshot>> {
        conn.query_row(
            r#"
            SELECT id, context_id, trigger_type, snapshot_type, summary, terminal_tail,
                   git_diff_summary, active_file, cursor_position, ai_context_summary,
                   metadata_json, created_at
            FROM state_snapshots
            WHERE context_id = ?1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            params![context_id],
            Self::state_snapshot_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    fn scratchpad_notes_locked(
        conn: &Connection,
        context_id: &str,
        pinned: bool,
        limit: usize,
    ) -> Result<Vec<ScratchpadNote>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, context_id, note, pinned, created_at, updated_at
            FROM scratchpad_notes
            WHERE context_id = ?1 AND pinned = ?2
            ORDER BY updated_at DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![context_id, if pinned { 1 } else { 0 }, limit as i64],
            Self::scratchpad_note_from_row,
        )?;
        let mut notes = Vec::new();
        for note in rows {
            notes.push(note?);
        }
        Ok(notes)
    }

    fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
        let status: String = row.get(2)?;
        let status = TaskStatus::try_from(status.as_str()).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    err.to_string(),
                )),
            )
        })?;

        Ok(Task {
            id: row.get(0)?,
            title: row.get(1)?,
            status,
            due_date: row.get(3)?,
            due_at: row.get(4)?,
            notes: row.get(5)?,
            priority: row.get(6)?,
            source: row.get(7)?,
            project_path: row.get(8)?,
            client_label: row.get(9)?,
            project_label: row.get(10)?,
            reminder_sent_at: row.get(11)?,
            completed_at: row.get(12)?,
            created_at: row.get(13)?,
            updated_at: row.get(14)?,
        })
    }

    fn quick_note_from_row(row: &Row<'_>) -> rusqlite::Result<QuickNote> {
        Ok(QuickNote {
            id: row.get(0)?,
            body: row.get(1)?,
            source: row.get(2)?,
            project_path: row.get(3)?,
            created_at: row.get(4)?,
        })
    }

    fn commitment_from_row(row: &Row<'_>) -> rusqlite::Result<Commitment> {
        Ok(Commitment {
            id: row.get(0)?,
            title: row.get(1)?,
            source: row.get(2)?,
            owner: row.get(3)?,
            due_at: row.get(4)?,
            status: row.get(5)?,
            confidence: row.get(6)?,
            evidence_json: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    }

    fn email_thread_from_row(row: &Row<'_>) -> rusqlite::Result<EmailThread> {
        Ok(EmailThread {
            id: row.get(0)?,
            subject: row.get(1)?,
            latest_sender: row.get(2)?,
            latest_at: row.get(3)?,
            pending_reply: row.get::<_, i64>(4)? == 1,
            evidence_json: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }

    fn source_event_from_row(row: &Row<'_>) -> rusqlite::Result<SourceEvent> {
        Ok(SourceEvent {
            id: row.get(0)?,
            source: row.get(1)?,
            event_type: row.get(2)?,
            app: row.get(3)?,
            title: row.get(4)?,
            domain: row.get(5)?,
            url_redacted: row.get(6)?,
            workspace_key: row.get(7)?,
            started_at: row.get(8)?,
            ended_at: row.get(9)?,
            duration_ms: row.get(10)?,
            sensitivity: row.get(11)?,
            metadata_json: row.get(12)?,
            created_at: row.get(13)?,
        })
    }

    fn recovery_event_from_row(row: &Row<'_>) -> rusqlite::Result<RecoveryEvent> {
        Ok(RecoveryEvent {
            id: row.get(0)?,
            event_type: row.get(1)?,
            started_at: row.get(2)?,
            ended_at: row.get(3)?,
            duration_ms: row.get(4)?,
            note: row.get(5)?,
            evidence_json: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }

    fn recovery_event_by_id_locked(conn: &Connection, id: &str) -> Result<RecoveryEvent> {
        conn.query_row(
            r#"
            SELECT id, event_type, started_at, ended_at, duration_ms, note,
                   evidence_json, created_at, updated_at
            FROM recovery_events
            WHERE id = ?1
            "#,
            params![id],
            Self::recovery_event_from_row,
        )
        .map_err(Into::into)
    }

    fn workspace_context_from_row(row: &Row<'_>) -> rusqlite::Result<WorkspaceContext> {
        Ok(WorkspaceContext {
            id: row.get(0)?,
            context_key: row.get(1)?,
            context_type: row.get(2)?,
            label: row.get(3)?,
            git_repo: row.get(4)?,
            git_branch: row.get(5)?,
            folder_path: row.get(6)?,
            domain: row.get(7)?,
            email_thread_id: row.get(8)?,
            project_id: row.get(9)?,
            last_seen_at: row.get(10)?,
            metadata_json: row.get(11)?,
            created_at: row.get(12)?,
            updated_at: row.get(13)?,
        })
    }

    fn scratchpad_note_from_row(row: &Row<'_>) -> rusqlite::Result<ScratchpadNote> {
        Ok(ScratchpadNote {
            id: row.get(0)?,
            context_id: row.get(1)?,
            note: row.get(2)?,
            pinned: row.get::<_, i64>(3)? == 1,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    }

    fn state_snapshot_from_row(row: &Row<'_>) -> rusqlite::Result<StateSnapshot> {
        Ok(StateSnapshot {
            id: row.get(0)?,
            context_id: row.get(1)?,
            trigger_type: row.get(2)?,
            snapshot_type: row.get(3)?,
            summary: row.get(4)?,
            terminal_tail: row.get(5)?,
            git_diff_summary: row.get(6)?,
            active_file: row.get(7)?,
            cursor_position: row.get(8)?,
            ai_context_summary: row.get(9)?,
            metadata_json: row.get(10)?,
            created_at: row.get(11)?,
        })
    }

    fn ai_usage_from_row(row: &Row<'_>) -> rusqlite::Result<AiUsage> {
        Ok(AiUsage {
            id: row.get(0)?,
            provider: row.get(1)?,
            tool_name: row.get(2)?,
            thread_title: row.get(3)?,
            context_id: row.get(4)?,
            prompt_summary: row.get(5)?,
            output_summary: row.get(6)?,
            started_at: row.get(7)?,
            ended_at: row.get(8)?,
            duration_ms: row.get(9)?,
            metadata_json: row.get(10)?,
            created_at: row.get(11)?,
        })
    }

    fn agent_run_from_row(row: &Row<'_>) -> rusqlite::Result<AgentRun> {
        Ok(AgentRun {
            id: row.get(0)?,
            context_id: row.get(1)?,
            tool_name: row.get(2)?,
            command_label: row.get(3)?,
            started_at: row.get(4)?,
            ended_at: row.get(5)?,
            status: row.get(6)?,
            exit_code: row.get(7)?,
            summary: row.get(8)?,
            error_tail: row.get(9)?,
            notified: row.get::<_, i64>(10)? == 1,
            metadata_json: row.get(11)?,
        })
    }

    fn work_output_from_row(row: &Row<'_>) -> rusqlite::Result<WorkOutput> {
        Ok(WorkOutput {
            id: row.get(0)?,
            output_type: row.get(1)?,
            title: row.get(2)?,
            source: row.get(3)?,
            ai_assisted: row.get::<_, i64>(4)? == 1,
            status: row.get(5)?,
            evidence_json: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }

    fn meeting_from_row(row: &Row<'_>) -> rusqlite::Result<Meeting> {
        Ok(Meeting {
            id: row.get(0)?,
            title: row.get(1)?,
            starts_at: row.get(2)?,
            ends_at: row.get(3)?,
            attendees_json: row.get(4)?,
            summary: row.get(5)?,
            actions_json: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }

    fn calendar_event_from_row(row: &Row<'_>) -> rusqlite::Result<CalendarEvent> {
        Ok(CalendarEvent {
            id: row.get(0)?,
            source: row.get(1)?,
            external_id: row.get(2)?,
            calendar_name: row.get(3)?,
            title: row.get(4)?,
            starts_at: row.get(5)?,
            ends_at: row.get(6)?,
            location: row.get(7)?,
            status: row.get(8)?,
            planned_work_type: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        })
    }

    fn focus_session_base_from_row(row: &Row<'_>) -> rusqlite::Result<FocusSessionRecord> {
        Ok(FocusSessionRecord {
            id: row.get(0)?,
            goal: row.get(1)?,
            client: row.get(2)?,
            project: row.get(3)?,
            task: row.get(4)?,
            ticket_id: row.get(5)?,
            target_ms: row.get(6)?,
            started_at: row.get(7)?,
            ended_at: row.get(8)?,
            status: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        })
    }

    fn field_visit_from_row(row: &Row<'_>) -> rusqlite::Result<FieldVisit> {
        Ok(FieldVisit {
            id: row.get(0)?,
            client_label: row.get(1)?,
            starts_at: row.get(2)?,
            ends_at: row.get(3)?,
            location_label: row.get(4)?,
            debrief: row.get(5)?,
            status: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }

    fn idle_block_from_row(row: &Row<'_>) -> rusqlite::Result<IdleBlock> {
        Ok(IdleBlock {
            id: row.get(0)?,
            started_at: row.get(1)?,
            ended_at: row.get(2)?,
            duration_ms: row.get(3)?,
            category: row.get(4)?,
            classified: row.get::<_, i64>(5)? == 1,
            evidence_json: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }

    fn loop_action_from_row(row: &Row<'_>) -> rusqlite::Result<LoopAction> {
        Ok(LoopAction {
            id: row.get(0)?,
            action: row.get(1)?,
            snoozed_until: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
        })
    }

    fn work_session_from_row(row: &Row<'_>) -> rusqlite::Result<WorkSessionSummary> {
        let evidence_json: Option<String> = row.get(9)?;
        Ok(WorkSessionSummary {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            started_at: row.get(3)?,
            ended_at: row.get(4)?,
            duration_ms: row.get(5)?,
            ai_used: row.get::<_, i64>(6)? == 1,
            confidence_percent: ((row.get::<_, f64>(7)? * 100.0).round() as i64).clamp(0, 100),
            summary: row.get(8)?,
            evidence_event_ids: evidence_json
                .as_deref()
                .and_then(|json| serde_json::from_str(json).ok())
                .unwrap_or_default(),
            billing_status: row
                .get::<_, Option<String>>(10)?
                .unwrap_or_else(|| "draft".to_string()),
            billable: row.get::<_, i64>(11)? != 0,
            client_label: row.get(12)?,
            project_label: row.get(13)?,
            ticket_id: row.get(14)?,
            review_notes: row.get(15)?,
        })
    }

    fn pause_state_locked(conn: &Connection) -> Result<PauseState> {
        Ok(conn.query_row(
            "SELECT paused, reason, updated_at FROM pause_state WHERE id = 1",
            [],
            |row| {
                Ok(PauseState {
                    paused: row.get::<_, i64>(0)? == 1,
                    reason: row.get(1)?,
                    updated_at: row.get(2)?,
                })
            },
        )?)
    }

    fn list_source_events_for_materialization(&self) -> Result<Vec<StoredSourceEvent>> {
        self.list_source_events_for_materialization_between(None, None)
    }

    fn list_source_events_for_materialization_between(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<Vec<StoredSourceEvent>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, source, event_type, app, title, domain, workspace_key, started_at, ended_at, metadata_json
            FROM source_events
            WHERE (?1 IS NULL OR ended_at >= ?1)
              AND (?2 IS NULL OR started_at < ?2)
            ORDER BY started_at, ended_at, id
            "#,
        )?;
        let rows = stmt.query_map(params![from_ms, to_ms], |row| {
            Ok(StoredSourceEvent {
                id: row.get(0)?,
                source: row.get(1)?,
                event_type: row.get(2)?,
                app: row.get(3)?,
                title: row.get(4)?,
                domain: row.get(5)?,
                workspace_key: row.get(6)?,
                started_at: row.get(7)?,
                ended_at: row.get(8)?,
                metadata_json: row.get(9)?,
            })
        })?;

        let mut events = Vec::new();
        for event in rows {
            events.push(event?);
        }
        Ok(events)
    }

    fn stream_event_ids_locked(conn: &Connection, stream_id: &str) -> Result<Vec<String>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT event_id
            FROM stream_events
            WHERE stream_id = ?1
            ORDER BY event_id
            "#,
        )?;
        let rows = stmt.query_map(params![stream_id], |row| row.get::<_, String>(0))?;
        let mut event_ids = Vec::new();
        for event_id in rows {
            event_ids.push(event_id?);
        }
        Ok(event_ids)
    }
}

#[derive(Debug, Clone)]
struct StoredSourceEvent {
    id: String,
    source: String,
    event_type: String,
    app: Option<String>,
    title: Option<String>,
    domain: Option<String>,
    workspace_key: Option<String>,
    started_at: i64,
    ended_at: i64,
    metadata_json: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct WorkspaceContextParts<'a> {
    context_key: &'a str,
    context_type: &'a str,
    label: Option<&'a str>,
    folder_path: Option<&'a str>,
    domain: Option<&'a str>,
}

#[derive(Debug, Clone)]
struct MaterializedSession {
    id: String,
    title: String,
    context_id: Option<String>,
    category: String,
    status: String,
    started_at: i64,
    ended_at: i64,
    duration_ms: i64,
    ai_used: bool,
    confidence: f64,
    summary: Option<String>,
    evidence_event_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct MaterializedStream {
    id: String,
    title: String,
    stream_type: String,
    context_id: Option<String>,
    started_at: i64,
    ended_at: Option<i64>,
    summary: Option<String>,
    confidence: f64,
    event_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct FocusSessionRecord {
    id: String,
    goal: String,
    client: Option<String>,
    project: Option<String>,
    task: Option<String>,
    ticket_id: Option<String>,
    target_ms: i64,
    started_at: i64,
    ended_at: Option<i64>,
    status: String,
    created_at: i64,
    updated_at: i64,
}

fn work_session_summary_from_materialized(session: MaterializedSession) -> WorkSessionSummary {
    WorkSessionSummary {
        id: session.id,
        title: session.title,
        status: session.status,
        started_at: session.started_at,
        ended_at: session.ended_at,
        duration_ms: session.duration_ms,
        ai_used: session.ai_used,
        confidence_percent: (session.confidence * 100.0).round() as i64,
        summary: session.summary,
        evidence_event_ids: session.evidence_event_ids,
        billing_status: "draft".to_string(),
        billable: true,
        client_label: None,
        project_label: None,
        ticket_id: None,
        review_notes: None,
    }
}

fn parallel_stream_summary_from_materialized(stream: MaterializedStream) -> ParallelStreamSummary {
    ParallelStreamSummary {
        id: stream.id,
        title: stream.title,
        status: "active".to_string(),
        started_at: stream.started_at,
        ended_at: stream.ended_at,
        summary: stream.summary,
        event_ids: stream.event_ids,
        next_action: Some("Review latest unresolved event and close the loop.".to_string()),
    }
}

fn workspace_context_parts_from_source_event(
    event: &SourceEvent,
) -> Option<WorkspaceContextParts<'_>> {
    let workspace_key = event.workspace_key.as_deref();
    let domain = event.domain.as_deref();
    let app = event.app.as_deref();
    let context_key = workspace_key.or(domain).or(app)?;

    if domain.is_some_and(|value| Some(value) == workspace_key) {
        return Some(WorkspaceContextParts {
            context_key,
            context_type: "domain",
            label: event.title.as_deref(),
            folder_path: None,
            domain,
        });
    }

    if workspace_key.is_some() {
        return Some(WorkspaceContextParts {
            context_key,
            context_type: "workspace",
            label: event.title.as_deref(),
            folder_path: workspace_key,
            domain,
        });
    }

    Some(WorkspaceContextParts {
        context_key,
        context_type: "app",
        label: app,
        folder_path: None,
        domain,
    })
}

fn build_sessions_from_source_events(
    events: &[StoredSourceEvent],
    idle_gap_ms: i64,
) -> Vec<MaterializedSession> {
    let mut sessions = Vec::new();
    let mut current: Vec<StoredSourceEvent> = Vec::new();
    let mut last_end = None;
    let mut current_anchor: Option<String> = None;

    for event in events {
        let gap_starts_new = last_end
            .map(|ended_at| event.started_at.saturating_sub(ended_at) > idle_gap_ms)
            .unwrap_or(false);
        let event_anchor = session_anchor_key(event);
        let context_starts_new = !current.is_empty()
            && current_anchor.is_some()
            && event_anchor.is_some()
            && current_anchor != event_anchor
            && current
                .first()
                .map(|first| event.started_at.saturating_sub(first.started_at) >= 15 * 60_000)
                .unwrap_or(false);

        if (gap_starts_new || context_starts_new) && !current.is_empty() {
            sessions.push(materialize_session(std::mem::take(&mut current)));
            current_anchor = None;
        }
        last_end = Some(event.ended_at);
        if current_anchor.is_none() {
            current_anchor = event_anchor;
        }
        current.push(event.clone());
    }

    if !current.is_empty() {
        sessions.push(materialize_session(current));
    }
    sessions
}

fn session_anchor_key(event: &StoredSourceEvent) -> Option<String> {
    let workspace = event.workspace_key.as_deref()?;
    if looks_like_project_path(workspace) {
        return Some(file_name_from_path(workspace).to_ascii_lowercase());
    }
    None
}

fn materialize_session(events: Vec<StoredSourceEvent>) -> MaterializedSession {
    let started_at = events
        .first()
        .map(|event| event.started_at)
        .unwrap_or_default();
    let ended_at = events
        .last()
        .map(|event| event.ended_at)
        .unwrap_or(started_at);
    let evidence_event_ids: Vec<String> = events.iter().map(|event| event.id.clone()).collect();
    let title = session_title_from_events(&events);
    let context_id = events.iter().find_map(event_context_id);
    let ai_used = events.iter().any(is_ai_event);
    let category = infer_session_category(&events).to_string();
    let summary = session_summary(&events);

    MaterializedSession {
        id: format!(
            "session-{}",
            evidence_event_ids
                .first()
                .map(String::as_str)
                .unwrap_or("empty")
        ),
        title,
        context_id,
        category,
        status: "completed".to_string(),
        started_at,
        ended_at,
        duration_ms: ended_at.saturating_sub(started_at),
        ai_used,
        confidence: if ai_used { 0.92 } else { 0.86 },
        summary: Some(summary),
        evidence_event_ids,
    }
}

fn build_streams_from_source_events(events: &[StoredSourceEvent]) -> Vec<MaterializedStream> {
    let mut grouped: HashMap<String, Vec<StoredSourceEvent>> = HashMap::new();
    for event in events {
        let key = event_context_key(event).unwrap_or_else(|| event.source.clone());
        grouped.entry(key).or_default().push(event.clone());
    }

    let mut streams: Vec<MaterializedStream> = grouped
        .into_iter()
        .map(|(key, mut events)| {
            events.sort_by_key(|event| (event.started_at, event.ended_at));
            let started_at = events
                .first()
                .map(|event| event.started_at)
                .unwrap_or_default();
            let ended_at = events.last().map(|event| event.ended_at);
            let event_ids: Vec<String> = events.iter().map(|event| event.id.clone()).collect();
            MaterializedStream {
                id: format!("stream-{}", stable_id_part(&key)),
                title: stream_title(&key, &events),
                stream_type: infer_stream_type(&events).to_string(),
                context_id: events.first().and_then(event_context_id),
                started_at,
                ended_at,
                summary: Some(stream_summary(&key, &events)),
                confidence: 0.84,
                event_ids,
            }
        })
        .collect();

    streams.sort_by_key(|stream| std::cmp::Reverse(stream.ended_at.unwrap_or(stream.started_at)));
    streams
}

fn event_context_key(event: &StoredSourceEvent) -> Option<String> {
    event
        .workspace_key
        .clone()
        .or_else(|| event.domain.clone())
        .or_else(|| event.app.clone())
}

fn event_context_id(event: &StoredSourceEvent) -> Option<String> {
    event_context_key(event).map(|key| context_id_from_key(&key))
}

fn detect_ai_tools(
    domain: Option<&str>,
    title: Option<&str>,
    url_redacted: Option<&str>,
    metadata_json: Option<&str>,
) -> Vec<&'static str> {
    let mut tools = Vec::new();
    if let Some(metadata) = metadata_json {
        if let Ok(value) = serde_json::from_str::<Value>(metadata) {
            collect_ai_tools_from_json(&value, &mut tools);
        } else {
            for tool in detect_ai_tools_in_text(metadata) {
                push_static_tool(&mut tools, tool);
            }
        }
    }

    let haystack = [
        domain.unwrap_or_default(),
        title.unwrap_or_default(),
        url_redacted.unwrap_or_default(),
        metadata_json.unwrap_or_default(),
    ]
    .join(" ")
    .to_ascii_lowercase();
    for tool in detect_ai_tools_in_text(&haystack) {
        push_static_tool(&mut tools, tool);
    }

    tools
}

fn detect_ai_tool(
    domain: Option<&str>,
    title: Option<&str>,
    url_redacted: Option<&str>,
    metadata_json: Option<&str>,
) -> Option<&'static str> {
    detect_ai_tools(domain, title, url_redacted, metadata_json)
        .into_iter()
        .next()
}

fn collect_ai_tools_from_json(value: &Value, tools: &mut Vec<&'static str>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_ai_tools_from_json(item, tools);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                let lower = key.to_ascii_lowercase();
                if matches!(key.as_str(), "aiTools" | "ai_tools" | "tool" | "toolName")
                    || lower.contains("ai")
                    || lower.contains("tool")
                {
                    collect_ai_tools_from_json(item, tools);
                }
            }
        }
        Value::String(value) => {
            for tool in detect_ai_tools_in_text(value) {
                push_static_tool(tools, tool);
            }
        }
        _ => {}
    }
}

fn detect_ai_tools_in_text(value: &str) -> Vec<&'static str> {
    let lower = value.to_ascii_lowercase();
    let exact = lower
        .trim()
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '[' | ']' | '{' | '}'));
    let mut tools = Vec::new();
    for (value, label) in [
        ("codex", "Codex"),
        ("copilot", "Copilot"),
        ("chatgpt", "ChatGPT"),
        ("claude code", "Claude Code"),
        ("claude", "Claude"),
        ("gemini", "Gemini"),
        ("continue", "Continue"),
        ("cursor", "Cursor"),
        ("aider", "Aider"),
        ("cline", "Cline"),
        ("windsurf", "Windsurf"),
    ] {
        if exact == value {
            push_static_tool(&mut tools, label);
        }
    }
    for (needles, label) in [
        (
            &["claude code", "claude-code", "anthropic.claude"][..],
            "Claude Code",
        ),
        (
            &["codex app-server", "openai.chatgpt", "/codex"][..],
            "Codex",
        ),
        (
            &[
                "github.copilot",
                "@vscode/copilot",
                "copilot chat",
                "copilot status",
                "copilot generated",
                "github copilot",
                "copilot.microsoft.com",
                "microsoft copilot",
                "/copilot",
            ][..],
            "Copilot",
        ),
        (&["chatgpt.com", "chatgpt", "/chatgpt"][..], "ChatGPT"),
        (&["gemini", "aistudio.google.com"][..], "Gemini"),
        (&["claude", "claude.ai"][..], "Claude"),
        (&["continue.continue"][..], "Continue"),
        (&["cursor.com"][..], "Cursor"),
        (&["aider"][..], "Aider"),
        (&["cline", "claude-dev"][..], "Cline"),
        (&["windsurf"][..], "Windsurf"),
    ] {
        if needles.iter().any(|needle| lower.contains(needle)) {
            if label == "Claude"
                && (lower.contains("claude code")
                    || lower.contains("claude-code")
                    || lower.contains("anthropic.claude"))
            {
                continue;
            }
            push_static_tool(&mut tools, label);
        }
    }
    tools
}

fn push_static_tool(tools: &mut Vec<&'static str>, tool: &'static str) {
    if !tools.contains(&tool) {
        tools.push(tool);
    }
}

fn is_ai_event(event: &StoredSourceEvent) -> bool {
    !detect_ai_tools(
        event.domain.as_deref(),
        event.title.as_deref().or(event.app.as_deref()),
        None,
        event.metadata_json.as_deref(),
    )
    .is_empty()
}

fn infer_session_category(events: &[StoredSourceEvent]) -> &'static str {
    if events.iter().any(is_ai_event) {
        "ai_assisted"
    } else if events
        .iter()
        .any(|event| event.event_type.contains("browser"))
    {
        "research"
    } else {
        "focus_work"
    }
}

fn infer_stream_type(events: &[StoredSourceEvent]) -> &'static str {
    if events.iter().any(is_ai_event) {
        "ai_thread"
    } else if events
        .iter()
        .any(|event| event.event_type.contains("browser"))
    {
        "browser_context"
    } else {
        "workspace"
    }
}

fn stream_title(key: &str, events: &[StoredSourceEvent]) -> String {
    if looks_like_path(key) {
        return file_name_from_path(key);
    }

    if key.contains('.') && !key.contains('/') {
        return events
            .iter()
            .rev()
            .filter_map(event_preferred_title)
            .find(|title| title != key && !is_generic_app_title(title))
            .unwrap_or_else(|| clean_capture_label(key).unwrap_or_else(|| key.to_string()));
    }

    events
        .iter()
        .rev()
        .filter_map(event_preferred_title)
        .find(|title| !is_generic_app_title(title))
        .or_else(|| clean_app_label(key))
        .unwrap_or_else(|| key.to_string())
}

fn session_title_from_events(events: &[StoredSourceEvent]) -> String {
    let mut document_labels = Vec::new();
    for event in events.iter().rev() {
        if !event.event_type.contains("editor") {
            continue;
        }
        if let Some(label) = event_preferred_title(event) {
            if !is_generic_app_title(&label)
                && !document_labels.iter().any(|existing| existing == &label)
            {
                document_labels.push(label);
            }
        }
        if document_labels.len() == 3 {
            return document_labels.join(" + ");
        }
    }
    if !document_labels.is_empty() {
        return document_labels.join(" + ");
    }

    if let Some(folder) = events
        .iter()
        .rev()
        .filter_map(|event| event.workspace_key.as_deref())
        .find(|value| looks_like_project_path(value))
    {
        return file_name_from_path(folder);
    }

    let mut labels = Vec::new();
    for event in events.iter().rev() {
        if let Some(label) = event_preferred_title(event) {
            if !labels.iter().any(|existing| existing == &label) {
                labels.push(label);
            }
        }
        if labels.len() == 3 {
            break;
        }
    }

    if labels.is_empty() {
        "Captured work session".to_string()
    } else {
        labels.join(" + ")
    }
}

fn session_summary(events: &[StoredSourceEvent]) -> String {
    let event_count = events.len();
    let source_count = format!(
        "{} {}",
        event_count,
        if event_count == 1 { "event" } else { "events" }
    );

    if let Some(folder) = events
        .iter()
        .rev()
        .filter_map(|event| event.workspace_key.as_deref())
        .find(|value| looks_like_project_path(value))
    {
        return format!("Folder: {} - {source_count}", clean_report_text(folder));
    }

    let apps = compact_event_apps(events);
    if apps.is_empty() {
        source_count
    } else {
        format!("{} - {source_count}", apps.join(", "))
    }
}

fn stream_summary(key: &str, events: &[StoredSourceEvent]) -> String {
    let event_count = events.len();
    let source_count = format!(
        "{} {}",
        event_count,
        if event_count == 1 { "event" } else { "events" }
    );

    if looks_like_project_path(key) {
        return format!("Folder: {} - {source_count}", clean_report_text(key));
    }

    if key.contains('.') && !key.contains('/') {
        return format!("Browser: {} - {source_count}", clean_report_text(key));
    }

    let apps = compact_event_apps(events);
    if apps.is_empty() {
        source_count
    } else {
        format!("{} - {source_count}", apps.join(", "))
    }
}

fn compact_event_apps(events: &[StoredSourceEvent]) -> Vec<String> {
    let mut labels = Vec::new();
    for event in events.iter().rev() {
        let label = event
            .app
            .as_deref()
            .and_then(clean_app_label)
            .or_else(|| legacy_app_from_event_id(&event.id));
        if let Some(label) = label {
            if !is_self_app_label(&label) && !labels.iter().any(|existing| existing == &label) {
                labels.push(label);
            }
        }
        if labels.len() == 3 {
            break;
        }
    }
    labels
}

fn event_preferred_title(event: &StoredSourceEvent) -> Option<String> {
    event
        .title
        .as_deref()
        .and_then(clean_capture_label)
        .map(|title| compact_capture_label(&title))
        .filter(|title| !is_self_app_label(title))
        .or_else(|| {
            event
                .workspace_key
                .as_deref()
                .and_then(clean_capture_label)
                .map(|key| compact_capture_label(&key))
        })
        .or_else(|| event.domain.as_deref().and_then(clean_capture_label))
        .or_else(|| event.app.as_deref().and_then(clean_app_label))
        .or_else(|| legacy_app_from_event_id(&event.id))
}

fn compact_capture_label(value: &str) -> String {
    if looks_like_path(value) {
        file_name_from_path(value)
    } else {
        clean_app_label(value).unwrap_or_else(|| clean_report_text(value))
    }
}

fn clean_app_label(value: &str) -> Option<String> {
    let cleaned = clean_capture_label(value)?;
    let normalized = cleaned.to_ascii_lowercase();
    match normalized.as_str() {
        "code" | "visual studio code" => Some("VS Code".to_string()),
        "googlechrome" | "google chrome" => Some("Google Chrome".to_string()),
        "bravebrowser" | "brave browser" => Some("Brave Browser".to_string()),
        "systemsettings" | "system settings" => Some("System Settings".to_string()),
        "daytrail"
        | "daytrail-desktop"
        | "ai.daytrail.desktop"
        | "worktracedesktop"
        | "worktrace ai"
        | "worktrace-ai"
        | "ai.worktrace.desktop" => Some(DISPLAY_APP_NAME.to_string()),
        "warp" | "warpterminal" | "iterm" | "iterm2" => Some("Terminal".to_string()),
        "/bin/zsh" | "/bin/bash" | "zsh" | "bash" | "fish" | "pwsh" | "powershell" | "terminal"
        | "dumb" | "ansi" | "vt100" | "xterm" | "xterm-256color" | "screen" | "tmux" => {
            Some("Terminal".to_string())
        }
        _ => Some(cleaned),
    }
}

fn terminal_bridge_app_label(metadata: &TerminalBridgeMetadata) -> String {
    metadata
        .terminal
        .as_deref()
        .and_then(normalize_terminal_bridge_label)
        .or_else(|| {
            metadata
                .shell
                .as_deref()
                .and_then(normalize_terminal_bridge_label)
        })
        .unwrap_or_else(|| "Terminal".to_string())
}

fn normalize_terminal_bridge_label(value: &str) -> Option<String> {
    let cleaned = clean_capture_label(value)?;
    let normalized = cleaned.to_ascii_lowercase();
    if normalized == "code" || normalized.contains("visual studio code") {
        return Some("VS Code".to_string());
    }
    if normalized.contains("warp") {
        return Some("Warp".to_string());
    }
    if normalized.contains("iterm") {
        return Some("iTerm".to_string());
    }
    if matches!(
        normalized.as_str(),
        "/bin/zsh"
            | "/bin/bash"
            | "zsh"
            | "bash"
            | "fish"
            | "pwsh"
            | "powershell"
            | "dumb"
            | "unknown"
            | "ansi"
            | "vt100"
            | "xterm"
            | "xterm-256color"
            | "screen"
            | "tmux"
    ) || normalized.contains("terminal")
    {
        return Some("Terminal".to_string());
    }
    Some(cleaned)
}

fn clean_capture_label(value: &str) -> Option<String> {
    let cleaned = clean_report_text(
        value
            .trim()
            .trim_matches(|ch| matches!(ch, '\u{200e}' | '\u{200f}' | '\u{202a}' | '\u{202c}')),
    );
    (!cleaned.is_empty()).then_some(cleaned)
}

fn legacy_app_from_event_id(id: &str) -> Option<String> {
    let raw = id.rsplit('_').next()?;
    let label = split_compact_app_name(raw);
    clean_app_label(&label)
}

fn split_compact_app_name(value: &str) -> String {
    let mut output = String::new();
    let mut previous_was_lower = false;

    for ch in value.chars() {
        if ch == '-' || ch == '_' {
            if !output.ends_with(' ') {
                output.push(' ');
            }
            previous_was_lower = false;
            continue;
        }

        if ch.is_ascii_uppercase() && previous_was_lower && !output.ends_with(' ') {
            output.push(' ');
        }
        previous_was_lower = ch.is_ascii_lowercase();
        output.push(ch);
    }

    clean_report_text(&output)
}

fn is_generic_app_title(value: &str) -> bool {
    clean_app_label(value).is_some_and(|label| {
        matches!(
            label.as_str(),
            "VS Code" | "Google Chrome" | "Brave Browser" | "Terminal" | "System Settings"
        )
    })
}

fn is_self_app_label(value: &str) -> bool {
    clean_app_label(value).is_some_and(|label| label == DISPLAY_APP_NAME)
}

/// Returns true for system apps that indicate the screen is locked, the
/// screensaver is running, or the user is otherwise idle/away. Time spent in
/// these apps must never count as billable work.
fn is_idle_system_app(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "loginwindow"
            | "com.apple.loginwindow"
            | "screensaver"
            | "com.apple.screensaver"
            | "com.apple.notificationcenter"
            | "lockscreen"
            | "com.apple.lockscreen"
            | "systemevents"
            | "com.apple.systemevents"
            | "lockapp"
            | "logonui"
            | "logon ui"
            | "windows logon application"
            | "windows security"
            | "scrnsave"
    )
}

fn looks_like_path(value: &str) -> bool {
    value.starts_with('/') || value.starts_with("~/") || value.contains("\\")
}

fn looks_like_project_path(value: &str) -> bool {
    looks_like_path(value) && !matches!(value, "/bin/zsh" | "/bin/bash" | "/bin/sh")
}

#[derive(Default)]
struct UsageBucket {
    duration_ms: i64,
    events: usize,
    contexts: HashMap<String, i64>,
    examples: Vec<String>,
}

fn build_ai_usage_summary(
    events: &[SourceEvent],
    usage_rows: &[AiUsage],
    output_count: usize,
) -> AiUsageSummary {
    let mut tool_buckets: HashMap<String, UsageBucket> = HashMap::new();
    let mut context_buckets: HashMap<String, UsageBucket> = HashMap::new();

    for event in events {
        let tools = source_event_ai_tools(event);
        if tools.is_empty() {
            continue;
        }
        let duration_ms = event_duration_ms(event);
        let context = source_event_context_label(event);
        let title = event
            .title
            .as_deref()
            .and_then(clean_capture_label)
            .unwrap_or_else(|| context.clone());

        for tool in tools {
            let tool_bucket = tool_buckets.entry(tool.to_string()).or_default();
            tool_bucket.duration_ms += duration_ms;
            tool_bucket.events += 1;
            *tool_bucket.contexts.entry(context.clone()).or_default() += duration_ms;
            push_example(&mut tool_bucket.examples, title.clone());
        }

        let context_bucket = context_buckets.entry(context).or_default();
        context_bucket.duration_ms += duration_ms;
        context_bucket.events += 1;
        push_example(&mut context_bucket.examples, title);
    }

    for usage in usage_rows {
        if is_observed_ai_usage_row(usage) {
            continue;
        }
        let tool = usage
            .tool_name
            .as_deref()
            .or(usage.provider.as_deref())
            .map(clean_report_text)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "AI".to_string());
        let context = usage.context_id.as_deref().and_then(ai_usage_context_label);
        let title = usage
            .thread_title
            .as_deref()
            .or(usage.prompt_summary.as_deref())
            .map(clean_report_text)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "AI usage".to_string());

        let tool_bucket = tool_buckets.entry(tool).or_default();
        let duration_ms = usage.duration_ms.unwrap_or_else(|| {
            usage
                .ended_at
                .unwrap_or(usage.created_at)
                .saturating_sub(usage.started_at.unwrap_or(usage.created_at))
        });
        tool_bucket.duration_ms += duration_ms;
        tool_bucket.events += 1;
        push_example(&mut tool_bucket.examples, title.clone());

        if let Some(context) = context {
            *tool_bucket.contexts.entry(context.clone()).or_default() += duration_ms;
            let context_bucket = context_buckets.entry(context).or_default();
            context_bucket.duration_ms += duration_ms;
            context_bucket.events += 1;
            push_example(&mut context_bucket.examples, title);
        }
    }

    let mut tools = tool_buckets
        .into_iter()
        .map(|(tool, bucket)| {
            let mut contexts = bucket.contexts.into_keys().collect::<Vec<_>>();
            contexts.sort();
            AiToolUsage {
                tool,
                duration_ms: bucket.duration_ms,
                events: bucket.events,
                contexts,
            }
        })
        .collect::<Vec<_>>();
    tools.sort_by_key(|tool| std::cmp::Reverse(tool.duration_ms));

    let mut contexts = context_buckets
        .into_iter()
        .map(|(label, bucket)| AiContextUsage {
            label,
            duration_ms: bucket.duration_ms,
            events: bucket.events,
        })
        .collect::<Vec<_>>();
    contexts.sort_by_key(|context| std::cmp::Reverse(context.duration_ms));

    AiUsageSummary {
        total_duration_ms: tools.iter().map(|tool| tool.duration_ms).sum(),
        tools,
        contexts,
        output_count,
    }
}

fn is_observed_ai_usage_row(usage: &AiUsage) -> bool {
    usage
        .prompt_summary
        .as_deref()
        .unwrap_or_default()
        .is_empty()
        && usage
            .output_summary
            .as_deref()
            .unwrap_or_default()
            .is_empty()
        && usage.duration_ms.unwrap_or_default() > 0
}

/// Known editor and IDE app names whose window titles follow the
/// `"filename — project — AppName"` (or similar) pattern.
const EDITOR_APPS: &[&str] = &[
    "Visual Studio Code",
    "VS Code Insiders",
    "Cursor",
    "Sublime Text",
    "Sublime Text 2",
    "Sublime Text 3",
    "Sublime Text 4",
    "Xcode",
    "IntelliJ IDEA",
    "WebStorm",
    "PyCharm",
    "GoLand",
    "Rider",
    "CLion",
    "RubyMine",
    "DataGrip",
    "Nova",
    "BBEdit",
    "MacVim",
    "Neovim",
    "Zed",
];

const BROWSER_APPS: &[&str] = &[
    "Safari",
    "Firefox",
    "Firefox Developer Edition",
    "Google Chrome",
    "Google Chrome Canary",
    "Microsoft Edge",
    "Brave Browser",
    "ChatGPT Atlas",
    "Arc",
    "Chromium",
    "Opera",
    "Vivaldi",
    "Tor Browser",
    "Waterfox",
];

/// Tab page titles that contain no useful information.
const GENERIC_TAB_TITLES: &[&str] = &[
    "New Tab",
    "New private tab",
    "New Incognito Window",
    "about:blank",
    "about:newtab",
    "New Window",
    "Start Page",
    "Blank Page",
];

fn is_generic_tab_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    GENERIC_TAB_TITLES.iter().any(|t| t.to_lowercase() == lower) || lower.is_empty()
}

/// Parse an editor window title into `(filename, project)`.
///
/// Handles the common patterns:
/// - VS Code / Cursor / Zed : `"filename — project — AppName"`
///   (may have a leading `●` or `•` for modified files)
/// - Sublime Text 4          : `"filename (project) - Sublime Text"`
/// - JetBrains               : `"filename [/path/to/project] – IDE"`
fn parse_editor_file_from_title(title: &str, app_name: &str) -> Option<(String, Option<String>)> {
    if !EDITOR_APPS.contains(&app_name) {
        return None;
    }

    // Strip leading modified-file marker (● U+25CF or • U+2022)
    let title = title
        .trim_start_matches('\u{25CF}')
        .trim_start_matches('\u{2022}')
        .trim();

    // ── VS Code / Cursor / Zed / Xcode: "file — project — App" ──────────
    // Use U+2014 em-dash as primary separator; some systems emit " - ".
    let parts_em: Vec<&str> = title.split(" \u{2014} ").collect();
    if parts_em.len() >= 2 {
        let filename = parts_em[0].trim().to_string();
        if looks_like_filename(&filename) {
            let project = if parts_em.len() >= 3 {
                // parts[1] is the project, parts[2..] is the app name
                let p = parts_em[1].trim().to_string();
                if !EDITOR_APPS.contains(&p.as_str()) {
                    Some(basename_from_path(&p))
                } else {
                    None
                }
            } else {
                None
            };
            return Some((filename, project));
        }
    }

    // ── Sublime Text: "file (project) - Sublime Text" ────────────────────
    if let Some(paren_start) = title.rfind(" (") {
        if let Some(paren_end) = title[paren_start..].find(')') {
            let filename = title[..paren_start].trim().to_string();
            let project_raw = &title[paren_start + 2..paren_start + paren_end];
            if looks_like_filename(&filename) {
                let project = if !project_raw.is_empty() {
                    Some(basename_from_path(project_raw.trim()))
                } else {
                    None
                };
                return Some((filename, project));
            }
        }
    }

    // ── JetBrains: "file [/path] – IDE" ──────────────────────────────────
    let parts_en: Vec<&str> = title.split(" \u{2013} ").collect(); // en-dash
    if parts_en.len() >= 2 {
        let filename = parts_en[0].trim().to_string();
        if looks_like_filename(&filename) {
            // Look for a bracket-enclosed project path
            let project = parts_en[1..].iter().find_map(|part| {
                let part = part.trim();
                if part.starts_with('[') && part.ends_with(']') {
                    Some(basename_from_path(&part[1..part.len() - 1]))
                } else {
                    None
                }
            });
            return Some((filename, project));
        }
    }

    None
}

fn looks_like_filename(s: &str) -> bool {
    if s.is_empty() || s.len() > 200 {
        return false;
    }
    // Must contain a '.' and the extension should be recognisable code/doc extension,
    // OR it must not look like an app name.
    let has_extension = s.contains('.');
    let looks_like_path = s.starts_with('/') || s.contains('\\');
    !looks_like_path && has_extension
}

fn basename_from_path(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

fn build_app_usage_summary(events: &[SourceEvent]) -> AppUsageSummary {
    let mut app_buckets: HashMap<String, Vec<&SourceEvent>> = HashMap::new();
    for event in events {
        let app = event
            .app
            .as_deref()
            .and_then(clean_app_label)
            .unwrap_or_else(|| event.source.clone());
        if is_self_app_label(&app) {
            continue;
        }
        // Exclude lock-screen / screensaver events retroactively (old data).
        if is_idle_system_app(event.app.as_deref().unwrap_or_default()) {
            continue;
        }
        app_buckets.entry(app).or_default().push(event);
    }

    let mut apps = app_buckets
        .into_iter()
        .map(|(app, app_events)| {
            let mut project_buckets: HashMap<String, UsageBucket> = HashMap::new();
            let mut app_ai_events = Vec::new();
            let duration_ms = merge_event_intervals(&app_events);

            // (file_name, context) → (duration_ms, events)
            let mut file_buckets: HashMap<(String, Option<String>), (i64, usize)> = HashMap::new();

            for event in &app_events {
                let project = source_event_context_label(event);
                let bucket = project_buckets.entry(project).or_default();
                let dur = event_duration_ms(event);
                bucket.duration_ms += dur;
                bucket.events += 1;
                *bucket
                    .contexts
                    .entry(source_event_full_context_label(event))
                    .or_default() += dur;
                if let Some(title) = event.title.as_deref().and_then(clean_capture_label) {
                    push_example(&mut bucket.examples, title.clone());

                    // Per-file breakdown: editors get filename+project, browsers get tab title+domain
                    if let Some((filename, proj)) = parse_editor_file_from_title(&title, &app) {
                        let key = (filename, proj);
                        let entry = file_buckets.entry(key).or_default();
                        entry.0 += dur;
                        entry.1 += 1;
                    } else if BROWSER_APPS.contains(&app.as_str()) && !is_generic_tab_title(&title)
                    {
                        let context = event.domain.clone();
                        let key = (title, context);
                        let entry = file_buckets.entry(key).or_default();
                        entry.0 += dur;
                        entry.1 += 1;
                    }
                }
                if source_event_ai_tool(event).is_some() {
                    app_ai_events.push(*event);
                }
            }

            let mut projects = project_buckets
                .into_iter()
                .map(|(label, bucket)| {
                    let project_ai_events = app_events
                        .iter()
                        .copied()
                        .filter(|event| source_event_context_label(event) == label)
                        .filter(|event| source_event_ai_tool(event).is_some())
                        .collect::<Vec<_>>();
                    let mut contexts = bucket.contexts.into_keys().collect::<Vec<_>>();
                    contexts.sort();
                    AppProjectUsage {
                        label,
                        contexts,
                        duration_ms: bucket.duration_ms,
                        events: bucket.events,
                        ai_tools: ai_tools_from_events(&project_ai_events),
                        examples: bucket.examples,
                    }
                })
                .collect::<Vec<_>>();
            projects.sort_by_key(|project| std::cmp::Reverse(project.duration_ms));

            let mut files: Vec<FileUsage> = file_buckets
                .into_iter()
                .map(|((name, context), (dur, evts))| FileUsage {
                    name,
                    context,
                    duration_ms: dur,
                    events: evts,
                })
                .collect();
            files.sort_by_key(|f| std::cmp::Reverse(f.duration_ms));
            // Keep the top 20 most-used files to avoid bloating the payload
            files.truncate(20);

            AppUsage {
                category: classify_app_category(&app).to_string(),
                app,
                duration_ms,
                events: app_events.len(),
                projects,
                ai_tools: ai_tools_from_events(&app_ai_events),
                files,
            }
        })
        .collect::<Vec<_>>();
    apps.sort_by_key(|app| std::cmp::Reverse(app.duration_ms));

    // Total = union of all source_events intervals (not sum of per-app totals,
    // which would double-count events if an app appears in two separate windows).
    let all_events: Vec<&SourceEvent> = events
        .iter()
        .filter(|e| {
            e.app.as_deref()
                .and_then(clean_app_label)
                .map(|a| !is_self_app_label(&a) && !is_idle_system_app(&a))
                .unwrap_or(false)
        })
        .collect();
    AppUsageSummary {
        total_duration_ms: merge_event_intervals(&all_events),
        apps,
    }
}

fn configured_recovery_threshold_ms(settings: &Settings) -> i64 {
    settings.recovery_threshold_minutes.clamp(15, 120) * 60_000
}

fn build_recovery_summary(
    source_events: &[SourceEvent],
    recovery_events: &[RecoveryEvent],
    from_ms: Option<i64>,
    to_ms: Option<i64>,
    threshold_ms: i64,
) -> RecoverySummary {
    let threshold_ms = threshold_ms.clamp(15 * 60_000, 120 * 60_000);
    let mut work_events = source_events
        .iter()
        .filter(|event| event.duration_ms > 0)
        .filter(|event| !is_idle_system_app(event.app.as_deref().unwrap_or_default()))
        .collect::<Vec<_>>();
    work_events.sort_by_key(|event| (event.started_at, event.ended_at));

    let mut taken_breaks = recovery_events
        .iter()
        .filter(|event| event.event_type == "taken")
        .filter_map(|event| event.ended_at.map(|end| (event.started_at, end)))
        .collect::<Vec<_>>();
    taken_breaks.sort_by_key(|(started_at, _)| *started_at);

    let mut total_screen_ms = 0_i64;
    let mut longest_uninterrupted_ms = 0_i64;
    let mut current_streak_ms = 0_i64;
    let mut streak_start: Option<i64> = None;
    let mut last_end: Option<i64> = None;

    for event in work_events {
        let event_start = from_ms.map_or(event.started_at, |from| event.started_at.max(from));
        let event_end = to_ms.map_or(event.ended_at, |to| event.ended_at.min(to));
        if event_end <= event_start {
            continue;
        }
        for (started_at, ended_at) in recovery_work_segments(event_start, event_end, &taken_breaks)
        {
            if ended_at <= started_at {
                continue;
            }
            total_screen_ms += ended_at - started_at;

            let reset_for_gap = last_end
                .map(|end| started_at.saturating_sub(end) >= RECOVERY_STREAK_RESET_GAP_MS)
                .unwrap_or(true);
            let reset_for_recovery = last_end
                .map(|end| {
                    taken_breaks.iter().any(|(break_start, break_end)| {
                        *break_start >= end && *break_end <= started_at
                    })
                })
                .unwrap_or(false);

            if reset_for_gap || reset_for_recovery {
                streak_start = Some(started_at);
            }

            let start = streak_start.unwrap_or(started_at);
            current_streak_ms = ended_at.saturating_sub(start);
            longest_uninterrupted_ms = longest_uninterrupted_ms.max(current_streak_ms);
            last_end = Some(ended_at);
        }
    }

    let taken_count = recovery_events
        .iter()
        .filter(|event| event.event_type == "taken")
        .count();
    let skipped_count = recovery_events
        .iter()
        .filter(|event| event.event_type == "skipped")
        .count();
    let snoozed_count = recovery_events
        .iter()
        .filter(|event| event.event_type == "snoozed")
        .count();
    let prompted_count = recovery_events
        .iter()
        .filter(|event| event.event_type == "prompted" || event.event_type.ends_with("_prompted"))
        .count();

    let overrun_ms = longest_uninterrupted_ms.saturating_sub(threshold_ms);
    let long_run_penalty = ((overrun_ms / (15 * 60_000)) * 5).min(40);
    let skipped_penalty = (skipped_count as i64 * 10).min(30);
    let snooze_penalty = (snoozed_count as i64 * 3).min(15);
    let break_credit = (taken_count as i64 * 8).min(20);
    let score =
        (100 - long_run_penalty - skipped_penalty - snooze_penalty + break_credit).clamp(0, 100);

    let prompt_due = current_streak_ms >= threshold_ms;
    let next_prompt = (total_screen_ms > 0).then(|| RecoveryPrompt {
        action: if prompt_due { "due" } else { "ready" }.to_string(),
        reason: if prompt_due {
            "Long uninterrupted screen run".to_string()
        } else {
            "Smart Breaks are ready when sustained input continues".to_string()
        },
        streak_ms: current_streak_ms,
        suggested_minutes: RECOVERY_SUGGESTED_BREAK_MINUTES,
    });

    let mut recent_events = recovery_events.to_vec();
    recent_events.sort_by_key(|event| std::cmp::Reverse(event.started_at));
    recent_events.truncate(8);

    RecoverySummary {
        score,
        total_screen_ms,
        longest_uninterrupted_ms,
        current_streak_ms,
        taken_count,
        skipped_count,
        snoozed_count,
        prompted_count,
        next_prompt,
        recent_events,
    }
}

fn recovery_work_segments(
    event_start: i64,
    event_end: i64,
    taken_breaks: &[(i64, i64)],
) -> Vec<(i64, i64)> {
    let mut segments = Vec::new();
    let mut cursor = event_start;
    for (break_start, break_end) in taken_breaks {
        let overlap_start = (*break_start).max(event_start);
        let overlap_end = (*break_end).min(event_end);
        if overlap_end <= overlap_start {
            continue;
        }
        if overlap_start > cursor {
            segments.push((cursor, overlap_start));
        }
        cursor = cursor.max(overlap_end);
        if cursor >= event_end {
            break;
        }
    }
    if cursor < event_end {
        segments.push((cursor, event_end));
    }
    segments
}

fn build_automation_candidates(events: &[SourceEvent]) -> Vec<AutomationCandidate> {
    let mut buckets: HashMap<String, UsageBucket> = HashMap::new();
    let mut ai_tools_by_context: HashMap<String, HashSet<String>> = HashMap::new();
    for event in events {
        let key = source_event_context_label(event);
        if key == "Captured activity" || key == DISPLAY_APP_NAME {
            continue;
        }
        let bucket = buckets.entry(key.clone()).or_default();
        bucket.duration_ms += event_duration_ms(event);
        bucket.events += 1;
        if let Some(app) = event.app.as_deref().and_then(clean_app_label) {
            push_example(&mut bucket.examples, app);
        }
        if let Some(tool) = source_event_ai_tool(event) {
            ai_tools_by_context
                .entry(key)
                .or_default()
                .insert(tool.to_string());
        }
    }

    let mut candidates = buckets
        .into_iter()
        .filter(|(_, bucket)| bucket.events >= 3)
        .map(|(title, bucket)| {
            let mut related_ai_tools = ai_tools_by_context
                .remove(&title)
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();
            related_ai_tools.sort();
            AutomationCandidate {
                id: format!("automation-{}", stable_id_part(&title)),
                title,
                signal: "Repeated app/project pattern".to_string(),
                reason: "This context appeared repeatedly today; review whether reporting, lookup, copy/paste, or follow-up steps can be automated.".to_string(),
                occurrences: bucket.events,
                duration_ms: bucket.duration_ms,
                suggested_steps: automation_suggested_steps(&bucket.examples, related_ai_tools.as_slice()),
                example_sources: bucket.examples,
                related_ai_tools,
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| {
        (
            std::cmp::Reverse(candidate.occurrences),
            std::cmp::Reverse(candidate.duration_ms),
        )
    });
    candidates.truncate(8);
    candidates
}

#[derive(Debug, Clone)]
struct InferenceEvidence<'a> {
    event: &'a SourceEvent,
    key: String,
    clean_title: String,
}

fn build_inferred_work_blocks(
    events: &[SourceEvent],
    calendar_events: &[CalendarEvent],
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Vec<InferredWorkBlock> {
    let mut evidence = events
        .iter()
        .filter(|event| event_duration_ms(event) > 0)
        .filter(|event| from_ms.is_none_or(|from| event.ended_at > from))
        .filter(|event| to_ms.is_none_or(|to| event.started_at < to))
        .filter_map(|event| {
            presentation_inference_key(event).map(|(key, clean_title)| InferenceEvidence {
                event,
                key,
                clean_title,
            })
        })
        .collect::<Vec<_>>();

    evidence.sort_by_key(|item| (item.key.clone(), item.event.started_at));

    let mut blocks = Vec::new();
    let mut current: Vec<InferenceEvidence<'_>> = Vec::new();
    for item in evidence {
        let split = current.last().is_some_and(|previous| {
            previous.key != item.key
                || item
                    .event
                    .started_at
                    .saturating_sub(previous.event.ended_at)
                    > 15 * 60_000
        });
        if split {
            if let Some(block) = build_presentation_inferred_block(&current, calendar_events) {
                blocks.push(block);
            }
            current.clear();
        }
        current.push(item);
    }
    if let Some(block) = build_presentation_inferred_block(&current, calendar_events) {
        blocks.push(block);
    }

    blocks.sort_by_key(|block| (block.started_at, std::cmp::Reverse(block.duration_ms)));
    blocks.truncate(12);
    blocks
}

fn build_presentation_inferred_block(
    evidence: &[InferenceEvidence<'_>],
    calendar_events: &[CalendarEvent],
) -> Option<InferredWorkBlock> {
    if evidence.is_empty() {
        return None;
    }

    let started_at = evidence
        .iter()
        .map(|item| item.event.started_at)
        .min()
        .unwrap_or_default();
    let ended_at = evidence
        .iter()
        .map(|item| item.event.ended_at)
        .max()
        .unwrap_or(started_at);
    let span_ms = ended_at.saturating_sub(started_at);
    let active_ms = merged_interval_duration_ms(
        evidence
            .iter()
            .map(|item| (item.event.started_at, item.event.ended_at))
            .collect(),
    );
    if active_ms < 20 * 60_000 || span_ms < 30 * 60_000 {
        return None;
    }

    let title = evidence
        .iter()
        .max_by_key(|item| event_duration_ms(item.event))
        .map(|item| item.clean_title.clone())
        .unwrap_or_else(|| "presentation".to_string());
    let primary_app = dominant_label(
        evidence
            .iter()
            .filter_map(|item| {
                item.event
                    .app
                    .as_deref()
                    .and_then(clean_app_label)
                    .map(|label| (label, event_duration_ms(item.event)))
            }),
    )
    .unwrap_or_else(|| "Captured activity".to_string());
    let primary_context = dominant_label(
        evidence
            .iter()
            .map(|item| (source_event_context_label(item.event), event_duration_ms(item.event))),
    )
    .unwrap_or_else(|| source_event_context_label(evidence[0].event));
    let calendar_support = calendar_events
        .iter()
        .filter(|event| !calendar_event_cancelled(event))
        .any(|event| {
            event.starts_at < ended_at
                && event.ends_at > started_at
                && calendar_event_looks_like_meeting(event)
        });
    let mut confidence_percent = 60;
    if evidence.iter().any(|item| source_mentions_google_slides(item.event)) {
        confidence_percent += 15;
    }
    if active_ms >= 45 * 60_000 || span_ms >= 60 * 60_000 {
        confidence_percent += 10;
    }
    if calendar_support {
        confidence_percent += 10;
    }
    if evidence.len() >= 3 {
        confidence_percent += 5;
    }
    confidence_percent = confidence_percent.min(92);
    if confidence_percent < 60 {
        return None;
    }

    let confidence = if confidence_percent >= 75 {
        "high"
    } else {
        "medium"
    };
    let evidence_ids = evidence
        .iter()
        .map(|item| item.event.id.clone())
        .collect::<Vec<_>>();
    let detail = format!(
        "{} of sustained presentation activity from {} to {}. Confirm whether this was a meeting, demo, review, or preparation.",
        format_duration_words(active_ms),
        format_clock_time(started_at),
        format_clock_time(ended_at)
    );
    let reason = if calendar_support {
        "Google Slides stayed active for a sustained block and overlaps calendar meeting time."
            .to_string()
    } else {
        "Google Slides stayed active for a sustained block; DayTrail needs confirmation before marking it as meeting time."
            .to_string()
    };

    Some(InferredWorkBlock {
        id: format!(
            "inferred-presentation-{}-{}",
            started_at,
            stable_id_part(&title)
        ),
        category: "presentation_meeting".to_string(),
        title: format!("Possible meeting or presentation: {title}"),
        detail,
        confidence: confidence.to_string(),
        confidence_percent,
        started_at,
        ended_at,
        duration_ms: span_ms.max(active_ms),
        primary_app,
        primary_context,
        reason,
        evidence_ids,
        suggested_actions: vec![
            "Confirm as meeting".to_string(),
            "Mark as presentation prep".to_string(),
            "Ignore suggestion".to_string(),
        ],
    })
}

fn presentation_inference_key(event: &SourceEvent) -> Option<(String, String)> {
    if !source_looks_like_presentation(event) {
        return None;
    }
    let clean_title = event
        .title
        .as_deref()
        .and_then(clean_capture_label)
        .map(clean_presentation_title)
        .filter(|title| title.len() >= 3)
        .or_else(|| event.workspace_key.as_deref().and_then(clean_capture_label))
        .or_else(|| event.domain.as_deref().and_then(clean_capture_label))
        .unwrap_or_else(|| "presentation".to_string());
    let key = stable_id_part(&clean_title);
    if key.is_empty() {
        None
    } else {
        Some((key, clean_title))
    }
}

fn source_looks_like_presentation(event: &SourceEvent) -> bool {
    let combined = [
        event.title.as_deref(),
        event.domain.as_deref(),
        event.url_redacted.as_deref(),
        event.workspace_key.as_deref(),
        event.app.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.to_ascii_lowercase())
    .collect::<Vec<_>>()
    .join(" ");

    combined.contains("google slides")
        || combined.contains("docs.google.com/presentation")
        || combined.contains("presentation/d/")
        || combined.contains("powerpoint")
        || combined.contains("keynote")
        || combined.contains("slide deck")
}

fn source_mentions_google_slides(event: &SourceEvent) -> bool {
    let combined = [
        event.title.as_deref(),
        event.domain.as_deref(),
        event.url_redacted.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.to_ascii_lowercase())
    .collect::<Vec<_>>()
    .join(" ");
    combined.contains("google slides")
        || combined.contains("docs.google.com/presentation")
        || combined.contains("presentation/d/")
}

fn clean_presentation_title(value: String) -> String {
    let mut title = value
        .replace(" - Google Slides", "")
        .replace(" - PowerPoint", "")
        .replace(" - Keynote", "")
        .replace(" | Google Slides", "")
        .replace(" · Google Slides", "");
    title = clean_report_text(&title);
    if title.is_empty() {
        value
    } else {
        title
    }
}

fn calendar_event_looks_like_meeting(event: &CalendarEvent) -> bool {
    let text = [
        Some(event.title.as_str()),
        event.location.as_deref(),
        event.planned_work_type.as_deref(),
        event.calendar_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.to_ascii_lowercase())
    .collect::<Vec<_>>()
    .join(" ");
    [
        "meeting",
        "call",
        "demo",
        "presentation",
        "review",
        "sync",
        "standup",
        "discussion",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn dominant_label<I>(items: I) -> Option<String>
where
    I: IntoIterator<Item = (String, i64)>,
{
    let mut buckets: HashMap<String, i64> = HashMap::new();
    for (label, duration_ms) in items {
        if let Some(cleaned) = clean_capture_label(&label) {
            *buckets.entry(cleaned).or_default() += duration_ms.max(0);
        }
    }
    buckets
        .into_iter()
        .max_by_key(|(_, duration_ms)| *duration_ms)
        .map(|(label, _)| label)
}

fn format_clock_time(value: i64) -> String {
    match Local.timestamp_millis_opt(value).single() {
        Some(time) => time.format("%-I:%M %p").to_string(),
        None => "unknown time".to_string(),
    }
}

fn automation_suggested_steps(examples: &[String], ai_tools: &[String]) -> Vec<String> {
    let has_browser = examples
        .iter()
        .any(|example| example.to_ascii_lowercase().contains("chrome"));
    let has_editor = examples.iter().any(|example| {
        let lower = example.to_ascii_lowercase();
        lower.contains("code") || lower.contains("cursor") || lower.contains("webstorm")
    });
    let has_terminal = examples.iter().any(|example| {
        let lower = example.to_ascii_lowercase();
        lower.contains("terminal") || lower.contains("warp") || lower.contains("iterm")
    });

    let mut steps = Vec::new();
    if has_browser {
        steps.push("Identify the repeated browser lookup or reporting step.".to_string());
    }
    if has_editor || has_terminal {
        steps.push(
            "Check whether the repeated project commands can become a script or task runner."
                .to_string(),
        );
    }
    if !ai_tools.is_empty() {
        steps.push(format!(
            "Convert the repeated {} prompt into a saved template.",
            ai_tools.join("/")
        ));
    }
    steps.push("Review the captured examples before creating an automation.".to_string());
    steps.truncate(3);
    steps
}

fn build_capture_health_with_permission_state(
    events: &[SourceEvent],
    settings: &Settings,
    pause_state: &PauseState,
    required_permissions_granted: bool,
    liveness: crate::active_window::CaptureLiveness,
    heartbeat: Option<&crate::active_window::WatcherHeartbeat>,
) -> CaptureHealthSummary {
    use crate::active_window::CaptureLiveness;

    // The watcher heartbeat distinguishes "quiet" from "broken". When the poll
    // thread has stalled (App Nap / hang / crash) or lost Accessibility, this is
    // the single most important signal — surfaced first and escalated to an error.
    let last_tick_at = heartbeat.map(|hb| hb.last_tick_at_ms);
    let watcher_check = match liveness {
        CaptureLiveness::Stalled => CaptureHealthCheck {
            id: "capture-watcher".to_string(),
            label: "Capture engine".to_string(),
            status: "error".to_string(),
            detail: "The capture watcher stopped responding — recent activity is not being recorded.".to_string(),
            last_seen_at: last_tick_at,
            evidence_count: 0,
            action: Some("Quit and reopen DayTrail to restart capture.".to_string()),
        },
        CaptureLiveness::PermissionLost => CaptureHealthCheck {
            id: "capture-watcher".to_string(),
            label: "Capture engine".to_string(),
            status: "error".to_string(),
            detail: "Accessibility permission was revoked — apps and window titles can no longer be captured.".to_string(),
            last_seen_at: last_tick_at,
            evidence_count: 0,
            action: Some("Re-grant Accessibility for DayTrail in Privacy & Security.".to_string()),
        },
        CaptureLiveness::Healthy => CaptureHealthCheck {
            id: "capture-watcher".to_string(),
            label: "Capture engine".to_string(),
            status: "ok".to_string(),
            detail: "Capture engine is running.".to_string(),
            last_seen_at: last_tick_at,
            evidence_count: 1,
            action: None,
        },
        CaptureLiveness::Unknown => CaptureHealthCheck {
            id: "capture-watcher".to_string(),
            label: "Capture engine".to_string(),
            status: "waiting".to_string(),
            detail: "Capture engine is starting up.".to_string(),
            last_seen_at: last_tick_at,
            evidence_count: 0,
            action: None,
        },
    };

    let mut checks = vec![
        watcher_check,
        CaptureHealthCheck {
            id: "os-permissions".to_string(),
            label: "OS permissions".to_string(),
            status: if required_permissions_granted {
                "ok".to_string()
            } else {
                "needs_setup".to_string()
            },
            detail: if required_permissions_granted {
                "Required desktop capture permissions are granted.".to_string()
            } else {
                "Grant Accessibility for the installed DayTrail app to make app and window-title capture reliable.".to_string()
            },
            last_seen_at: Some(now_ms()),
            evidence_count: usize::from(required_permissions_granted),
            action: (!required_permissions_granted)
                .then_some("Open Privacy & Security > Accessibility.".to_string()),
        },
    ];

    checks.extend(vec![
        capture_health_check(
            "active-window",
            "Apps",
            events,
            |event| event.source == "active-window" || event.event_type == "active_window",
            None,
        ),
        capture_health_check(
            "browser-bridge",
            "Browser tabs",
            events,
            |event| {
                event.source.contains("browser")
                    || event.event_type.contains("browser")
                    || event.domain.is_some()
            },
            (!settings.browser_bridge_enabled)
                .then_some("Enable browser bridge in Settings.".to_string()),
        ),
        capture_health_check(
            "editor-bridge",
            "Editor projects",
            events,
            |event| {
                let haystack = [
                    event.source.as_str(),
                    event.event_type.as_str(),
                    event.app.as_deref().unwrap_or_default(),
                ]
                .join(" ")
                .to_ascii_lowercase();
                haystack.contains("editor")
                    || haystack.contains("vscode")
                    || haystack.contains("code")
                    || haystack.contains("cursor")
                    || haystack.contains("jetbrains")
                    || haystack.contains("webstorm")
                    || haystack.contains("intellij")
                    || haystack.contains("netbeans")
            },
            None,
        ),
        capture_health_check(
            "terminal-bridge",
            "Terminal folders",
            events,
            |event| {
                let haystack = [
                    event.source.as_str(),
                    event.event_type.as_str(),
                    event.app.as_deref().unwrap_or_default(),
                ]
                .join(" ")
                .to_ascii_lowercase();
                haystack.contains("terminal")
                    || haystack.contains("warp")
                    || haystack.contains("iterm")
                    || haystack.contains("zsh")
            },
            settings
                .terminal_bridge_path
                .is_none()
                .then_some("Install shell integration to capture cwd and commands.".to_string()),
        ),
        capture_health_check(
            "ai-tools",
            "AI tools",
            events,
            |event| source_event_ai_tool(event).is_some(),
            None,
        ),
        CaptureHealthCheck {
            id: "privacy".to_string(),
            label: "Privacy policy".to_string(),
            status: "ok".to_string(),
            detail: if settings.full_clipboard_history {
                "Full clipboard storage is enabled by user choice.".to_string()
            } else {
                "Metadata-first capture; clipboard content not stored.".to_string()
            },
            last_seen_at: Some(now_ms()),
            evidence_count: settings.excluded_apps.len()
                + settings.excluded_domains.len()
                + settings.excluded_projects.len(),
            action: None,
        },
    ]);

    let ok_count = checks
        .iter()
        .filter(|check| check.id != "privacy" && check.status == "ok")
        .count();
    let needs_setup = checks
        .iter()
        .filter(|check| check.id != "privacy" && check.status == "needs_setup")
        .count();
    let ai_waiting = checks
        .iter()
        .any(|check| check.id == "ai-tools" && check.status != "ok");
    // A broken capture engine outranks everything except an intentional pause:
    // the user must know capture is down, not see a reassuring "warming up".
    let status = if pause_state.paused {
        "paused"
    } else if matches!(
        liveness,
        CaptureLiveness::Stalled | CaptureLiveness::PermissionLost
    ) {
        "error"
    } else if needs_setup > 0 {
        "needs_setup"
    } else if ok_count >= 3 && !ai_waiting {
        "healthy"
    } else {
        "warming_up"
    };
    let headline = match (status, liveness) {
        ("paused", _) => "Capture is paused".to_string(),
        ("error", CaptureLiveness::PermissionLost) => {
            "Capture degraded — Accessibility permission was revoked".to_string()
        }
        ("error", _) => "Capture stopped — the capture engine isn't responding".to_string(),
        ("healthy", _) => "Core capture is receiving signals".to_string(),
        ("needs_setup", _) => format!("{needs_setup} capture source(s) need setup"),
        _ => "Capture is waiting for more signals".to_string(),
    };

    CaptureHealthSummary {
        status: status.to_string(),
        headline,
        updated_at: now_ms(),
        checks,
    }
}

fn capture_health_check<F>(
    id: &str,
    label: &str,
    events: &[SourceEvent],
    matches: F,
    forced_action: Option<String>,
) -> CaptureHealthCheck
where
    F: Fn(&SourceEvent) -> bool,
{
    let matched = events
        .iter()
        .filter(|event| matches(event))
        .collect::<Vec<_>>();
    let last_seen_at = matched.iter().map(|event| event.ended_at).max();
    let status = if matched.is_empty() && forced_action.is_some() {
        "needs_setup"
    } else if matched.is_empty() {
        "waiting"
    } else {
        "ok"
    };
    let detail = match status {
        "ok" => {
            let label = matched
                .iter()
                .max_by_key(|event| event.ended_at)
                .and_then(|event| {
                    event
                        .title
                        .as_deref()
                        .and_then(clean_capture_label)
                        .or_else(|| event.workspace_key.as_deref().and_then(clean_capture_label))
                        .or_else(|| event.domain.as_deref().and_then(clean_capture_label))
                        .or_else(|| event.app.as_deref().and_then(clean_app_label))
                })
                .unwrap_or_else(|| "recent signal".to_string());
            format!("Last signal: {label}")
        }
        "needs_setup" => forced_action
            .clone()
            .unwrap_or_else(|| "Configure this source to improve capture.".to_string()),
        _ => "No signal captured today yet.".to_string(),
    };

    CaptureHealthCheck {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        detail,
        last_seen_at,
        evidence_count: matched.len(),
        action: forced_action,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_unclosed_loop_inbox(
    tasks: &[Task],
    pending_replies: &[EmailThread],
    commitments: &[Commitment],
    ai_outputs: &[WorkOutput],
    meetings: &[Meeting],
    field_visits: &[FieldVisit],
    idle_blocks: &[IdleBlock],
    loop_risks: &[LoopRisk],
    hidden_loop_ids: &HashSet<String>,
) -> Vec<UnclosedLoopItem> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    for task in tasks.iter().filter(|task| task.status == TaskStatus::Open).take(8) {
        let id = format!("task-{}", task.id);
        if hidden_loop_ids.contains(&id) {
            continue;
        }
        push_unclosed_loop(
            &mut items,
            &mut seen,
            UnclosedLoopItem {
                id,
                category: "Task".to_string(),
                title: task.title.clone(),
                detail: task
                    .notes
                    .clone()
                    .or_else(|| task.project_label.clone())
                    .or_else(|| task.source.clone())
                    .unwrap_or_else(|| task_due_reason(task)),
                source: "Tasks & Reminders".to_string(),
                risk: match task.priority.as_deref() {
                    Some("high") => "high".to_string(),
                    Some("low") => "low".to_string(),
                    _ => "medium".to_string(),
                },
                status: task.status.as_db_value().to_string(),
                primary_action: "Complete, snooze, or delete".to_string(),
                evidence_ids: Vec::new(),
            },
        );
    }

    for reply in pending_replies.iter().take(8) {
        let id = format!("reply-{}", reply.id);
        if hidden_loop_ids.contains(&id) {
            continue;
        }
        push_unclosed_loop(
            &mut items,
            &mut seen,
            UnclosedLoopItem {
                id,
                category: "Reply".to_string(),
                title: format!("Reply to {}", reply.subject),
                detail: reply
                    .latest_sender
                    .clone()
                    .unwrap_or_else(|| "Latest message needs attention".to_string()),
                source: "Source-marked reply".to_string(),
                risk: "high".to_string(),
                status: "open".to_string(),
                primary_action: "Send reply or dismiss".to_string(),
                evidence_ids: extract_evidence_ids(reply.evidence_json.as_deref()),
            },
        );
    }

    for commitment in commitments.iter().take(8) {
        let id = format!("promise-{}", commitment.id);
        if hidden_loop_ids.contains(&id) {
            continue;
        }
        push_unclosed_loop(
            &mut items,
            &mut seen,
            UnclosedLoopItem {
                id,
                category: "Promise".to_string(),
                title: commitment.title.clone(),
                detail: commitment
                    .source
                    .clone()
                    .unwrap_or_else(|| "Open commitment".to_string()),
                source: "Saved commitment".to_string(),
                risk: if commitment.due_at.is_some() {
                    "high".to_string()
                } else {
                    "medium".to_string()
                },
                status: commitment.status.clone(),
                primary_action: "Close, reschedule, or add evidence".to_string(),
                evidence_ids: extract_evidence_ids(commitment.evidence_json.as_deref()),
            },
        );
    }

    for output in ai_outputs
        .iter()
        .filter(|output| matches!(output.status.as_str(), "drafted" | "needs_review"))
        .take(8)
    {
        let id = format!("ai-output-{}", output.id);
        if hidden_loop_ids.contains(&id) {
            continue;
        }
        push_unclosed_loop(
            &mut items,
            &mut seen,
            UnclosedLoopItem {
                id,
                category: "AI Output".to_string(),
                title: output.title.clone(),
                detail: format!("{} - {}", output.output_type, output.status),
                source: output
                    .source
                    .clone()
                    .unwrap_or_else(|| "AI output ledger".to_string()),
                risk: "medium".to_string(),
                status: output.status.clone(),
                primary_action: "Use, send, or discard".to_string(),
                evidence_ids: extract_evidence_ids(output.evidence_json.as_deref()),
            },
        );
    }

    for meeting in meetings
        .iter()
        .filter(|meeting| {
            meeting
                .actions_json
                .as_deref()
                .is_some_and(|value| value != "[]")
        })
        .take(6)
    {
        let id = format!("meeting-{}", meeting.id);
        if hidden_loop_ids.contains(&id) {
            continue;
        }
        push_unclosed_loop(
            &mut items,
            &mut seen,
            UnclosedLoopItem {
                id,
                category: "Meeting".to_string(),
                title: meeting.title.clone(),
                detail: meeting
                    .summary
                    .clone()
                    .unwrap_or_else(|| "Meeting has captured action items".to_string()),
                source: "Meeting actions".to_string(),
                risk: "medium".to_string(),
                status: "open".to_string(),
                primary_action: "Confirm notes and actions".to_string(),
                evidence_ids: Vec::new(),
            },
        );
    }

    for visit in field_visits
        .iter()
        .filter(|visit| visit.status != "completed" || visit.debrief.is_none())
        .take(6)
    {
        let id = format!("field-visit-{}", visit.id);
        if hidden_loop_ids.contains(&id) {
            continue;
        }
        push_unclosed_loop(
            &mut items,
            &mut seen,
            UnclosedLoopItem {
                id,
                category: "Field Visit".to_string(),
                title: visit
                    .client_label
                    .clone()
                    .or_else(|| visit.location_label.clone())
                    .unwrap_or_else(|| "Client visit".to_string()),
                detail: "Visit needs debrief or follow-up".to_string(),
                source: "Field visit recovery".to_string(),
                risk: "medium".to_string(),
                status: visit.status.clone(),
                primary_action: "Add debrief and follow-up".to_string(),
                evidence_ids: Vec::new(),
            },
        );
    }

    for idle in idle_blocks
        .iter()
        .filter(|idle| !idle.classified && is_actionable_idle_block(idle))
        .take(6)
    {
        let id = format!("idle-{}", idle.id);
        if hidden_loop_ids.contains(&id) {
            continue;
        }
        push_unclosed_loop(
            &mut items,
            &mut seen,
            UnclosedLoopItem {
                id,
                category: "Away Time".to_string(),
                title: format!("Classify {} away", format_duration_words(idle.duration_ms)),
                detail: "Laptop was idle but this may have been real work.".to_string(),
                source: "Smart idle recovery".to_string(),
                risk: "low".to_string(),
                status: "open".to_string(),
                primary_action: "Record break, meeting, call, travel, or ignore".to_string(),
                evidence_ids: extract_evidence_ids(idle.evidence_json.as_deref()),
            },
        );
    }

    for risk in loop_risks.iter().take(12) {
        let id = format!("risk-{}-{}", risk.risk_type, risk.id);
        if hidden_loop_ids.contains(&id) {
            continue;
        }
        push_unclosed_loop(
            &mut items,
            &mut seen,
            UnclosedLoopItem {
                id,
                category: loop_risk_category(&risk.risk_type).to_string(),
                title: risk.title.clone(),
                detail: risk.reason.clone(),
                source: risk.source.clone(),
                risk: if risk.priority >= 90 {
                    "high".to_string()
                } else if risk.priority >= 80 {
                    "medium".to_string()
                } else {
                    "low".to_string()
                },
                status: "open".to_string(),
                primary_action: "Review and close loop".to_string(),
                evidence_ids: extract_evidence_ids(risk.evidence_json.as_deref()),
            },
        );
    }

    items.sort_by_key(|item| {
        let risk_rank = match item.risk.as_str() {
            "high" => 0,
            "medium" => 1,
            _ => 2,
        };
        (risk_rank, item.title.clone())
    });
    items.truncate(20);
    items
}

fn push_unclosed_loop(
    items: &mut Vec<UnclosedLoopItem>,
    seen: &mut HashSet<String>,
    item: UnclosedLoopItem,
) {
    let key = format!("{}:{}", item.category, item.title).to_ascii_lowercase();
    if seen.insert(key) {
        items.push(item);
    }
}

/// Compact snapshot of today's activity for passing as chat context to the LLM.
/// Intentionally terse — keeps tokens low while giving the LLM all key facts.
fn build_compact_chat_snapshot(snapshot: &TodaySnapshot) -> String {
    let total_session_ms: i64 = snapshot
        .work_sessions
        .iter()
        .map(|s| s.duration_ms.max(0))
        .sum();
    let total_ms = if total_session_ms > 0 {
        total_session_ms
    } else {
        snapshot.app_usage_summary.total_duration_ms.max(0)
    };

    let mut out = format!("## Today — {}\n\n", snapshot.local_date);
    out.push_str(&format!(
        "**Time tracked:** {}  |  **Sessions:** {}\n",
        format_duration_words(total_ms),
        snapshot.work_sessions.len()
    ));

    if !snapshot.work_sessions.is_empty() {
        out.push_str("\n**Work sessions:**\n");
        for s in snapshot.work_sessions.iter().take(8) {
            let label = s
                .project_label
                .as_deref()
                .or(s.client_label.as_deref())
                .map(|p| format!(" [{}]", p))
                .unwrap_or_default();
            out.push_str(&format!(
                "- {}{} — {}{}",
                s.title,
                label,
                format_duration_words(s.duration_ms),
                if s.billable { " (billable)" } else { "" }
            ));
            out.push('\n');
        }
    }

    if !snapshot.app_usage_summary.apps.is_empty() {
        out.push_str("\n**App usage:**\n");
        for app in snapshot.app_usage_summary.apps.iter().take(8) {
            let pct = if total_ms > 0 {
                app.duration_ms * 100 / total_ms
            } else {
                0
            };
            out.push_str(&format!(
                "- {} [{}]: {} ({}%)\n",
                app.app,
                app.category,
                format_duration_words(app.duration_ms),
                pct
            ));
        }
    }

    let open_tasks: Vec<_> = snapshot
        .tasks
        .iter()
        .filter(|t| t.status == crate::models::TaskStatus::Open)
        .collect();
    let now = now_ms();
    let overdue_count = open_tasks
        .iter()
        .filter(|t| t.due_at.is_some_and(|d| d < now))
        .count();
    out.push_str(&format!(
        "\n**Tasks:** {} open",
        open_tasks.len()
    ));
    if overdue_count > 0 {
        out.push_str(&format!(", {} overdue", overdue_count));
    }
    out.push('\n');

    if !open_tasks.is_empty() {
        for t in open_tasks.iter().take(5) {
            let overdue = t.due_at.is_some_and(|d| d < now);
            out.push_str(&format!(
                "  - {}{}\n",
                t.title,
                if overdue { " (OVERDUE)" } else { "" }
            ));
        }
        if open_tasks.len() > 5 {
            out.push_str(&format!("  - … and {} more\n", open_tasks.len() - 5));
        }
    }

    if !snapshot.loop_risks.is_empty() {
        out.push_str(&format!(
            "\n**Open loops:** {} items need attention\n",
            snapshot.loop_risks.len()
        ));
    }

    if !snapshot.commitments.is_empty() {
        let overdue_c = snapshot
            .commitments
            .iter()
            .filter(|c| c.due_at.is_some_and(|d| d < now))
            .count();
        out.push_str(&format!(
            "\n**Commitments:** {} open{}",
            snapshot.commitments.len(),
            if overdue_c > 0 {
                format!(", {} overdue", overdue_c)
            } else {
                String::new()
            }
        ));
        out.push('\n');
    }

    if !snapshot.ai_usage_summary.tools.is_empty() {
        let tools: Vec<String> = snapshot
            .ai_usage_summary
            .tools
            .iter()
            .take(5)
            .map(|t| format!("{} ({})", t.tool, format_duration_words(t.duration_ms)))
            .collect();
        out.push_str(&format!("\n**AI tools used today:** {}\n", tools.join(", ")));
    }

    if !snapshot.meetings.is_empty() {
        out.push_str(&format!(
            "\n**Meetings today:** {}\n",
            snapshot.meetings.len()
        ));
        for m in snapshot.meetings.iter().take(3) {
            out.push_str(&format!("  - {}\n", m.title));
        }
    }

    out
}

fn format_local_date_from_ms(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Compact date-range summary for chat context. Much shorter than the full export markdown.
fn build_compact_weekly_context(export: &ExportPayload) -> String {
    let from = export.from_date.as_deref().unwrap_or("?");
    let to = export.to_date.as_deref().unwrap_or("?");
    let mut out = format!("## Activity range ({} to {})\n\n", from, to);

    let total_ms: i64 = export
        .work_sessions
        .iter()
        .map(|s| s.duration_ms.max(0))
        .sum();
    out.push_str(&format!(
        "**Total time:** {}  |  **Sessions:** {}\n",
        format_duration_words(total_ms),
        export.work_sessions.len()
    ));

    if !export.app_usage_summary.apps.is_empty() {
        out.push_str("\n**Top apps in this range:**\n");
        for app in export.app_usage_summary.apps.iter().take(6) {
            let pct = if total_ms > 0 {
                app.duration_ms * 100 / total_ms
            } else {
                0
            };
            out.push_str(&format!(
                "- {} [{}]: {} ({}%)\n",
                app.app,
                app.category,
                format_duration_words(app.duration_ms),
                pct
            ));
        }
    }

    let completed: Vec<_> = export
        .tasks
        .iter()
        .filter(|t| t.status == crate::models::TaskStatus::Done)
        .collect();
    let open: Vec<_> = export
        .tasks
        .iter()
        .filter(|t| t.status == crate::models::TaskStatus::Open)
        .collect();
    out.push_str(&format!(
        "\n**Tasks:** {} completed, {} open\n",
        completed.len(),
        open.len()
    ));

    if !export.commitments.is_empty() {
        out.push_str(&format!(
            "\n**Commitments tracked in this range:** {}\n",
            export.commitments.len()
        ));
    }

    if !export.ai_usage_summary.tools.is_empty() {
        let tools: Vec<String> = export
            .ai_usage_summary
            .tools
            .iter()
            .take(4)
            .map(|t| format!("{} ({})", t.tool, format_duration_words(t.duration_ms)))
            .collect();
        out.push_str(&format!("\n**AI tools used in this range:** {}\n", tools.join(", ")));
    }

    out
}

fn is_actionable_idle_block(block: &IdleBlock) -> bool {
    !block
        .evidence_json
        .as_deref()
        .is_some_and(|value| value.contains("source_event_gap"))
}

/// Returns true when a UTC timestamp (ms) falls outside [start_hour, end_hour)
/// in local wall-clock time. Both hours are 0–23 inclusive; end_hour=18 means
/// the window closes at 18:00 (6 pm).
fn is_outside_work_hours(timestamp_ms: i64, start_hour: i64, end_hour: i64) -> bool {
    use chrono::Timelike;
    // Guard against misconfigured range — treat as "always inside work hours".
    if end_hour <= start_hour {
        return false;
    }
    let local = Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .unwrap_or_else(|| Local.timestamp_millis_opt(0).single().unwrap());
    let hour = local.hour() as i64;
    hour < start_hour || hour >= end_hour
}

fn loop_risk_category(risk_type: &str) -> &'static str {
    match risk_type {
        "reply_debt" => "Reply",
        "ai_output_open" => "AI Output",
        "ghost_agent" => "Agent",
        "stale_hypothesis" => "Code Safety",
        "idle_unclassified" => "Away Time",
        "commitment_overdue" => "Promise",
        "meeting_actions" => "Meeting",
        "field_visit_debrief" => "Field Visit",
        _ => "Loop",
    }
}

fn build_timesheet_rows(
    sessions: &[WorkSessionSummary],
    events: &[SourceEvent],
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Vec<TimesheetRow> {
    let events_by_id = events
        .iter()
        .map(|event| (event.id.as_str(), event))
        .collect::<HashMap<_, _>>();

    sessions
        .iter()
        .map(|session| {
            let matched = session
                .evidence_event_ids
                .iter()
                .filter_map(|id| events_by_id.get(id.as_str()).copied())
                .collect::<Vec<_>>();
            let source_slice = if matched.is_empty() {
                events
                    .iter()
                    .filter(|event| {
                        event.started_at < session.ended_at && event.ended_at > session.started_at
                    })
                    .collect::<Vec<_>>()
            } else {
                matched
            };
            let mut apps = source_slice
                .iter()
                .filter_map(|event| event.app.as_deref().and_then(clean_app_label))
                .collect::<Vec<_>>();
            apps.sort();
            apps.dedup();
            let mut contexts = source_slice
                .iter()
                .map(|event| source_event_context_label(event))
                .collect::<Vec<_>>();
            contexts.sort();
            contexts.dedup();
            let mut ai_tools = source_slice
                .iter()
                .flat_map(|event| {
                    source_event_ai_tools(event)
                        .into_iter()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            ai_tools.sort();
            ai_tools.dedup();

            let started_at =
                from_ms.map_or(session.started_at, |from| session.started_at.max(from));
            let ended_at = to_ms.map_or(session.ended_at, |to| session.ended_at.min(to));
            let duration_ms = ended_at.saturating_sub(started_at);

            TimesheetRow {
                id: session.id.clone(),
                local_date: local_date_label(started_at),
                started_at,
                ended_at,
                duration_ms,
                title: session.title.clone(),
                category: if ai_tools.is_empty() {
                    "work".to_string()
                } else {
                    "ai_assisted".to_string()
                },
                app: apps.join(", "),
                project_or_client: session
                    .client_label
                    .clone()
                    .or_else(|| session.project_label.clone())
                    .or_else(|| contexts.first().cloned())
                    .unwrap_or_else(|| {
                        session
                            .summary
                            .clone()
                            .unwrap_or_else(|| "Captured context".to_string())
                    }),
                ai_used: !ai_tools.is_empty() || session.ai_used,
                ai_tools,
                confidence_percent: session.confidence_percent,
                evidence_ids: session.evidence_event_ids.clone(),
                billing_status: session.billing_status.clone(),
                billable: session.billable,
                client_label: session.client_label.clone(),
                project_label: session.project_label.clone(),
                ticket_id: session.ticket_id.clone(),
            }
        })
        .collect()
}

fn build_ai_contribution_rows(
    events: &[SourceEvent],
    outputs: &[WorkOutput],
    usage: &[AiUsage],
) -> Vec<AiContributionRow> {
    let usage_ids_linked_to_outputs = outputs
        .iter()
        .flat_map(|output| extract_evidence_ids(output.evidence_json.as_deref()))
        .collect::<HashSet<_>>();
    let mut rows = events
        .iter()
        .flat_map(|event| {
            source_event_ai_tools(event)
                .into_iter()
                .map(|tool| AiContributionRow {
                    id: format!("event-{}-{}", stable_id_part(tool), event.id),
                    tool: tool.to_string(),
                    app: event
                        .app
                        .as_deref()
                        .and_then(clean_app_label)
                        .unwrap_or_else(|| "AI tool".to_string()),
                    project_or_client: source_event_context_label(event),
                    started_at: event.started_at,
                    ended_at: event.ended_at,
                    duration_ms: event_duration_ms(event),
                    title: event
                        .title
                        .as_deref()
                        .and_then(clean_capture_label)
                        .unwrap_or_else(|| tool.to_string()),
                    destination: event
                        .domain
                        .clone()
                        .or_else(|| event.url_redacted.clone())
                        .unwrap_or_else(|| "Captured activity".to_string()),
                    status: "captured".to_string(),
                    evidence_ids: vec![event.id.clone()],
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    rows.extend(
        outputs
            .iter()
            .filter(|output| output.ai_assisted)
            .map(|output| AiContributionRow {
                id: format!("output-{}", output.id),
                tool: output
                    .source
                    .clone()
                    .unwrap_or_else(|| "AI-assisted output".to_string()),
                app: DISPLAY_APP_NAME.to_string(),
                project_or_client: output
                    .source
                    .clone()
                    .unwrap_or_else(|| "Captured context".to_string()),
                started_at: output.created_at,
                ended_at: output.updated_at,
                duration_ms: output.updated_at.saturating_sub(output.created_at),
                title: output.title.clone(),
                destination: output.output_type.clone(),
                status: output.status.clone(),
                evidence_ids: extract_evidence_ids(output.evidence_json.as_deref()),
            }),
    );

    rows.extend(
        usage
            .iter()
            .filter(|item| !usage_ids_linked_to_outputs.contains(&item.id))
            .filter(|item| !is_observed_ai_usage_row(item))
            .map(|item| AiContributionRow {
                id: format!("usage-{}", item.id),
                tool: item
                    .tool_name
                    .clone()
                    .or_else(|| item.provider.clone())
                    .unwrap_or_else(|| "AI".to_string()),
                app: item.provider.clone().unwrap_or_else(|| "AI".to_string()),
                project_or_client: item
                    .context_id
                    .clone()
                    .unwrap_or_else(|| "Captured context".to_string()),
                started_at: item.started_at.unwrap_or(item.created_at),
                ended_at: item.ended_at.unwrap_or(item.created_at),
                duration_ms: item.duration_ms.unwrap_or_default(),
                title: item
                    .thread_title
                    .clone()
                    .or_else(|| item.prompt_summary.clone())
                    .unwrap_or_else(|| "AI usage".to_string()),
                destination: item
                    .output_summary
                    .clone()
                    .unwrap_or_else(|| "AI response".to_string()),
                status: "recorded".to_string(),
                evidence_ids: vec![item.id.clone()],
            }),
    );

    rows.sort_by_key(|row| std::cmp::Reverse(row.started_at));
    rows
}

fn build_calendar_reconciliation(
    calendar_events: &[CalendarEvent],
    work_sessions: &[WorkSessionSummary],
    source_events: &[SourceEvent],
) -> CalendarReconciliation {
    let planned = calendar_events
        .iter()
        .filter(|event| !calendar_event_cancelled(event))
        .collect::<Vec<_>>();
    let mut items = Vec::new();
    let mut matched_events = 0usize;
    let mut planned_duration_ms = 0i64;
    let mut actual_overlap_ms = 0i64;

    for event in planned {
        let duration_ms = event.ends_at.saturating_sub(event.starts_at);
        planned_duration_ms += duration_ms;
        let overlapping_sources = source_events
            .iter()
            .filter(|source| source.started_at < event.ends_at && source.ended_at > event.starts_at)
            .collect::<Vec<_>>();
        let intervals = overlapping_sources
            .iter()
            .map(|source| {
                (
                    source.started_at.max(event.starts_at),
                    source.ended_at.min(event.ends_at),
                )
            })
            .collect::<Vec<_>>();
        let overlap_ms = merged_interval_duration_ms(intervals);
        actual_overlap_ms += overlap_ms;

        let matched_session_ids = work_sessions
            .iter()
            .filter(|session| {
                session.started_at < event.ends_at && session.ended_at > event.starts_at
            })
            .map(|session| session.id.clone())
            .collect::<Vec<_>>();
        let matched_source_event_ids = overlapping_sources
            .iter()
            .map(|source| source.id.clone())
            .collect::<Vec<_>>();
        let strong_signal = overlapping_sources
            .iter()
            .any(|source| calendar_event_matches_source(event, source));
        let enough_overlap =
            overlap_ms >= 5 * 60_000 && overlap_ms.saturating_mul(4) >= duration_ms.max(1);
        let status = if overlap_ms == 0 {
            "missed"
        } else if strong_signal || enough_overlap {
            matched_events += 1;
            "matched"
        } else {
            "partial"
        };
        let evidence_label = overlapping_sources
            .iter()
            .max_by_key(|source| {
                source
                    .ended_at
                    .min(event.ends_at)
                    .saturating_sub(source.started_at.max(event.starts_at))
            })
            .map(|source| source_event_short_label(source));

        items.push(CalendarReconciliationItem {
            id: event.id.clone(),
            title: event.title.clone(),
            starts_at: event.starts_at,
            ends_at: event.ends_at,
            status: status.to_string(),
            actual_overlap_ms: overlap_ms,
            matched_session_ids,
            matched_source_event_ids,
            evidence_label,
        });
    }

    items.sort_by_key(|item| item.starts_at);
    let planned_events = items.len();
    CalendarReconciliation {
        planned_events,
        matched_events,
        unmatched_events: planned_events.saturating_sub(matched_events),
        planned_duration_ms,
        actual_overlap_ms,
        items,
    }
}

fn calendar_event_cancelled(event: &CalendarEvent) -> bool {
    matches!(
        event.status.trim().to_ascii_lowercase().as_str(),
        "cancelled" | "canceled" | "declined"
    )
}

fn calendar_event_matches_source(event: &CalendarEvent, source: &SourceEvent) -> bool {
    let source_text = [
        source.app.as_deref(),
        source.title.as_deref(),
        source.domain.as_deref(),
        source.url_redacted.as_deref(),
        source.workspace_key.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(normalize_match_text)
    .collect::<Vec<_>>()
    .join(" ");

    calendar_match_terms(event)
        .iter()
        .any(|term| !term.is_empty() && source_text.contains(term))
}

fn calendar_match_terms(event: &CalendarEvent) -> Vec<String> {
    [
        Some(event.title.as_str()),
        event.location.as_deref(),
        event.planned_work_type.as_deref(),
        event.calendar_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .flat_map(match_terms)
    .collect()
}

fn summarize_focus_session(
    focus: &FocusSessionRecord,
    events: &[SourceEvent],
    now: i64,
) -> FocusSessionSummary {
    let ended_at = focus.ended_at.unwrap_or(now);
    let overlapping_events = events
        .iter()
        .filter(|event| event.started_at < ended_at && event.ended_at > focus.started_at)
        .collect::<Vec<_>>();
    let mut matched_intervals = Vec::new();
    let mut drift_intervals = Vec::new();
    let mut evidence_event_ids = Vec::new();
    let mut drift_events = Vec::new();

    for event in overlapping_events {
        let interval = (
            event.started_at.max(focus.started_at),
            event.ended_at.min(ended_at),
        );
        if interval.1 <= interval.0 {
            continue;
        }
        evidence_event_ids.push(event.id.clone());
        if focus_matches_source(focus, event) {
            matched_intervals.push(interval);
        } else {
            drift_intervals.push(interval);
            let label = source_event_short_label(event);
            if !drift_events.contains(&label) {
                drift_events.push(label);
            }
        }
    }

    let actual_duration_ms = ended_at.saturating_sub(focus.started_at);
    let matched_work_ms = merged_interval_duration_ms(matched_intervals);
    let drift_ms = merged_interval_duration_ms(drift_intervals);

    FocusSessionSummary {
        id: focus.id.clone(),
        goal: focus.goal.clone(),
        client: focus.client.clone(),
        project: focus.project.clone(),
        task: focus.task.clone(),
        ticket_id: focus.ticket_id.clone(),
        target_ms: focus.target_ms,
        started_at: focus.started_at,
        ended_at: focus.ended_at,
        status: focus.status.clone(),
        actual_duration_ms,
        matched_work_ms,
        drift_ms,
        evidence_event_ids,
        drift_events,
        created_at: focus.created_at,
        updated_at: focus.updated_at,
    }
}

fn focus_matches_source(focus: &FocusSessionRecord, source: &SourceEvent) -> bool {
    let source_text = [
        source.app.as_deref(),
        source.title.as_deref(),
        source.domain.as_deref(),
        source.url_redacted.as_deref(),
        source.workspace_key.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(normalize_match_text)
    .collect::<Vec<_>>()
    .join(" ");
    let terms = [
        Some(focus.goal.as_str()),
        focus.client.as_deref(),
        focus.project.as_deref(),
        focus.task.as_deref(),
        focus.ticket_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .flat_map(match_terms)
    .collect::<Vec<_>>();

    terms
        .iter()
        .any(|term| !term.is_empty() && source_text.contains(term))
}

fn match_terms(value: &str) -> Vec<String> {
    let normalized = normalize_match_text(value);
    let mut terms = Vec::new();
    if normalized.len() >= 3 {
        terms.push(normalized.clone());
    }
    terms.extend(
        normalized
            .split_whitespace()
            .filter(|part| part.len() >= 3)
            .map(str::to_string),
    );
    terms.sort();
    terms.dedup();
    terms
}

fn normalize_match_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn merged_interval_duration_ms(mut intervals: Vec<(i64, i64)>) -> i64 {
    intervals.retain(|(start, end)| end > start);
    intervals.sort_by_key(|(start, _)| *start);
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (start, end) in intervals {
        if let Some((_, last_end)) = merged.last_mut() {
            if start <= *last_end {
                *last_end = (*last_end).max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
        .into_iter()
        .map(|(start, end)| end.saturating_sub(start))
        .sum()
}

fn source_event_short_label(event: &SourceEvent) -> String {
    [
        event.app.as_deref().and_then(clean_app_label),
        event.title.as_deref().and_then(clean_capture_label),
        event.domain.as_deref().and_then(clean_capture_label),
        event.workspace_key.as_deref().and_then(clean_capture_label),
    ]
    .into_iter()
    .flatten()
    .next()
    .unwrap_or_else(|| "Captured activity".to_string())
}

fn build_ai_output_ledger(
    events: &[SourceEvent],
    ai_outputs: &[WorkOutput],
) -> Vec<AiOutputLedgerItem> {
    let mut ledger = Vec::new();
    let events_by_id = events
        .iter()
        .map(|event| (event.id.as_str(), event))
        .collect::<HashMap<_, _>>();
    let mut seen_event_ids = HashSet::new();

    for output in ai_outputs.iter().take(20) {
        let evidence_ids = extract_evidence_ids(output.evidence_json.as_deref());
        let evidence_events = evidence_ids
            .iter()
            .filter_map(|id| events_by_id.get(id.as_str()).copied())
            .collect::<Vec<_>>();
        for id in &evidence_ids {
            seen_event_ids.insert(id.clone());
        }
        let tool = output
            .source
            .as_deref()
            .and_then(|source| detect_ai_tool(None, Some(source), None, None))
            .or_else(|| {
                evidence_events
                    .iter()
                    .find_map(|event| source_event_ai_tool(event))
            })
            .unwrap_or("AI");
        let source_context = evidence_events
            .first()
            .map(|event| source_event_context_label(event))
            .or_else(|| output.source.clone())
            .unwrap_or_else(|| "AI-assisted work".to_string());
        let duration_ms = evidence_events
            .iter()
            .map(|event| event_duration_ms(event))
            .sum();

        ledger.push(AiOutputLedgerItem {
            id: format!("output-{}", output.id),
            title: output.title.clone(),
            tool: tool.to_string(),
            source_context,
            destination: output.output_type.clone(),
            status: output.status.clone(),
            duration_ms,
            evidence_ids,
            evidence: output
                .evidence_json
                .clone()
                .unwrap_or_else(|| "No explicit evidence linked yet".to_string()),
        });
    }

    for event in events
        .iter()
        .filter(|event| !seen_event_ids.contains(&event.id))
    {
        for tool in source_event_ai_tools(event) {
            ledger.push(AiOutputLedgerItem {
                id: format!("ai-event-{}-{}", stable_id_part(tool), event.id),
                title: event
                    .title
                    .as_deref()
                    .and_then(clean_capture_label)
                    .unwrap_or_else(|| format!("{tool} activity")),
                tool: tool.to_string(),
                source_context: source_event_context_label(event),
                destination: "Observed usage".to_string(),
                status: "captured".to_string(),
                duration_ms: event_duration_ms(event),
                evidence_ids: vec![event.id.clone()],
                evidence: event
                    .url_redacted
                    .clone()
                    .or_else(|| event.workspace_key.clone())
                    .or_else(|| event.domain.clone())
                    .unwrap_or_else(|| event.source.clone()),
            });
        }
    }

    ledger.sort_by_key(|item| std::cmp::Reverse(item.duration_ms));
    ledger.truncate(30);
    ledger
}

fn build_menu_bar_summary(
    events: &[SourceEvent],
    sessions: &[WorkSessionSummary],
    ai_usage: &AiUsageSummary,
    loops: &[UnclosedLoopItem],
    pause_state: &PauseState,
    next_action: Option<&NextBestAction>,
) -> MenuBarSummary {
    let latest_event = events.iter().max_by_key(|event| event.ended_at);
    let current_work = latest_event
        .map(|event| {
            let app = event.app.as_deref().and_then(clean_app_label);
            let context = Some(source_event_context_label(event));
            match (app, context) {
                (Some(app), Some(context)) if app != context => format!("{app} - {context}"),
                (Some(app), _) => app,
                (_, Some(context)) => context,
                _ => "Waiting for activity".to_string(),
            }
        })
        .or_else(|| sessions.first().map(|session| session.title.clone()))
        .unwrap_or_else(|| "Waiting for activity".to_string());
    let detail = latest_event
        .and_then(|event| event.title.as_deref().and_then(clean_capture_label))
        .or_else(|| sessions.first().and_then(|session| session.summary.clone()))
        .unwrap_or_else(|| "Open an app, editor, terminal, browser, or AI tool.".to_string());

    MenuBarSummary {
        current_work,
        detail,
        capture_state: if pause_state.paused {
            "Paused".to_string()
        } else {
            "Capturing".to_string()
        },
        ai_usage: format_duration_words(ai_usage.total_duration_ms),
        open_loops: loops.len(),
        next_action: next_action.map(|action| action.title.clone()),
        updated_at: now_ms(),
    }
}

fn extract_evidence_ids(value: Option<&str>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    if let Ok(ids) = serde_json::from_str::<Vec<String>>(value) {
        return ids
            .into_iter()
            .filter_map(|id| clean_capture_label(&id))
            .collect();
    }
    if let Ok(value) = serde_json::from_str::<Value>(value) {
        if let Some(ids) = value
            .get("ids")
            .or_else(|| value.get("evidenceIds"))
            .and_then(Value::as_array)
        {
            return ids
                .iter()
                .filter_map(Value::as_str)
                .filter_map(clean_capture_label)
                .collect();
        }
    }
    clean_capture_label(value).into_iter().collect()
}

fn format_duration_words(duration_ms: i64) -> String {
    let minutes = (duration_ms.max(0) + 59_999) / 60_000;
    if minutes < 1 {
        return "<1m".to_string();
    }
    let hours = minutes / 60;
    let remainder = minutes % 60;
    if hours == 0 {
        format!("{minutes}m")
    } else if remainder == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {remainder}m")
    }
}

fn ai_tools_from_events(events: &[&SourceEvent]) -> Vec<AiToolUsage> {
    let mut buckets: HashMap<String, UsageBucket> = HashMap::new();
    for event in events {
        let context = source_event_context_label(event);
        for tool in source_event_ai_tools(event) {
            let bucket = buckets.entry(tool.to_string()).or_default();
            bucket.duration_ms += event_duration_ms(event);
            bucket.events += 1;
            *bucket.contexts.entry(context.clone()).or_default() += event_duration_ms(event);
        }
    }

    let mut tools = buckets
        .into_iter()
        .map(|(tool, bucket)| {
            let mut contexts = bucket.contexts.into_keys().collect::<Vec<_>>();
            contexts.sort();
            AiToolUsage {
                tool,
                duration_ms: bucket.duration_ms,
                events: bucket.events,
                contexts,
            }
        })
        .collect::<Vec<_>>();
    tools.sort_by_key(|tool| std::cmp::Reverse(tool.duration_ms));
    tools
}

fn source_event_ai_tool(event: &SourceEvent) -> Option<&'static str> {
    // When a real domain is captured, do NOT fall back to the app name as the
    // title — this prevents "ChatGPT Atlas" (a browser whose name contains
    // "chatgpt") from being flagged as AI when the user is actually visiting
    // Gmail, YouTube, or any non-AI site inside that browser.
    let title = if event.domain.is_some() {
        event.title.as_deref()
    } else {
        event.title.as_deref().or(event.app.as_deref())
    };
    detect_ai_tool(
        event.domain.as_deref(),
        title,
        event.url_redacted.as_deref(),
        event.metadata_json.as_deref(),
    )
}

fn source_event_ai_tools(event: &SourceEvent) -> Vec<&'static str> {
    let title = if event.domain.is_some() {
        event.title.as_deref()
    } else {
        event.title.as_deref().or(event.app.as_deref())
    };
    detect_ai_tools(
        event.domain.as_deref(),
        title,
        event.url_redacted.as_deref(),
        event.metadata_json.as_deref(),
    )
}

fn source_event_context_label(event: &SourceEvent) -> String {
    event
        .workspace_key
        .as_deref()
        .and_then(clean_capture_label)
        .map(|label| compact_capture_label(&label))
        .or_else(|| event.domain.as_deref().and_then(clean_capture_label))
        .or_else(|| {
            event
                .title
                .as_deref()
                .and_then(clean_capture_label)
                .map(|title| compact_capture_label(&title))
        })
        .or_else(|| event.app.as_deref().and_then(clean_app_label))
        .unwrap_or_else(|| "Captured activity".to_string())
}

fn source_event_full_context_label(event: &SourceEvent) -> String {
    event
        .workspace_key
        .as_deref()
        .and_then(clean_capture_label)
        .or_else(|| event.url_redacted.as_deref().and_then(clean_capture_label))
        .or_else(|| event.domain.as_deref().and_then(clean_capture_label))
        .or_else(|| event.title.as_deref().and_then(clean_capture_label))
        .or_else(|| event.app.as_deref().and_then(clean_app_label))
        .unwrap_or_else(|| "Captured activity".to_string())
}

fn ai_usage_context_label(value: &str) -> Option<String> {
    let cleaned = clean_report_text(value);
    if cleaned.is_empty() || cleaned.starts_with("context-") {
        return None;
    }
    if cleaned == "worktrace:internal" {
        return Some(DISPLAY_APP_NAME.to_string());
    }
    Some(cleaned)
}

fn event_duration_ms(event: &SourceEvent) -> i64 {
    event
        .duration_ms
        .max(event.ended_at.saturating_sub(event.started_at))
        .max(0)
}

/// Compute the total duration of the union of event intervals (ms).
/// Prevents double-counting when overlapping events exist.
fn merge_event_intervals(events: &[&SourceEvent]) -> i64 {
    let mut intervals: Vec<(i64, i64)> = events
        .iter()
        .map(|e| {
            let start = e.started_at;
            let end = e.ended_at.max(e.started_at + e.duration_ms).max(start);
            (start, end)
        })
        .filter(|(s, e)| e > s)
        .collect();
    intervals.sort_unstable_by_key(|&(s, _)| s);

    let mut total = 0_i64;
    let mut merge_start = i64::MIN;
    let mut merge_end = i64::MIN;
    for (s, e) in intervals {
        if s <= merge_end {
            merge_end = merge_end.max(e);
        } else {
            if merge_end > merge_start {
                total += merge_end - merge_start;
            }
            merge_start = s;
            merge_end = e;
        }
    }
    if merge_end > merge_start {
        total += merge_end - merge_start;
    }
    total
}

fn push_example(examples: &mut Vec<String>, value: String) {
    if examples.len() < 4 && !examples.iter().any(|existing| existing == &value) {
        examples.push(value);
    }
}

fn stable_id_part(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
        } else if !output.ends_with('-') {
            output.push('-');
        }
    }
    output.trim_matches('-').to_string()
}

fn context_id_from_key(key: &str) -> String {
    let stable = stable_id_part(key);
    if stable.is_empty() {
        "context-unknown".to_string()
    } else {
        format!("context-{stable}")
    }
}

fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };

    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                dir_size(&path)
            } else {
                file_size(&path)
            }
        })
        .sum()
}

fn parse_rfc3339_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp_millis())
}

fn redact_ai_context(value: &str) -> String {
    value
        .lines()
        .map(redact_sensitive_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_sensitive_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let sensitive = [
        "api_key",
        "apikey",
        "api-key",
        "authorization",
        "bearer ",
        "password",
        "passwd",
        "secret",
        "token",
        "access_token",
        "refresh_token",
        "x-goog-api-key",
        "x-api-key",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    if !sensitive {
        return line.to_string();
    }

    if let Some((left, _)) = line.split_once(':') {
        return format!("{left}: [redacted]");
    }
    if let Some((left, _)) = line.split_once('=') {
        return format!("{left}=[redacted]");
    }
    "[redacted sensitive line]".to_string()
}

fn parse_local_date_start_ms(value: &str) -> Option<i64> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    Some(local_day_bounds_ms(date).0)
}

fn parse_export_text_ms(value: &str) -> Option<i64> {
    parse_rfc3339_ms(value).or_else(|| parse_local_date_start_ms(value))
}

fn local_date_label(timestamp_ms: i64) -> String {
    Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .or_else(|| Local.timestamp_millis_opt(timestamp_ms).earliest())
        .map(|date_time| date_time.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| Local::now().date_naive().to_string())
}

fn timestamp_in_export_range(timestamp_ms: i64, from_ms: Option<i64>, to_ms: Option<i64>) -> bool {
    from_ms.is_none_or(|from| timestamp_ms >= from) && to_ms.is_none_or(|to| timestamp_ms < to)
}

fn timespan_in_export_range(
    started_at: i64,
    ended_at: i64,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> bool {
    from_ms.is_none_or(|from| ended_at >= from) && to_ms.is_none_or(|to| started_at < to)
}

fn text_timestamp_in_export_range(value: &str, from_ms: Option<i64>, to_ms: Option<i64>) -> bool {
    parse_export_text_ms(value)
        .is_some_and(|timestamp| timestamp_in_export_range(timestamp, from_ms, to_ms))
}

fn task_in_export_range(task: &Task, from_ms: Option<i64>, to_ms: Option<i64>) -> bool {
    range_unbounded(from_ms, to_ms)
        || text_timestamp_in_export_range(&task.created_at, from_ms, to_ms)
        || text_timestamp_in_export_range(&task.updated_at, from_ms, to_ms)
        || task
            .due_date
            .as_deref()
            .is_some_and(|date| text_timestamp_in_export_range(date, from_ms, to_ms))
}

fn quick_note_in_export_range(note: &QuickNote, from_ms: Option<i64>, to_ms: Option<i64>) -> bool {
    range_unbounded(from_ms, to_ms)
        || text_timestamp_in_export_range(&note.created_at, from_ms, to_ms)
}

fn commitment_in_export_range(
    commitment: &Commitment,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> bool {
    range_unbounded(from_ms, to_ms)
        || commitment
            .due_at
            .is_some_and(|due_at| timestamp_in_export_range(due_at, from_ms, to_ms))
        || timestamp_in_export_range(commitment.created_at, from_ms, to_ms)
        || timestamp_in_export_range(commitment.updated_at, from_ms, to_ms)
}

fn pending_reply_in_export_range(
    reply: &EmailThread,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> bool {
    range_unbounded(from_ms, to_ms)
        || reply
            .latest_at
            .is_some_and(|latest_at| timestamp_in_export_range(latest_at, from_ms, to_ms))
        || timestamp_in_export_range(reply.created_at, from_ms, to_ms)
        || timestamp_in_export_range(reply.updated_at, from_ms, to_ms)
}

fn work_output_in_export_range(
    output: &WorkOutput,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> bool {
    range_unbounded(from_ms, to_ms)
        || timespan_in_export_range(output.created_at, output.updated_at, from_ms, to_ms)
}

fn meeting_in_export_range(meeting: &Meeting, from_ms: Option<i64>, to_ms: Option<i64>) -> bool {
    if range_unbounded(from_ms, to_ms) {
        return true;
    }

    if let Some(start) = meeting.starts_at {
        let end = meeting.ends_at.unwrap_or(start);
        if timespan_in_export_range(start, end, from_ms, to_ms) {
            return true;
        }
    }

    timestamp_in_export_range(meeting.created_at, from_ms, to_ms)
        || timestamp_in_export_range(meeting.updated_at, from_ms, to_ms)
}

fn field_visit_in_export_range(
    visit: &FieldVisit,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> bool {
    range_unbounded(from_ms, to_ms)
        || timespan_in_export_range(
            visit.starts_at,
            visit.ends_at.unwrap_or(visit.starts_at),
            from_ms,
            to_ms,
        )
        || timestamp_in_export_range(visit.created_at, from_ms, to_ms)
        || timestamp_in_export_range(visit.updated_at, from_ms, to_ms)
}

fn range_unbounded(from_ms: Option<i64>, to_ms: Option<i64>) -> bool {
    from_ms.is_none() && to_ms.is_none()
}

fn filter_tasks_by_range(items: Vec<Task>, from_ms: Option<i64>, to_ms: Option<i64>) -> Vec<Task> {
    items
        .into_iter()
        .filter(|task| task_in_export_range(task, from_ms, to_ms))
        .collect()
}

fn filter_quick_notes_by_range(
    items: Vec<QuickNote>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Vec<QuickNote> {
    items
        .into_iter()
        .filter(|note| quick_note_in_export_range(note, from_ms, to_ms))
        .collect()
}

fn filter_commitments_by_range(
    items: Vec<Commitment>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Vec<Commitment> {
    items
        .into_iter()
        .filter(|commitment| commitment_in_export_range(commitment, from_ms, to_ms))
        .collect()
}

fn filter_pending_replies_by_range(
    items: Vec<EmailThread>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Vec<EmailThread> {
    items
        .into_iter()
        .filter(|reply| pending_reply_in_export_range(reply, from_ms, to_ms))
        .collect()
}

fn filter_work_outputs_by_range(
    items: Vec<WorkOutput>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Vec<WorkOutput> {
    items
        .into_iter()
        .filter(|output| work_output_in_export_range(output, from_ms, to_ms))
        .collect()
}

fn filter_meetings_by_range(
    items: Vec<Meeting>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Vec<Meeting> {
    items
        .into_iter()
        .filter(|meeting| meeting_in_export_range(meeting, from_ms, to_ms))
        .collect()
}

fn filter_field_visits_by_range(
    items: Vec<FieldVisit>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Vec<FieldVisit> {
    items
        .into_iter()
        .filter(|visit| field_visit_in_export_range(visit, from_ms, to_ms))
        .collect()
}

fn redact_terminal_command(command: &str) -> String {
    let mut redact_next = false;
    let mut redacted = Vec::new();

    for part in command.split_whitespace() {
        let lower = part.to_ascii_lowercase();
        let sensitive_flag = matches!(
            lower.as_str(),
            "-p" | "--password" | "--pass" | "--token" | "--api-key" | "--apikey" | "--secret"
        );

        if redact_next {
            redacted.push("[redacted]".to_string());
            redact_next = false;
            continue;
        }

        if sensitive_flag {
            redacted.push(part.to_string());
            redact_next = true;
            continue;
        }

        if let Some((key, _)) = part.split_once('=') {
            let key_lower = key.to_ascii_lowercase();
            if [
                "password", "passwd", "token", "api_key", "apikey", "secret", "key",
            ]
            .iter()
            .any(|needle| key_lower.contains(needle))
            {
                redacted.push(format!("{key}=[redacted]"));
                continue;
            }
        }

        redacted.push(part.to_string());
    }

    redacted.join(" ")
}

fn read_terminal_bridge_metadata(path: &Path) -> Result<Option<TerminalBridgeMetadata>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read terminal bridge {}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&contents)
        .with_context(|| format!("invalid terminal bridge JSON {}", path.display()))
        .map(Some)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut output = Vec::new();
    for path in paths {
        if !output.iter().any(|existing: &PathBuf| existing == &path) {
            output.push(path);
        }
    }
    Ok(output)
}

fn horizon_end_ms(horizon: &str) -> i64 {
    let days = if horizon == "week" { 6 } else { 0 };
    let date = Local::now().date_naive() + ChronoDuration::days(days);
    local_date_to_epoch_ms(&date.format("%Y-%m-%d").to_string()).unwrap_or_else(now_ms)
}

fn date_range_to_ms(
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> Result<(Option<i64>, Option<i64>)> {
    let from_ms = from_date
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            let d = NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .with_context(|| format!("invalid from_date: {v}"))?;
            Ok::<i64, anyhow::Error>(local_day_bounds_ms(d).0)
        })
        .transpose()?;
    let to_ms = to_date
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            let d = NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .with_context(|| format!("invalid to_date: {v}"))?;
            Ok::<i64, anyhow::Error>(local_day_bounds_ms(d).1)
        })
        .transpose()?;
    Ok((from_ms, to_ms))
}

fn format_duration_md(ms: i64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

fn format_time_label(epoch_ms: i64) -> String {
    let dt = Local.timestamp_millis_opt(epoch_ms).single();
    dt.map(|t| t.format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".to_string())
}

fn export_range_bounds(range: &ExportRangeInput) -> Result<(Option<i64>, Option<i64>)> {
    let from_ms = range
        .from_date
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .with_context(|| format!("invalid from_date: {value}"))?;
            Ok::<i64, anyhow::Error>(local_day_bounds_ms(date).0)
        })
        .transpose()?;
    let to_ms = range
        .to_date
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .with_context(|| format!("invalid to_date: {value}"))?;
            Ok::<i64, anyhow::Error>(local_day_bounds_ms(date).1)
        })
        .transpose()?;

    if let (Some(from_ms), Some(to_ms)) = (from_ms, to_ms) {
        anyhow::ensure!(
            from_ms < to_ms,
            "from_date must be before or equal to to_date"
        );
    }

    Ok((from_ms, to_ms))
}

fn ensure_sqlite_integrity(path: &Path) -> Result<()> {
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open sqlite database {}", path.display()))?;
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .with_context(|| format!("failed to verify sqlite database {}", path.display()))?;
    anyhow::ensure!(
        integrity == "ok",
        "sqlite integrity check failed for {}: {}",
        path.display(),
        integrity
    );
    Ok(())
}

fn local_day_bounds_ms(date: NaiveDate) -> (i64, i64) {
    fn local_start_ms(date: NaiveDate) -> i64 {
        let naive = date
            .and_hms_opt(0, 0, 0)
            .expect("midnight should be valid for date");
        Local
            .from_local_datetime(&naive)
            .single()
            .or_else(|| Local.from_local_datetime(&naive).earliest())
            .map(|date_time| date_time.timestamp_millis())
            .unwrap_or_else(now_ms)
    }

    let start = local_start_ms(date);
    let end = local_start_ms(date + ChronoDuration::days(1));
    (start, end)
}

fn local_date_to_epoch_ms(value: &str) -> Option<i64> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    let naive = date.and_hms_opt(23, 59, 59)?;
    Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())
        .map(|date_time| date_time.timestamp_millis())
}

fn epoch_ms_to_local_date(value: i64) -> Option<String> {
    Local
        .timestamp_millis_opt(value)
        .single()
        .or_else(|| Local.timestamp_millis_opt(value).earliest())
        .map(|date_time| date_time.format("%Y-%m-%d").to_string())
}

fn normalize_task_priority(value: &str) -> String {
    match value.trim().to_lowercase().as_str() {
        "high" => "high".to_string(),
        "low" => "low".to_string(),
        _ => "medium".to_string(),
    }
}

fn parse_task_drafts_from_text(text: &str, default_priority: &str) -> Vec<TaskDraft> {
    text.lines()
        .filter_map(clean_task_draft_title)
        .take(50)
        .map(|title| TaskDraft {
            title,
            due_date: None,
            due_at: None,
            notes: None,
            priority: Some(default_priority.to_string()),
            client_label: None,
            project_label: None,
        })
        .collect()
}

fn parse_task_drafts_from_ai_output(
    output: &str,
    default_priority: &str,
) -> Option<Vec<TaskDraft>> {
    let json_text = extract_json_payload(output);
    let value: Value = serde_json::from_str(&json_text).ok()?;
    let rows = value
        .as_array()
        .or_else(|| value.get("tasks").and_then(Value::as_array))?;
    let drafts = rows
        .iter()
        .filter_map(|row| {
            let title = row
                .get("title")
                .and_then(Value::as_str)
                .and_then(clean_task_draft_title)?;
            Some(TaskDraft {
                title,
                due_date: row
                    .get("dueDate")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                due_at: row.get("dueAt").and_then(Value::as_i64),
                notes: row
                    .get("notes")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                priority: Some(
                    row.get("priority")
                        .and_then(Value::as_str)
                        .map(normalize_task_priority)
                        .unwrap_or_else(|| default_priority.to_string()),
                ),
                client_label: row
                    .get("clientLabel")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                project_label: row
                    .get("projectLabel")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
            })
        })
        .take(50)
        .collect::<Vec<_>>();

    Some(drafts)
}

fn extract_json_payload(output: &str) -> String {
    let trimmed = output.trim();
    if let Some(start) = trimmed.find("```") {
        if let Some(end) = trimmed[start + 3..].find("```") {
            let fenced = &trimmed[start + 3..start + 3 + end];
            return fenced
                .trim()
                .strip_prefix("json")
                .unwrap_or(fenced.trim())
                .trim()
                .to_string();
        }
    }
    trimmed.to_string()
}

fn clean_task_draft_title(value: &str) -> Option<String> {
    let mut title = value.trim();
    if title.is_empty() {
        return None;
    }
    title = title
        .trim_start_matches(|c: char| c == '-' || c == '*' || c == '•' || c.is_whitespace())
        .trim();
    let digit_count = title
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .map(char::len_utf8)
        .sum::<usize>();
    if digit_count > 0 {
        let rest = &title[digit_count..];
        if rest.starts_with('.') || rest.starts_with(')') {
            title = rest[1..].trim();
        }
    }
    title = title
        .trim_start_matches("[ ]")
        .trim_start_matches("[x]")
        .trim_start_matches("[X]")
        .trim();

    let title = title
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`')
        .trim();
    if title.len() < 2 {
        return None;
    }
    Some(title.chars().take(160).collect())
}

fn task_priority_rank(task: &Task) -> i64 {
    match task.priority.as_deref() {
        Some("high") => 1,
        Some("medium") => 2,
        Some("low") => 3,
        _ => 3,
    }
}

fn task_due_reason(task: &Task) -> String {
    if let Some(due_at) = task.due_at {
        return format!("Due {}", format_task_due_at_label(due_at));
    }
    task.due_date
        .as_ref()
        .map(|date| format!("Due {date}"))
        .unwrap_or_else(|| "Open task without a due date".into())
}

fn format_task_due_at_label(due_at: i64) -> String {
    Local
        .timestamp_millis_opt(due_at)
        .single()
        .or_else(|| Local.timestamp_millis_opt(due_at).earliest())
        .map(|date_time| date_time.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| due_at.to_string())
}

fn format_task_report_line(task: &Task) -> String {
    let marker = if task.status == TaskStatus::Done {
        "x"
    } else {
        " "
    };
    let mut details = Vec::new();

    if let Some(due_at) = task.due_at {
        details.push(format!("due {}", format_task_due_at_label(due_at)));
    } else if let Some(due_date) = task.due_date.as_deref() {
        details.push(format!("due {}", clean_report_text(due_date)));
    }

    if let Some(priority) = task.priority.as_deref().filter(|value| !value.is_empty()) {
        details.push(format!("priority {}", clean_report_text(priority)));
    }

    let context = [task.client_label.as_deref(), task.project_label.as_deref()]
        .into_iter()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .map(clean_report_text)
        .collect::<Vec<_>>();
    if !context.is_empty() {
        details.push(context.join(" / "));
    }

    let mut line = format!("- [{}] {}", marker, clean_report_text(&task.title));
    if !details.is_empty() {
        line.push_str(&format!(" ({})", details.join("; ")));
    }
    line.push('\n');
    if let Some(notes) = task
        .notes
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        line.push_str(&format!("  - Notes: {}\n", clean_report_text(notes)));
    }
    line
}

fn date_is_on_or_before(value: &str, boundary: NaiveDate) -> bool {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok_and(|date| date <= boundary)
}

fn build_capacity_summary(snapshot: &TodaySnapshot, horizon: &str) -> String {
    let meeting_minutes: i64 = snapshot
        .meetings
        .iter()
        .filter_map(|meeting| meeting.starts_at.zip(meeting.ends_at))
        .map(|(start, end)| end.saturating_sub(start).saturating_div(60_000))
        .sum();
    let field_minutes: i64 = snapshot
        .field_visits
        .iter()
        .filter_map(|visit| visit.ends_at.map(|end| (visit.starts_at, end)))
        .map(|(start, end)| end.saturating_sub(start).saturating_div(60_000))
        .sum();
    let unclassified_idle = snapshot
        .idle_blocks
        .iter()
        .filter(|block| !block.classified)
        .count();

    if meeting_minutes == 0 && field_minutes == 0 && unclassified_idle == 0 {
        return format!("No calendar or offline load captured for the {horizon} plan.");
    }

    format!(
        "{}m meetings, {}m field/offline work, {} unclassified idle block(s).",
        meeting_minutes, field_minutes, unclassified_idle
    )
}

fn build_plan_markdown(
    title: &str,
    must_close: &[PlanningItem],
    should_progress: &[PlanningItem],
    can_defer: &[PlanningItem],
    waiting: &[PlanningItem],
    at_risk: &[PlanningItem],
    capacity_summary: &str,
) -> String {
    let mut markdown = String::new();
    markdown.push_str(&format!("# {}\n\n", clean_report_text(title)));
    markdown.push_str("## Capacity\n");
    markdown.push_str(&format!("- {}\n", clean_report_text(capacity_summary)));
    append_planning_section(&mut markdown, "Must close", must_close);
    append_planning_section(&mut markdown, "Should progress", should_progress);
    append_planning_section(&mut markdown, "Can defer", can_defer);
    append_planning_section(&mut markdown, "Waiting for others", waiting);
    append_planning_section(&mut markdown, "At risk", at_risk);
    markdown.push_str("\n## Operating rule\n");
    markdown
        .push_str("- Close reply debt and due commitments before starting new speculative work.\n");
    markdown
}

fn append_planning_section(markdown: &mut String, title: &str, items: &[PlanningItem]) {
    markdown.push_str(&format!("\n## {title}\n"));
    if items.is_empty() {
        markdown.push_str("- None captured.\n");
        return;
    }

    for item in items {
        markdown.push_str(&format!(
            "- {} — {} ({})\n",
            clean_report_text(&item.title),
            clean_report_text(&item.reason),
            clean_report_text(&item.source)
        ));
    }
}

fn build_weekly_review_markdown_from_export(export: &ExportPayload) -> String {
    let mut markdown = String::new();
    let range_label = match (&export.from_date, &export.to_date) {
        (Some(from), Some(to)) => format!("{from} to {to}"),
        (Some(from), None) => format!("since {from}"),
        (None, Some(to)) => format!("through {to}"),
        _ => "latest captured range".to_string(),
    };

    markdown.push_str(&format!("# Weekly Work Review - {range_label}\n\n"));
    markdown.push_str("## AI weekly auto-draft\n");
    markdown.push_str(
        "- Source-backed draft generated from local DayTrail evidence. Review before sharing.\n",
    );

    markdown.push_str("\n## Movement\n");
    markdown.push_str(&format!(
        "- {} work session(s), {} source event(s), {} AI contribution(s).\n",
        export.work_sessions.len(),
        export.source_events.len(),
        export.ai_contribution_rows.len()
    ));
    markdown.push_str(&format!(
        "- Planned vs actual: {}/{} calendar event(s) matched, {} missed.\n",
        export.calendar_reconciliation.matched_events,
        export.calendar_reconciliation.planned_events,
        export.calendar_reconciliation.unmatched_events
    ));
    markdown.push_str(&format!(
        "- {} focus session(s), {} focus drift, {} review item(s).\n",
        export.focus_sessions.len(),
        format_duration_words(
            export
                .focus_sessions
                .iter()
                .map(|session| session.drift_ms)
                .sum()
        ),
        export.unclosed_loop_inbox.len() + export.inferred_work_blocks.len()
    ));
    markdown.push_str(&format!(
        "- Smart Break score {}, longest uninterrupted {}, {} taken, {} skipped.\n",
        export.recovery_summary.score,
        format_duration_words(export.recovery_summary.longest_uninterrupted_ms),
        export.recovery_summary.taken_count,
        export.recovery_summary.skipped_count
    ));

    markdown.push_str("\n## Work completed\n");
    if export.work_sessions.is_empty() && export.source_events.is_empty() {
        markdown.push_str("- No captured work sessions in this week range.\n");
    } else {
        for session in export.work_sessions.iter().take(8) {
            markdown.push_str(&format!(
                "- {}: {} ({}).\n",
                clean_report_text(&session.status),
                clean_report_text(&session.title),
                format_duration_words(session.duration_ms)
            ));
        }
        for event in export.source_events.iter().take(8) {
            let label = event
                .title
                .as_deref()
                .and_then(clean_capture_label)
                .or_else(|| event.app.as_deref().and_then(clean_app_label))
                .unwrap_or_else(|| "Captured activity".to_string());
            markdown.push_str(&format!(
                "- Evidence: {} in {}.\n",
                clean_report_text(&label),
                clean_report_text(&source_event_context_label(event))
            ));
        }
    }

    markdown.push_str("\n## Planned vs actual\n");
    if export.calendar_reconciliation.items.is_empty() {
        markdown.push_str("- No calendar events imported for this week.\n");
    } else {
        for item in export.calendar_reconciliation.items.iter().take(10) {
            markdown.push_str(&format!(
                "- {}: {} ({} overlap{}).\n",
                clean_report_text(&item.status),
                clean_report_text(&item.title),
                format_duration_words(item.actual_overlap_ms),
                item.evidence_label
                    .as_ref()
                    .map(|label| format!(", evidence: {}", clean_report_text(label)))
                    .unwrap_or_default()
            ));
        }
    }

    markdown.push_str("\n## Smart Breaks\n");
    if export.recovery_summary.total_screen_ms <= 0 {
        markdown.push_str("- No Smart Break data captured for this week.\n");
    } else {
        markdown.push_str(&format!(
            "- Score: {} based on {} screen time.\n",
            export.recovery_summary.score,
            format_duration_words(export.recovery_summary.total_screen_ms)
        ));
        markdown.push_str(&format!(
            "- Longest uninterrupted: {}.\n",
            format_duration_words(export.recovery_summary.longest_uninterrupted_ms)
        ));
        markdown.push_str(&format!(
            "- {} taken, {} skipped, {} snoozed.\n",
            export.recovery_summary.taken_count,
            export.recovery_summary.skipped_count,
            export.recovery_summary.snoozed_count
        ));
    }

    markdown.push_str("\n## Focus recovery\n");
    if export.focus_sessions.is_empty() {
        markdown.push_str("- No focus sessions captured for this week.\n");
    } else {
        for focus in export.focus_sessions.iter().take(10) {
            markdown.push_str(&format!(
                "- {}: {} matched work, {} drift{}.\n",
                clean_report_text(&focus.goal),
                format_duration_words(focus.matched_work_ms),
                format_duration_words(focus.drift_ms),
                if focus.drift_events.is_empty() {
                    String::new()
                } else {
                    format!(
                        " ({})",
                        focus
                            .drift_events
                            .iter()
                            .take(3)
                            .map(|event| clean_report_text(event))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            ));
        }
    }

    markdown.push_str("\n## AI work\n");
    if export.ai_contribution_rows.is_empty() {
        markdown.push_str("- No AI-assisted contributions captured for this week.\n");
    } else {
        for row in export.ai_contribution_rows.iter().take(10) {
            markdown.push_str(&format!(
                "- {}: {} ({}).\n",
                clean_report_text(&row.tool),
                clean_report_text(&row.title),
                format_duration_words(row.duration_ms)
            ));
        }
    }

    markdown.push_str("\n## Risks and follow-ups\n");
    if export.unclosed_loop_inbox.is_empty()
        && export.inferred_work_blocks.is_empty()
        && export.pending_replies.is_empty()
        && export.commitments.is_empty()
    {
        markdown.push_str("- No reply debt, open commitments, or review items captured.\n");
    } else {
        for item in export.unclosed_loop_inbox.iter().take(10) {
            markdown.push_str(&format!(
                "- {}: {} - {}\n",
                clean_report_text(&item.category),
                clean_report_text(&item.title),
                clean_report_text(&item.detail)
            ));
        }
        for block in export.inferred_work_blocks.iter().take(8) {
            markdown.push_str(&format!(
                "- Confirm inferred block: {} - {} ({})\n",
                clean_report_text(&block.title),
                clean_report_text(&block.reason),
                format_duration_words(block.duration_ms)
            ));
        }
        for reply in export.pending_replies.iter().take(5) {
            markdown.push_str(&format!(
                "- Reply debt: {}\n",
                clean_report_text(&reply.subject)
            ));
        }
        for commitment in export.commitments.iter().take(5) {
            markdown.push_str(&format!(
                "- Commitment: {}\n",
                clean_report_text(&commitment.title)
            ));
        }
    }

    markdown
}

fn build_daily_report_markdown(snapshot: &TodaySnapshot, include_system_apps: bool) -> String {
    let mut markdown = String::new();
    markdown.push_str(&format!(
        "# Daily Work Report - {}\n\n",
        snapshot.local_date
    ));

    let session_total_ms: i64 = snapshot
        .work_sessions
        .iter()
        .map(|session| session.duration_ms.max(0))
        .sum();
    let tracked_ms = if session_total_ms > 0 {
        session_total_ms
    } else {
        snapshot.app_usage_summary.total_duration_ms.max(0)
    };
    let needs_review_count = snapshot.unclosed_loop_inbox.len()
        + snapshot.inferred_work_blocks.len()
        + snapshot.loop_risks.len()
        + snapshot
            .idle_blocks
            .iter()
            .filter(|block| !block.classified)
            .count()
        + snapshot
            .work_sessions
            .iter()
            .filter(|session| session.billing_status == "draft")
            .count();

    markdown.push_str("## Summary\n");
    markdown.push_str(&format!(
        "- Tracking is {}.\n",
        if snapshot.pause_state.paused {
            "paused"
        } else {
            "active"
        }
    ));
    markdown.push_str(&format!(
        "- Worked for {} across {} work session(s).\n",
        format_duration_words(tracked_ms),
        snapshot.work_sessions.len()
    ));
    if let Some(session) = snapshot.work_sessions.first() {
        markdown.push_str(&format!(
            "- Main thread: {} for {}.\n",
            clean_report_text(&session.title),
            format_duration_words(session.duration_ms)
        ));
    }
    if !snapshot.ai_usage_summary.tools.is_empty() {
        markdown.push_str(&format!(
            "- AI tools detected: {}.\n",
            snapshot
                .ai_usage_summary
                .tools
                .iter()
                .take(5)
                .map(|tool| clean_report_text(&tool.tool))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(context) = &snapshot.project_context {
        markdown.push_str(&format!(
            "- Active context: {}\n",
            clean_report_text(&context.path)
        ));
    }

    markdown.push_str("\n## What happened\n");
    if snapshot.work_sessions.is_empty() {
        markdown.push_str(
            "- Keep working for a few minutes and DayTrail will summarize the main work threads.\n",
        );
    } else {
        for session in snapshot.work_sessions.iter().take(6) {
            markdown.push_str(&session_story_bullet(snapshot, session));
        }
    }

    markdown.push_str("\n## Work sessions\n");
    if snapshot.work_sessions.is_empty() {
        markdown.push_str("- No work sessions captured yet.\n");
    } else {
        for session in snapshot.work_sessions.iter().take(6) {
            markdown.push_str(&format!(
                "- {} - {} ({})\n",
                clean_report_text(&session.title),
                format_duration_words(session.duration_ms),
                clean_report_text(&session.status)
            ));
            if let Some(summary) = session.summary.as_deref().and_then(clean_capture_label) {
                markdown.push_str(&format!("  - {}\n", clean_report_text(&summary)));
            }
        }
    }

    markdown.push_str("\n## Apps used\n");
    let visible_apps = snapshot
        .app_usage_summary
        .apps
        .iter()
        .filter(|app| include_system_apps || is_simple_visible_report_app(&app.app))
        .collect::<Vec<_>>();

    if !visible_apps.is_empty() {
        markdown.push_str(&format!(
            "- Total app time: {}\n",
            format_duration_words(visible_apps.iter().map(|app| app.duration_ms.max(0)).sum())
        ));
        for app in visible_apps.into_iter().take(8) {
            let project_label = app
                .projects
                .first()
                .map(|project| clean_report_text(&project.label))
                .unwrap_or_else(|| "no project detected".to_string());
            let ai_tools = if app.ai_tools.is_empty() {
                "no AI detected".to_string()
            } else {
                app.ai_tools
                    .iter()
                    .take(3)
                    .map(|tool| clean_report_text(&tool.tool))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let example_label = app
                .projects
                .iter()
                .flat_map(|project| project.examples.iter())
                .next()
                .map(|example| clean_report_text(example))
                .filter(|example| !example.is_empty());
            markdown.push_str(&format!(
                "- {} - {} on {}{} (AI: {})\n",
                clean_report_text(&app.app),
                format_duration_words(app.duration_ms),
                project_label,
                example_label
                    .as_deref()
                    .map(|example| format!(" - {example}"))
                    .unwrap_or_default(),
                ai_tools
            ));
        }
    } else {
        markdown.push_str("- No app activity captured yet.\n");
    }

    markdown.push_str("\n## AI detected\n");
    if snapshot.ai_usage_summary.tools.is_empty() {
        markdown.push_str("- No AI activity detected from apps or browser tabs.\n");
    } else {
        markdown.push_str(&format!(
            "- Total AI activity: {} across {} tool(s).\n",
            format_duration_words(snapshot.ai_usage_summary.total_duration_ms),
            snapshot.ai_usage_summary.tools.len()
        ));
        for tool in snapshot.ai_usage_summary.tools.iter().take(8) {
            let contexts = if tool.contexts.is_empty() {
                "no context".to_string()
            } else {
                tool.contexts
                    .iter()
                    .take(3)
                    .map(|context| clean_report_text(context))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            markdown.push_str(&format!(
                "- {} - {} ({})\n",
                clean_report_text(&tool.tool),
                format_duration_words(tool.duration_ms),
                contexts
            ));
        }
    }

    markdown.push_str("\n## Needs review\n");
    if needs_review_count == 0 {
        markdown.push_str("- No review items detected from current sessions.\n");
    } else {
        for item in snapshot.unclosed_loop_inbox.iter().take(5) {
            markdown.push_str(&format!(
                "- {} - {}\n",
                clean_report_text(&item.title),
                clean_report_text(&item.detail)
            ));
        }
        for risk in snapshot.loop_risks.iter().take(5) {
            markdown.push_str(&format!(
                "- {} - {}\n",
                clean_report_text(&risk.title),
                clean_report_text(&risk.reason)
            ));
        }
        for block in snapshot.inferred_work_blocks.iter().take(5) {
            markdown.push_str(&format!(
                "- Confirm inferred block: {} - {} ({})\n",
                clean_report_text(&block.title),
                clean_report_text(&block.reason),
                format_duration_words(block.duration_ms)
            ));
        }
        for block in snapshot
            .idle_blocks
            .iter()
            .filter(|block| !block.classified)
            .take(3)
        {
            markdown.push_str(&format!(
                "- Classify idle gap: {}\n",
                format_duration_words(block.duration_ms)
            ));
        }
        for session in snapshot
            .work_sessions
            .iter()
            .filter(|session| session.billing_status == "draft")
            .take(5)
        {
            markdown.push_str(&format!(
                "- Review draft session: {}\n",
                clean_report_text(&session.title)
            ));
        }
    }

    markdown.push_str("\n## Tasks and commitments\n");
    if snapshot.tasks.is_empty() {
        markdown.push_str("- No tasks captured.\n");
    } else {
        for task in &snapshot.tasks {
            markdown.push_str(&format_task_report_line(task));
        }
    }
    if snapshot.commitments.is_empty() {
        markdown.push_str("- No open commitments captured.\n");
    } else {
        for commitment in &snapshot.commitments {
            markdown.push_str(&format!(
                "- [promise] {}\n",
                clean_report_text(&commitment.title)
            ));
        }
    }

    markdown.push_str("\n## Reply debt\n");
    if snapshot.pending_replies.is_empty() {
        markdown.push_str("- No pending replies detected.\n");
    } else {
        for thread in &snapshot.pending_replies {
            markdown.push_str(&format!("- {}\n", clean_report_text(&thread.subject)));
        }
    }

    markdown.push_str("\n## AI-generated deliverables\n");
    if snapshot.ai_outputs.is_empty() {
        markdown.push_str(
            "- No deliverable records captured yet. AI tool usage is reported separately above.\n",
        );
    } else {
        for output in &snapshot.ai_outputs {
            let ai_marker = if output.ai_assisted {
                "AI-assisted"
            } else {
                "manual"
            };
            markdown.push_str(&format!(
                "- [{}] {} ({})\n",
                clean_report_text(&output.status),
                clean_report_text(&output.title),
                ai_marker
            ));
        }
    }

    markdown.push_str("\n## Meetings, visits, and idle recovery\n");
    if snapshot.meetings.is_empty()
        && snapshot.field_visits.is_empty()
        && snapshot.idle_blocks.is_empty()
    {
        markdown.push_str("- No meetings, field visits, or idle blocks captured.\n");
    } else {
        for meeting in &snapshot.meetings {
            markdown.push_str(&format!(
                "- [meeting] {}\n",
                clean_report_text(&meeting.title)
            ));
        }
        for visit in &snapshot.field_visits {
            let label = visit
                .client_label
                .as_deref()
                .or(visit.location_label.as_deref())
                .unwrap_or("field visit");
            markdown.push_str(&format!(
                "- [field] {} ({})\n",
                clean_report_text(label),
                clean_report_text(&visit.status)
            ));
        }
        for block in &snapshot.idle_blocks {
            if !block.classified {
                markdown.push_str(&format!(
                    "- [idle] {} minutes need classification\n",
                    block.duration_ms / 60_000
                ));
            }
        }
    }

    markdown.push_str("\n## Unclosed loops and safety nets\n");
    if snapshot.loop_risks.is_empty() {
        markdown.push_str("- No unclosed loops detected.\n");
    } else {
        for risk in snapshot.loop_risks.iter().take(10) {
            markdown.push_str(&format!(
                "- [{}] {} — {}\n",
                clean_report_text(&risk.risk_type),
                clean_report_text(&risk.title),
                clean_report_text(&risk.reason)
            ));
        }
    }

    markdown.push_str("\n## Manual notes\n");
    if snapshot.quick_notes.is_empty() {
        markdown.push_str("- No manual notes captured.\n");
    } else {
        for note in &snapshot.quick_notes {
            markdown.push_str(&format!("- {}\n", clean_report_text(&note.body)));
        }
    }

    markdown.push_str("\n## Privacy posture\n");
    markdown.push_str("- Full clipboard content is stored only if you enable it.\n");
    markdown.push_str("- AI export uses configured redaction and local settings.\n");

    markdown
}

fn session_story_bullet(snapshot: &TodaySnapshot, session: &WorkSessionSummary) -> String {
    let evidence_ids = session
        .evidence_event_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let events = snapshot
        .source_events
        .iter()
        .filter(|event| {
            if !evidence_ids.is_empty() {
                evidence_ids.contains(event.id.as_str())
            } else {
                event.started_at < session.ended_at && event.ended_at > session.started_at
            }
        })
        .collect::<Vec<_>>();

    let mut app_durations: HashMap<String, i64> = HashMap::new();
    let mut contexts = Vec::new();
    for event in events {
        if let Some(app) = event.app.as_deref().and_then(clean_app_label) {
            if is_simple_visible_report_app(&app) {
                *app_durations.entry(app).or_default() += event.duration_ms.max(0);
            }
        }
        if let Some(context) = event
            .workspace_key
            .as_deref()
            .or(event.domain.as_deref())
            .and_then(clean_capture_label)
        {
            if !contexts.iter().any(|existing| existing == &context) {
                contexts.push(context);
            }
        }
    }
    let mut apps = app_durations.into_iter().collect::<Vec<_>>();
    apps.sort_by_key(|(_, duration_ms)| std::cmp::Reverse(*duration_ms));
    let app_label = apps
        .iter()
        .take(3)
        .map(|(app, duration_ms)| format!("{} ({})", app, format_duration_words(*duration_ms)))
        .collect::<Vec<_>>()
        .join(", ");
    let context_label = contexts
        .into_iter()
        .take(2)
        .map(|context| clean_report_text(&context))
        .collect::<Vec<_>>()
        .join(", ");

    let mut parts = vec![
        clean_report_text(&session.title),
        format_duration_words(session.duration_ms),
    ];
    if !app_label.is_empty() {
        parts.push(format!("mostly in {app_label}"));
    }
    if !context_label.is_empty() {
        parts.push(format!("context: {context_label}"));
    }
    if session.ai_used {
        parts.push("AI tools detected".to_string());
    }

    format!("- {}\n", parts.join(" - "))
}

fn classify_app_category(app_name: &str) -> &'static str {
    let normalized = app_name
        .chars()
        .filter(|ch| !matches!(*ch, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}'))
        .collect::<String>()
        .trim()
        .to_lowercase();
    if normalized.is_empty() {
        return "unknown";
    }
    if normalized == "idle" || normalized == "away" || normalized.contains("idle") {
        return "idle";
    }

    const SYSTEM_APPS: &[&str] = &[
        "system settings",
        "system preferences",
        "activity monitor",
        "notification center",
        "usernotificationcenter",
        "loginwindow",
        "windowserver",
        "control center",
        "dock",
        "problem reporter",
    ];
    const UTILITY_APPS: &[&str] = &[
        "finder",
        "preview",
        "textedit",
        "quicktime player",
        "archive utility",
        "font book",
    ];
    const AI_APPS: &[&str] = &[
        "chatgpt",
        "claude",
        "claude code",
        "gemini",
        "copilot",
        "github copilot",
        "codex",
        "aider",
        "cline",
    ];
    const COMMUNICATION_APPS: &[&str] = &[
        "slack",
        "microsoft teams",
        "teams",
        "mail",
        "outlook",
        "messages",
        "discord",
        "zoom",
        "google meet",
    ];
    const BROWSER_APPS: &[&str] = &[
        "safari",
        "firefox",
        "firefox developer edition",
        "google chrome",
        "google chrome canary",
        "chrome",
        "brave browser",
        "brave",
        "microsoft edge",
        "edge",
        "arc",
        "chromium",
        "opera",
        "vivaldi",
        "chatgpt atlas",
    ];
    const WORK_APPS: &[&str] = &[
        "code",
        "visual studio code",
        "vs code",
        "vs code insiders",
        "cursor",
        "terminal",
        "iterm",
        "iterm2",
        "warp",
        "xcode",
        "intellij idea",
        "webstorm",
        "pycharm",
        "goland",
        "datagrip",
        "zed",
        "sublime text",
        "mysql workbench",
        "postman",
        "docker",
        "figma",
        "notion",
        "obsidian",
    ];

    if SYSTEM_APPS
        .iter()
        .any(|name| normalized == *name || normalized.contains(*name))
    {
        return "system";
    }
    if UTILITY_APPS
        .iter()
        .any(|name| normalized == *name || normalized.contains(*name))
    {
        return "utility";
    }
    if AI_APPS
        .iter()
        .any(|name| normalized == *name || normalized.contains(*name))
    {
        return "ai";
    }
    if COMMUNICATION_APPS
        .iter()
        .any(|name| normalized == *name || normalized.contains(*name))
    {
        return "communication";
    }
    if BROWSER_APPS
        .iter()
        .any(|name| normalized == *name || normalized.contains(*name))
    {
        return "browser";
    }
    if WORK_APPS
        .iter()
        .any(|name| normalized == *name || normalized.contains(*name))
    {
        return "work";
    }
    "unknown"
}

fn is_simple_visible_report_app(app_name: &str) -> bool {
    !matches!(
        classify_app_category(app_name),
        "system" | "utility" | "idle" | "unknown"
    )
}

fn build_export_analysis_markdown(export: &ExportPayload) -> String {
    let mut markdown = String::new();
    markdown.push_str("# DayTrail Routine and Automation Analysis\n\n");
    markdown.push_str("## Scope\n");
    markdown.push_str(&format!(
        "- Range: {} to {}\n",
        export.from_date.as_deref().unwrap_or("beginning"),
        export.to_date.as_deref().unwrap_or("latest")
    ));
    markdown.push_str(&format!(
        "- Raw signals: {} activity record(s), {} session(s), {} idle block(s), {} AI usage record(s).\n",
        export.source_events.len(),
        export.work_sessions.len(),
        export.idle_blocks.len(),
        export.ai_usage.len()
    ));
    markdown.push_str(&format!(
        "- Reporting rows: {} observed activity row(s), {} AI contribution row(s).\n",
        export.timesheet_rows.len(),
        export.ai_contribution_rows.len()
    ));

    markdown.push_str("\n## App usage\n");
    if export.app_usage_summary.apps.is_empty() {
        markdown.push_str("- No app usage captured in this range.\n");
    } else {
        for app in export.app_usage_summary.apps.iter().take(8) {
            markdown.push_str(&format!(
                "- {}: {} across {} event(s)\n",
                clean_report_text(&app.app),
                format_duration_words(app.duration_ms),
                app.events
            ));
        }
    }

    markdown.push_str("\n## AI usage\n");
    if export.ai_usage_summary.tools.is_empty() {
        markdown.push_str("- No AI tool usage detected in this range.\n");
    } else {
        for tool in export.ai_usage_summary.tools.iter().take(8) {
            markdown.push_str(&format!(
                "- {}: {} across {} event(s)\n",
                clean_report_text(&tool.tool),
                format_duration_words(tool.duration_ms),
                tool.events
            ));
        }
    }

    markdown.push_str("\n## Automation candidates\n");
    if export.automation_candidates.is_empty() {
        markdown.push_str("- No repeated routines crossed the automation threshold.\n");
    } else {
        for candidate in export.automation_candidates.iter().take(8) {
            markdown.push_str(&format!(
                "- {}: {} occurrence(s), {}. {}\n",
                clean_report_text(&candidate.title),
                candidate.occurrences,
                format_duration_words(candidate.duration_ms),
                clean_report_text(&candidate.reason)
            ));
        }
    }

    markdown.push_str("\n## Inferred blocks to confirm\n");
    if export.inferred_work_blocks.is_empty() {
        markdown.push_str("- No inferred work blocks need confirmation in this range.\n");
    } else {
        for block in export.inferred_work_blocks.iter().take(8) {
            markdown.push_str(&format!(
                "- {}: {} confidence, {}. {}\n",
                clean_report_text(&block.title),
                clean_report_text(&block.confidence),
                format_duration_words(block.duration_ms),
                clean_report_text(&block.reason)
            ));
        }
    }

    markdown.push_str("\n## Open loops\n");
    if export.unclosed_loop_inbox.is_empty() {
        markdown.push_str("- No open loops in the exported snapshot.\n");
    } else {
        for item in export.unclosed_loop_inbox.iter().take(10) {
            markdown.push_str(&format!(
                "- [{}] {} — {}\n",
                clean_report_text(&item.category),
                clean_report_text(&item.title),
                clean_report_text(&item.primary_action)
            ));
        }
    }

    markdown
}

fn clean_report_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_for_audit(value: &str, max_chars: usize) -> String {
    let cleaned = clean_report_text(value);
    if cleaned.chars().count() <= max_chars {
        return cleaned;
    }

    let mut output = cleaned
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    output.push_str("...");
    output
}

fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(clean_report_text)
        .filter(|value| !value.is_empty())
}

fn file_name_from_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(value)
        .to_string()
}

fn first_words(value: &str, count: usize) -> String {
    let words = value.split_whitespace().take(count).collect::<Vec<_>>();
    if words.is_empty() {
        "Untitled".into()
    } else {
        words.join(" ")
    }
}

fn build_fts_query(query: &str) -> Option<String> {
    let terms = query
        .split_whitespace()
        .map(|term| {
            term.trim_matches(|character: char| {
                !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
            })
        })
        .filter(|term| !term.is_empty())
        .take(8)
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();

    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

fn parse_string_list_setting(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value)
        .map(normalize_string_list)
        .unwrap_or_default()
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_notification_sound(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "glass" => "glass".to_string(),
        "subtle" => "subtle".to_string(),
        "none" => "none".to_string(),
        _ => "daytrail".to_string(),
    }
}

fn is_excluded(value: &str, excluded_values: &[String]) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    excluded_values
        .iter()
        .any(|excluded| normalized == *excluded || normalized.ends_with(&format!(".{excluded}")))
}

fn is_project_excluded(value: &str, excluded_values: &[String]) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    excluded_values.iter().any(|excluded| {
        normalized == *excluded
            || normalized.starts_with(&format!("{excluded}/"))
            || normalized.starts_with(&format!("{excluded}\\"))
    })
}

fn active_window_context_is_excluded(
    app_name: &str,
    url: Option<&str>,
    workspace_key: Option<&str>,
    settings: &Settings,
) -> bool {
    if is_excluded(app_name, &settings.excluded_apps) {
        return true;
    }
    if url
        .and_then(|value| redact_url(value).0)
        .is_some_and(|domain| is_excluded(&domain, &settings.excluded_domains))
    {
        return true;
    }
    workspace_key
        .filter(|value| !value.trim().is_empty())
        .is_some_and(|value| is_project_excluded(value, &settings.excluded_projects))
}

fn redact_url(value: &str) -> (Option<String>, Option<String>) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return (None, None);
    }

    if let Ok(parsed) = url::Url::parse(trimmed) {
        let domain = parsed.host_str().map(|host| host.to_ascii_lowercase());
        if matches!(parsed.scheme(), "http" | "https") {
            let origin = parsed.origin().ascii_serialization();
            let path = domain
                .as_deref()
                .filter(|domain| should_preserve_ai_thread_path(domain))
                .and_then(|_| redacted_ai_path(&parsed))
                .unwrap_or_default();
            return (domain, Some(format!("{origin}{path}")));
        }

        let mut redacted = parsed.clone();
        redacted.set_query(None);
        redacted.set_fragment(None);
        return (domain, Some(redacted.to_string()));
    }

    let without_fragment = trimmed.split('#').next().unwrap_or_default();
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or_default()
        .trim();
    let redacted = if without_query.is_empty() {
        None
    } else {
        Some(without_query.to_string())
    };
    (None, redacted)
}

fn should_preserve_ai_thread_path(domain: &str) -> bool {
    matches!(
        domain,
        "chatgpt.com"
            | "claude.ai"
            | "gemini.google.com"
            | "aistudio.google.com"
            | "copilot.microsoft.com"
    )
}

fn redacted_ai_path(parsed: &url::Url) -> Option<String> {
    let segments: Vec<&str> = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .take(2)
        .collect();

    if segments.is_empty() {
        None
    } else {
        Some(format!("/{}", segments.join("/")))
    }
}

fn redact_metadata_url_fields(metadata: Option<&str>) -> Result<(Option<String>, bool)> {
    let Some(metadata) = metadata else {
        return Ok((None, false));
    };
    let Ok(mut value) = serde_json::from_str::<Value>(metadata) else {
        return Ok((Some(metadata.to_string()), false));
    };

    let mut changed = false;
    redact_json_url_fields(&mut value, &mut changed);
    if changed {
        Ok((Some(serde_json::to_string(&value)?), true))
    } else {
        Ok((Some(metadata.to_string()), false))
    }
}

fn redact_json_url_fields(value: &mut Value, changed: &mut bool) {
    match value {
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                if matches!(key.as_str(), "url" | "uri") {
                    if let Value::String(raw) = item {
                        if let Some(redacted) = redact_url(raw).1 {
                            if redacted != *raw {
                                *raw = redacted;
                                *changed = true;
                            }
                        }
                    }
                } else {
                    redact_json_url_fields(item, changed);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_json_url_fields(item, changed);
            }
        }
        _ => {}
    }
}

pub fn default_database_path() -> Result<PathBuf> {
    let base = dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .context("failed to resolve data directory")?;
    Ok(base.join(DATA_DIR_NAME).join(DB_FILE_NAME))
}

fn goal_event_matches(goal: &crate::models::DailyGoal, event: &SourceEvent) -> bool {
    let mv = goal.match_value.to_ascii_lowercase();
    match goal.target_type.as_str() {
        "app" => event
            .app
            .as_deref()
            .map(|a| a.to_ascii_lowercase() == mv)
            .unwrap_or(false),
        "project" => event
            .workspace_key
            .as_deref()
            .map(|k| {
                let kl = k.to_ascii_lowercase();
                kl == mv || kl.starts_with(&format!("{mv}/")) || kl.starts_with(&format!("{mv}\\"))
            })
            .unwrap_or(false),
        "category" => {
            let app_lower = event.app.as_deref().unwrap_or("").to_ascii_lowercase();
            categorize_app(&app_lower) == mv
        }
        _ => false,
    }
}

/// Rough app categorization matching frontend categories.
fn categorize_app(app_lower: &str) -> &'static str {
    if matches!(
        app_lower,
        "code" | "cursor" | "zed" | "xcode" | "vim" | "nvim" | "neovim"
            | "intellij idea" | "pycharm" | "webstorm" | "rider" | "clion"
    ) || app_lower.starts_with("visual studio")
    {
        "development"
    } else if matches!(app_lower, "terminal" | "iterm2" | "alacritty" | "warp" | "hyper" | "kitty" | "ghostty") {
        "terminal"
    } else if matches!(
        app_lower,
        "google chrome" | "safari" | "firefox" | "arc" | "edge" | "brave browser"
    ) {
        "browser"
    } else if matches!(
        app_lower,
        "slack" | "discord" | "teams" | "zoom" | "webex" | "telegram" | "whatsapp"
    ) {
        "communication"
    } else if matches!(app_lower, "notion" | "obsidian" | "bear" | "notes" | "roam research") {
        "notes"
    } else {
        "other"
    }
}

/// Parse `git commit -m "message"` / `git commit --message="msg"` etc.
fn extract_git_commit_message(last_command: Option<&str>) -> Option<String> {
    let cmd = last_command?.trim();
    // Must start with `git commit`
    let rest = cmd.strip_prefix("git commit")?.trim_start();
    // Walk args looking for -m / --message
    let args: Vec<String> = parse_shell_args(rest);
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-m" || arg == "--message" {
            if let Some(msg) = args.get(i + 1) {
                return Some(msg.trim_matches(|c| c == '\'' || c == '"').to_string());
            }
        } else if let Some(msg) = arg.strip_prefix("--message=") {
            return Some(msg.trim_matches(|c| c == '\'' || c == '"').to_string());
        } else if let Some(msg) = arg.strip_prefix("-m=") {
            return Some(msg.trim_matches(|c| c == '\'' || c == '"').to_string());
        }
        i += 1;
    }
    None
}

fn parse_shell_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for c in input.chars() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pause_state() -> PauseState {
        PauseState {
            paused: false,
            reason: None,
            updated_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn chat_lookback_detects_two_week_requests() {
        assert_eq!(
            chat_lookback_for_message("From last two weeks what are my routine tasks?"),
            ChatLookback::TwoWeeks
        );
        assert_eq!(
            chat_lookback_for_message("show me patterns from the past 14 days"),
            ChatLookback::TwoWeeks
        );
    }

    #[test]
    fn chat_lookback_detects_data_coverage_questions() {
        assert_eq!(
            chat_lookback_for_message("from when you have the data?"),
            ChatLookback::Quarter
        );
        assert_eq!(
            chat_lookback_for_message("how far back does DayTrail have available data"),
            ChatLookback::Quarter
        );
    }

    #[test]
    fn capture_health_degrades_when_required_os_permissions_are_missing() {
        let summary = build_capture_health_with_permission_state(
            &[],
            &Settings::default(),
            &test_pause_state(),
            false,
            crate::active_window::CaptureLiveness::Unknown,
            None,
        );

        assert_eq!(summary.status, "needs_setup");
        assert!(summary.checks.iter().any(|check| {
            check.id == "os-permissions"
                && check.status == "needs_setup"
                && check.detail.contains("Accessibility")
        }));
    }

    #[test]
    fn capture_health_does_not_overstate_empty_capture_as_healthy() {
        let settings = Settings {
            terminal_bridge_path: Some("/tmp/daytrail-shell-integration".into()),
            ..Default::default()
        };
        let summary = build_capture_health_with_permission_state(
            &[],
            &settings,
            &test_pause_state(),
            true,
            crate::active_window::CaptureLiveness::Unknown,
            None,
        );

        assert_eq!(summary.status, "warming_up");
        assert!(summary
            .checks
            .iter()
            .any(|check| check.id == "os-permissions" && check.status == "ok"));
    }

    #[test]
    fn capture_health_raises_an_error_when_the_watcher_has_stalled() {
        // A stalled watcher must override the otherwise-reassuring status so the
        // user is told capture stopped (the silent-stop bug made visible).
        let summary = build_capture_health_with_permission_state(
            &[],
            &Settings::default(),
            &test_pause_state(),
            true,
            crate::active_window::CaptureLiveness::Stalled,
            None,
        );

        assert_eq!(summary.status, "error");
        assert!(summary.headline.to_lowercase().contains("stopped"));
        assert!(summary.checks.iter().any(|check| {
            check.id == "capture-watcher" && check.status == "error" && check.action.is_some()
        }));
    }

    #[test]
    fn capture_health_flags_revoked_accessibility_permission() {
        let summary = build_capture_health_with_permission_state(
            &[],
            &Settings::default(),
            &test_pause_state(),
            true,
            crate::active_window::CaptureLiveness::PermissionLost,
            None,
        );

        assert_eq!(summary.status, "error");
        assert!(summary.headline.to_lowercase().contains("accessibility"));
    }

    #[test]
    fn capture_health_pause_outranks_a_stalled_watcher() {
        // An intentional pause is not an error; don't cry wolf.
        let mut paused = test_pause_state();
        paused.paused = true;
        let summary = build_capture_health_with_permission_state(
            &[],
            &Settings::default(),
            &paused,
            true,
            crate::active_window::CaptureLiveness::Stalled,
            None,
        );

        assert_eq!(summary.status, "paused");
    }

    #[test]
    fn idle_system_apps_include_macos_and_windows_lock_screens() {
        for app in ["loginwindow", "LockApp", "LogonUI", "Windows Security"] {
            assert!(is_idle_system_app(app));
        }

        for app in ["VS Code", "Google Chrome", "Slack"] {
            assert!(!is_idle_system_app(app));
        }
    }

    // ── Activity ↔ task links ──────────────────────────────────────────────

    fn temp_store() -> (tempfile::TempDir, WorktraceStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = WorktraceStore::open(dir.path().join("test.sqlite3")).expect("open store");
        (dir, store)
    }

    #[test]
    fn chat_context_includes_database_coverage() {
        let (_dir, store) = temp_store();
        let started_at = Local
            .with_ymd_and_hms(2026, 5, 25, 10, 0, 0)
            .single()
            .expect("valid local date")
            .timestamp_millis();
        store
            .record_source_event(SourceEventInput {
                id: Some("coverage-event".to_string()),
                source: "window".into(),
                event_type: "window".into(),
                app: Some("Editor".into()),
                title: Some("First useful record".into()),
                url: None,
                workspace_key: None,
                started_at: Some(started_at),
                ended_at: Some(started_at + 60_000),
                sensitivity: None,
                metadata_json: None,
            })
            .expect("record event");

        let (context, sources) = store
            .build_chat_context("from when you have the data?")
            .expect("build chat context");

        assert!(sources.contains(&"available data coverage".to_string()));
        assert!(context.contains("Available data coverage"));
        assert!(context.contains("2026-05-25"));
    }

    fn seed_event(store: &WorktraceStore, id: &str, title: &str) -> SourceEvent {
        store
            .record_source_event(SourceEventInput {
                id: Some(id.to_string()),
                source: "window".into(),
                event_type: "window".into(),
                app: Some("Editor".into()),
                title: Some(title.into()),
                url: None,
                workspace_key: None,
                started_at: Some(1_000),
                ended_at: Some(2_000),
                sensitivity: None,
                metadata_json: None,
            })
            .expect("record event")
    }

    fn seed_task(store: &WorktraceStore, title: &str) -> Task {
        store
            .create_task(TaskInput {
                title: title.into(),
                due_date: None,
                due_at: None,
                notes: None,
                priority: None,
                source: None,
                project_path: None,
                client_label: None,
                project_label: None,
            })
            .expect("create task")
    }

    fn rule_input(pattern: &str, matcher: crate::matching::MatcherType) -> TaskMatchRuleInput {
        TaskMatchRuleInput {
            field: crate::matching::MatchField::Title,
            matcher,
            pattern: pattern.into(),
            case_sensitive: false,
            enabled: true,
        }
    }

    #[test]
    fn manual_link_is_idempotent_and_listable() {
        let (_dir, store) = temp_store();
        let event = seed_event(&store, "e1", "fix [PROJECT-A] crash");
        let task = seed_task(&store, "Project A work");

        let link = store.link_activity_to_task(&event.id, task.id).unwrap();
        assert_eq!(link.origin, LinkOrigin::Manual);
        // Re-linking the same pair must not create a duplicate.
        store.link_activity_to_task(&event.id, task.id).unwrap();

        let activities = store.list_task_activities(task.id).unwrap();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].event.id, "e1");

        let tasks = store.list_activity_tasks(&event.id).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task.id);
    }

    #[test]
    fn manual_link_rejects_unknown_event_or_task() {
        let (_dir, store) = temp_store();
        let task = seed_task(&store, "t");
        assert!(store.link_activity_to_task("missing", task.id).is_err());

        let event = seed_event(&store, "e1", "x");
        assert!(store.link_activity_to_task(&event.id, 9999).is_err());
    }

    #[test]
    fn unlink_removes_the_link() {
        let (_dir, store) = temp_store();
        let event = seed_event(&store, "e1", "x");
        let task = seed_task(&store, "t");
        store.link_activity_to_task(&event.id, task.id).unwrap();

        let summary = store.unlink_activity_from_task(&event.id, task.id).unwrap();
        assert_eq!(summary.deleted_rows, 1);
        assert!(store.list_task_activities(task.id).unwrap().is_empty());
    }

    #[test]
    fn rules_auto_link_new_activities_on_ingest() {
        let (_dir, store) = temp_store();
        let task = seed_task(&store, "Project A");
        store
            .create_task_rule(task.id, rule_input("[PROJECT-A]", crate::matching::MatcherType::Contains))
            .unwrap();

        // Recorded AFTER the rule exists → auto-linked.
        let event = seed_event(&store, "e1", "fix [project-a] bug");
        seed_event(&store, "e2", "unrelated note");

        let activities = store.list_task_activities(task.id).unwrap();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].event.id, event.id);
        assert_eq!(activities[0].origin, LinkOrigin::Rule);
    }

    #[test]
    fn apply_rules_backfills_existing_activities() {
        let (_dir, store) = temp_store();
        // Activities recorded BEFORE any rule exists.
        seed_event(&store, "e1", "JIRA-100 deploy");
        seed_event(&store, "e2", "JIRA-200 review");
        seed_event(&store, "e3", "lunch");
        let task = seed_task(&store, "Tickets");
        store
            .create_task_rule(task.id, rule_input(r"JIRA-\d+", crate::matching::MatcherType::Regex))
            .unwrap();

        let summary = store.apply_task_rules(Some(task.id)).unwrap();
        assert_eq!(summary.linked, 2);
        assert_eq!(summary.scanned, 3);
        assert_eq!(summary.rules, 1);
        assert_eq!(store.list_task_activities(task.id).unwrap().len(), 2);

        // Idempotent: a second run links nothing new.
        let again = store.apply_task_rules(Some(task.id)).unwrap();
        assert_eq!(again.linked, 0);
    }

    #[test]
    fn invalid_regex_rule_is_rejected() {
        let (_dir, store) = temp_store();
        let task = seed_task(&store, "t");
        assert!(store
            .create_task_rule(task.id, rule_input("(unclosed", crate::matching::MatcherType::Regex))
            .is_err());
    }

    #[test]
    fn disabled_rule_does_not_auto_link() {
        let (_dir, store) = temp_store();
        let task = seed_task(&store, "t");
        let mut input = rule_input("match", crate::matching::MatcherType::Contains);
        input.enabled = false;
        store.create_task_rule(task.id, input).unwrap();
        seed_event(&store, "e1", "match this");
        assert!(store.list_task_activities(task.id).unwrap().is_empty());
    }

    #[test]
    fn apply_rules_without_a_task_id_runs_every_rule() {
        let (_dir, store) = temp_store();
        let alpha = seed_task(&store, "Alpha");
        let beta = seed_task(&store, "Beta");
        store
            .create_task_rule(alpha.id, rule_input("alpha", crate::matching::MatcherType::Contains))
            .unwrap();
        store
            .create_task_rule(beta.id, rule_input("beta", crate::matching::MatcherType::Contains))
            .unwrap();
        seed_event(&store, "e1", "alpha review");
        seed_event(&store, "e2", "beta deploy");
        seed_event(&store, "e3", "noise");

        // Auto-link already fired on ingest; a global apply must be idempotent.
        let summary = store.apply_task_rules(None).unwrap();
        assert_eq!(summary.linked, 0);
        assert_eq!(summary.rules, 2);
        assert_eq!(store.list_task_activities(alpha.id).unwrap().len(), 1);
        assert_eq!(store.list_task_activities(beta.id).unwrap().len(), 1);
    }

    #[test]
    fn case_sensitive_rule_distinguishes_case() {
        let (_dir, store) = temp_store();
        let task = seed_task(&store, "t");
        let mut input = rule_input("PROD", crate::matching::MatcherType::Contains);
        input.case_sensitive = true;
        store.create_task_rule(task.id, input).unwrap();
        seed_event(&store, "e1", "deploy to PROD");
        seed_event(&store, "e2", "prod is lowercase");
        let activities = store.list_task_activities(task.id).unwrap();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].event.id, "e1");
    }

    #[test]
    fn wildcard_rule_links_via_store() {
        let (_dir, store) = temp_store();
        let task = seed_task(&store, "t");
        store
            .create_task_rule(
                task.id,
                TaskMatchRuleInput {
                    field: crate::matching::MatchField::Url,
                    matcher: crate::matching::MatcherType::Wildcard,
                    pattern: "*github.com*".into(),
                    case_sensitive: false,
                    enabled: true,
                },
            )
            .unwrap();
        store
            .record_source_event(SourceEventInput {
                id: Some("e1".into()),
                source: "browser".into(),
                event_type: "browser".into(),
                app: Some("Chrome".into()),
                title: Some("PR".into()),
                url: Some("https://github.com/acme/repo".into()),
                workspace_key: None,
                started_at: Some(1),
                ended_at: Some(2),
                sensitivity: None,
                metadata_json: None,
            })
            .unwrap();
        assert_eq!(store.list_task_activities(task.id).unwrap().len(), 1);
    }

    #[test]
    fn updating_a_rule_changes_which_activities_match() {
        let (_dir, store) = temp_store();
        seed_event(&store, "e1", "alpha task");
        seed_event(&store, "e2", "beta task");
        let task = seed_task(&store, "t");
        let rule = store
            .create_task_rule(task.id, rule_input("alpha", crate::matching::MatcherType::Contains))
            .unwrap();
        store.apply_task_rules(Some(task.id)).unwrap();
        assert_eq!(store.list_task_activities(task.id).unwrap().len(), 1);

        // Re-point the rule at the other activity; applying picks it up too.
        store
            .update_task_rule(rule.id, rule_input("beta", crate::matching::MatcherType::Contains))
            .unwrap();
        let summary = store.apply_task_rules(Some(task.id)).unwrap();
        assert_eq!(summary.linked, 1);
        assert_eq!(store.list_task_activities(task.id).unwrap().len(), 2);
    }

    #[test]
    fn search_recent_activities_filters_by_substring() {
        let (_dir, store) = temp_store();
        seed_event(&store, "e1", "Acme planning sync");
        seed_event(&store, "e2", "lunch break");

        let all = store.search_recent_activities(None, 25).unwrap();
        assert_eq!(all.len(), 2);

        let filtered = store
            .search_recent_activities(Some("acme".into()), 25)
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "e1");

        // Blank query is treated as no filter.
        assert_eq!(store.search_recent_activities(Some("  ".into()), 25).unwrap().len(), 2);
    }

    #[test]
    fn pruning_a_source_event_cascades_its_links() {
        let (_dir, store) = temp_store();
        let task = seed_task(&store, "t");
        // Record an old event (ended_at well in the past) and link it.
        store
            .record_source_event(SourceEventInput {
                id: Some("old".into()),
                source: "window".into(),
                event_type: "window".into(),
                app: Some("Editor".into()),
                title: Some("ancient work".into()),
                url: None,
                workspace_key: None,
                started_at: Some(1_000),
                ended_at: Some(2_000),
                sensitivity: None,
                metadata_json: None,
            })
            .unwrap();
        store.link_activity_to_task("old", task.id).unwrap();
        assert_eq!(store.list_task_activities(task.id).unwrap().len(), 1);

        // Retention pruning deletes the source event; its link must go too.
        store.prune_captured_data_older_than_days(1).unwrap();
        assert!(store.list_task_activities(task.id).unwrap().is_empty());
        // The task itself survives.
        assert!(store.list_tasks(None).unwrap().iter().any(|t| t.id == task.id));
    }

    #[test]
    fn deleting_a_task_cascades_its_links_and_rules() {
        let (_dir, store) = temp_store();
        let task = seed_task(&store, "t");
        store
            .create_task_rule(task.id, rule_input("x", crate::matching::MatcherType::Contains))
            .unwrap();
        let event = seed_event(&store, "e1", "x marks");
        // Auto-linked via the rule.
        assert_eq!(store.list_task_activities(task.id).unwrap().len(), 1);

        store.delete_task(task.id).unwrap();
        assert!(store.list_task_rules(task.id).unwrap().is_empty());
        assert!(store.list_activity_tasks(&event.id).unwrap().is_empty());
    }
}
