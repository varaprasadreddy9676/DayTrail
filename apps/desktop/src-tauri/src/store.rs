use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Local, NaiveDate, SecondsFormat, TimeZone, Utc};
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, DatabaseName,
    OptionalExtension, Row,
};
use serde_json::Value;
use tauri::Manager;

const SOURCE_EVENT_COALESCE_GAP_MS: i64 = 2 * 60 * 1000;
const DISPLAY_APP_NAME: &str = "DayTrail";
const DATA_DIR_NAME: &str = "ai.daytrail.desktop";
const DB_FILE_NAME: &str = "daytrail.sqlite3";

use crate::{
    models::{
        ActiveWorkContext, ActiveWorkContextInput, AgentRun, AgentRunInput, AiContextUsage,
        AiContributionRow, AiOutputLedgerItem, AiToolUsage, AiUsage, AiUsageInput, AiUsageSummary,
        AppProjectUsage, AppUsage, AppUsageSummary, AutomationCandidate, BrowserBridgeEvent,
        CaptureHealthCheck, CaptureHealthSummary, Commitment, CommitmentInput,
        DatabaseTransferResult, EmailThread, EmailThreadInput, ExportPayload, ExportRangeInput,
        FieldVisit, FieldVisitInput, FileUsage, IdleBlock, IdleBlockInput, LoopAction,
        LoopActionInput, LoopRisk, Meeting, MeetingInput, MenuBarSummary, NextBestAction,
        ParallelStreamSummary, PauseState, PlanningItem, PlanningOutput, PrivacyDeleteSummary,
        ProjectContext, QuickNote, ReportOutput, ReturnMarker, ReviewSessionInput, ScratchpadNote,
        ScratchpadNoteInput, SearchResult, Settings, SettingsConfigPayload, SettingsPatch,
        SourceEvent, SourceEventInput, StateSnapshot, StateSnapshotInput, StorageLocationInfo,
        Task, TaskInput, TaskStatus, TerminalBridgeMetadata, TimesheetRow, TodaySnapshot,
        UnclosedLoopItem, WorkMemorySummary, WorkOutput, WorkOutputInput, WorkSessionSummary,
        WorkspaceContext,
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
                source TEXT,
                project_path TEXT,
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
            "#,
        )?;

        Self::migrate_tasks_schema(&conn)?;
        Self::migrate_quick_notes_schema(&conn)?;
        Self::migrate_legacy_compatible_columns(&conn)?;
        Self::migrate_url_redactions(&conn)?;

        conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_tasks_status_due_date
                ON tasks(status, due_date);
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

        let now = now_utc();
        let conn = self.lock()?;
        conn.execute(
            r#"
            INSERT INTO tasks (title, status, due_date, source, project_path, created_at, updated_at)
            VALUES (?1, 'open', ?2, ?3, ?4, ?5, ?5)
            "#,
            params![
                title,
                input.due_date,
                input.source,
                input.project_path,
                now
            ],
        )?;
        let id = conn.last_insert_rowid();
        Self::get_task_locked(&conn, id)
    }

    pub fn list_tasks(&self, status: Option<TaskStatus>) -> Result<Vec<Task>> {
        let conn = self.lock()?;
        let mut tasks = Vec::new();
        match status {
            Some(status) => {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT id, title, status, due_date, source, project_path, created_at, updated_at
                    FROM tasks
                    WHERE status = ?1
                    ORDER BY COALESCE(due_date, '9999-12-31'), created_at DESC
                    "#,
                )?;
                for task in stmt.query_map(params![status.as_db_value()], Self::task_from_row)? {
                    tasks.push(task?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT id, title, status, due_date, source, project_path, created_at, updated_at
                    FROM tasks
                    ORDER BY COALESCE(due_date, '9999-12-31'), created_at DESC
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
            "UPDATE tasks SET status = 'done', updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Self::get_task_locked(&conn, id)
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
            SELECT id, title, status, due_date, source, project_path, created_at, updated_at
            FROM tasks
            WHERE status = 'open' AND (due_date IS NULL OR due_date <= ?1)
            ORDER BY COALESCE(due_date, '9999-12-31'), created_at DESC
            "#,
        )?;
        let rows = stmt.query_map(params![local_date.clone()], Self::task_from_row)?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row?);
        }
        drop(stmt);
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
        let meetings = self.list_meetings(20)?;
        let field_visits = self.list_field_visits(20)?;
        let idle_blocks = self
            .list_idle_blocks_between(Some(day_start), Some(day_end), 20)?
            .into_iter()
            .filter(is_actionable_idle_block)
            .collect::<Vec<_>>();
        let source_events = self.list_today_source_events(10_000)?;
        let ai_usage = self.list_ai_usage_between(Some(day_start), Some(day_end), 1_000)?;
        let ai_usage_summary = build_ai_usage_summary(&source_events, &ai_usage, ai_outputs.len());
        let app_usage_summary = build_app_usage_summary(&source_events);
        let automation_candidates = build_automation_candidates(&source_events);
        let loop_risks = self.detect_loop_risks()?;
        let hidden_loop_ids = self.hidden_loop_ids()?;
        let pause_state = self.pause_state()?;
        let settings = self.get_settings()?;
        let next_best_action = self.next_best_action()?;
        let required_permissions_granted =
            crate::permissions::capture_permission_summary().all_required_granted;
        let capture_health = build_capture_health_with_permission_state(
            &source_events,
            &settings,
            &pause_state,
            required_permissions_granted,
        );
        let unclosed_loop_inbox = build_unclosed_loop_inbox(
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
            meetings,
            field_visits,
            idle_blocks,
            source_events,
            work_sessions,
            parallel_streams,
            ai_usage_summary,
            app_usage_summary,
            automation_candidates,
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
        let ai_usage_summary = build_ai_usage_summary(&source_events, &ai_usage, ai_outputs.len());
        let app_usage_summary = build_app_usage_summary(&source_events);
        let automation_candidates = build_automation_candidates(&source_events);
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
        let mut snapshot = self.today_snapshot()?;
        snapshot.tasks = self.list_tasks(None)?;
        let generated_at = now_utc();
        let title = format!("Weekly Work Review - {}", snapshot.local_date);
        let body_markdown = build_weekly_review_markdown(&snapshot);
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
                serde_json::to_string(&snapshot)?
            ],
        )?;

        Ok(ReportOutput {
            generated_at,
            report_type,
            title,
            body_markdown,
            used_ai: false,
            fallback_reason: Some(
                "Weekly review is generated deterministically from local evidence.".into(),
            ),
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

        for task in &snapshot.tasks {
            let item = PlanningItem {
                title: task.title.clone(),
                source: task.source.clone().unwrap_or_else(|| "task".into()),
                reason: task
                    .due_date
                    .as_ref()
                    .map(|date| format!("Due {date}"))
                    .unwrap_or_else(|| "Open task without a due date".into()),
                due_at: task.due_date.as_deref().and_then(local_date_to_epoch_ms),
                priority: if task
                    .due_date
                    .as_deref()
                    .is_some_and(|date| date_is_on_or_before(date, today))
                {
                    1
                } else {
                    3
                },
            };

            if task
                .due_date
                .as_deref()
                .is_some_and(|date| date_is_on_or_before(date, end_date))
            {
                must_close.push(item.clone());
            } else if task.due_date.is_some() {
                can_defer.push(item.clone());
            } else {
                should_progress.push(item.clone());
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
            "tasks",
            "quick_notes",
            "commitments",
            "email_threads",
            "meetings",
            "field_visits",
            "idle_blocks",
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

    pub fn apply_retention_policy(&self) -> Result<PrivacyDeleteSummary> {
        let days = self.get_settings()?.data_retention_days;
        if days <= 0 {
            return Ok(PrivacyDeleteSummary { deleted_rows: 0 });
        }
        self.prune_captured_data_older_than_days(days)
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
                reason: "Latest message is inbound and no outbound reply is recorded".to_string(),
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

        if let Some((id, title, due_date)) = conn
            .query_row(
                r#"
                SELECT id, title, due_date
                FROM tasks
                WHERE status = 'open'
                ORDER BY
                    CASE WHEN due_date IS NULL THEN 1 ELSE 0 END,
                    due_date,
                    created_at DESC
                LIMIT 1
                "#,
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?
        {
            return Ok(Some(NextBestAction {
                title,
                reason: due_date
                    .map(|date| format!("Open task is due on {date}"))
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
        if is_excluded(app_name, &settings.excluded_apps) {
            return Ok(());
        }
        if workspace_key
            .filter(|value| !value.trim().is_empty())
            .is_some_and(|value| is_project_excluded(value, &settings.excluded_projects))
        {
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
            SELECT id, title, status, due_date, source, project_path, created_at, updated_at
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
                  AND ?9 >= ended_at
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
                    reason: "Latest message is inbound and no outbound reply is recorded".into(),
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
            source: row.get(4)?,
            project_path: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
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
            let duration_ms = app_events
                .iter()
                .map(|event| event_duration_ms(event))
                .sum();

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

    AppUsageSummary {
        total_duration_ms: apps.iter().map(|app| app.duration_ms).sum(),
        apps,
    }
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
) -> CaptureHealthSummary {
    let mut checks = vec![CaptureHealthCheck {
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
    }];

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
    let status = if pause_state.paused {
        "paused"
    } else if needs_setup > 0 {
        "needs_setup"
    } else if ok_count >= 3 && !ai_waiting {
        "healthy"
    } else {
        "warming_up"
    };
    let headline = match status {
        "paused" => "Capture is paused".to_string(),
        "healthy" => "Core capture is receiving signals".to_string(),
        "needs_setup" => format!("{needs_setup} capture source(s) need setup"),
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
                source: "Reply Debt Radar".to_string(),
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
                source: "Commitment Tracker".to_string(),
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
                source: "Meeting closure".to_string(),
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

fn is_actionable_idle_block(block: &IdleBlock) -> bool {
    !block
        .evidence_json
        .as_deref()
        .is_some_and(|value| value.contains("source_event_gap"))
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

fn build_weekly_review_markdown(snapshot: &TodaySnapshot) -> String {
    let mut markdown = String::new();
    markdown.push_str(&format!(
        "# Weekly Work Review - week of {}\n\n",
        snapshot.local_date
    ));
    markdown.push_str("## Movement\n");
    markdown.push_str(&format!(
        "- {} work session(s) and {} parallel stream(s) captured.\n",
        snapshot.work_sessions.len(),
        snapshot.parallel_streams.len()
    ));
    markdown.push_str(&format!(
        "- {} AI-assisted output(s), {} open commitment(s), {} pending replie(s).\n",
        snapshot.ai_outputs.len(),
        snapshot.commitments.len(),
        snapshot.pending_replies.len()
    ));

    markdown.push_str("\n## Leadership summary\n");
    if snapshot.work_sessions.is_empty() {
        markdown.push_str("- No completed work sessions have been materialized yet.\n");
    } else {
        for session in snapshot.work_sessions.iter().take(6) {
            markdown.push_str(&format!(
                "- {}: {}\n",
                clean_report_text(&session.status),
                clean_report_text(&session.title)
            ));
        }
    }

    markdown.push_str("\n## Risks and follow-ups\n");
    if snapshot.pending_replies.is_empty() && snapshot.commitments.is_empty() {
        markdown.push_str("- No reply debt or open commitments captured.\n");
    } else {
        for reply in &snapshot.pending_replies {
            markdown.push_str(&format!(
                "- Reply debt: {}\n",
                clean_report_text(&reply.subject)
            ));
        }
        for commitment in &snapshot.commitments {
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
            let marker = if task.status == TaskStatus::Done {
                "x"
            } else {
                " "
            };
            markdown.push_str(&format!(
                "- [{}] {}\n",
                marker,
                clean_report_text(&task.title)
            ));
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
    fn capture_health_degrades_when_required_os_permissions_are_missing() {
        let summary = build_capture_health_with_permission_state(
            &[],
            &Settings::default(),
            &test_pause_state(),
            false,
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
        let summary =
            build_capture_health_with_permission_state(&[], &settings, &test_pause_state(), true);

        assert_eq!(summary.status, "warming_up");
        assert!(summary
            .checks
            .iter()
            .any(|check| check.id == "os-permissions" && check.status == "ok"));
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
}
