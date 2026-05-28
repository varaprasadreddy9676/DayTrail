use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::WorkMemorySummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializationFingerprint {
    pub source_event_count: usize,
    pub max_source_ended_at: i64,
    pub max_source_created_at: i64,
    pub total_source_duration_ms: i64,
    pub source_content_signature: i64,
    pub idle_gap_ms: i64,
}

pub fn source_event_materialization_fingerprint_locked(
    conn: &Connection,
    idle_gap_ms: i64,
) -> Result<MaterializationFingerprint> {
    conn.query_row(
        r#"
        SELECT
            COUNT(*),
            COALESCE(MAX(ended_at), 0),
            COALESCE(MAX(created_at), 0),
            COALESCE(SUM(duration_ms), 0),
            COALESCE(SUM(
                LENGTH(id)
                + LENGTH(source)
                + LENGTH(event_type)
                + LENGTH(COALESCE(app, ''))
                + LENGTH(COALESCE(title, ''))
                + LENGTH(COALESCE(domain, ''))
                + LENGTH(COALESCE(workspace_key, ''))
                + LENGTH(COALESCE(metadata_json, ''))
            ), 0)
        FROM source_events
        "#,
        [],
        |row| {
            Ok(MaterializationFingerprint {
                source_event_count: row.get::<_, i64>(0)? as usize,
                max_source_ended_at: row.get(1)?,
                max_source_created_at: row.get(2)?,
                total_source_duration_ms: row.get(3)?,
                source_content_signature: row.get(4)?,
                idle_gap_ms,
            })
        },
    )
    .map_err(Into::into)
}

pub fn materialization_state_locked(
    conn: &Connection,
) -> Result<Option<MaterializationFingerprint>> {
    conn.query_row(
        r#"
        SELECT source_event_count, max_source_ended_at, max_source_created_at,
               total_source_duration_ms, source_content_signature, idle_gap_ms
        FROM materialization_state
        WHERE id = 1
        "#,
        [],
        |row| {
            Ok(MaterializationFingerprint {
                source_event_count: row.get::<_, i64>(0)? as usize,
                max_source_ended_at: row.get(1)?,
                max_source_created_at: row.get(2)?,
                total_source_duration_ms: row.get(3)?,
                source_content_signature: row.get(4)?,
                idle_gap_ms: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn upsert_materialization_state_locked(
    conn: &Connection,
    fingerprint: &MaterializationFingerprint,
    updated_at: i64,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO materialization_state
            (id, source_event_count, max_source_ended_at, max_source_created_at,
             total_source_duration_ms, source_content_signature, idle_gap_ms, updated_at)
        VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(id) DO UPDATE SET
            source_event_count = excluded.source_event_count,
            max_source_ended_at = excluded.max_source_ended_at,
            max_source_created_at = excluded.max_source_created_at,
            total_source_duration_ms = excluded.total_source_duration_ms,
            source_content_signature = excluded.source_content_signature,
            idle_gap_ms = excluded.idle_gap_ms,
            updated_at = excluded.updated_at
        "#,
        params![
            fingerprint.source_event_count as i64,
            fingerprint.max_source_ended_at,
            fingerprint.max_source_created_at,
            fingerprint.total_source_duration_ms,
            fingerprint.source_content_signature,
            fingerprint.idle_gap_ms,
            updated_at,
        ],
    )?;
    Ok(())
}

pub fn session_graph_edge_count_locked(conn: &Connection) -> Result<usize> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM work_graph_edges WHERE relation = 'session_contains_event'",
        [],
        |row| row.get::<_, i64>(0),
    )? as usize)
}

pub fn work_memory_summary_locked(
    conn: &Connection,
    source_event_count: usize,
) -> Result<WorkMemorySummary> {
    let work_sessions = conn.query_row("SELECT COUNT(*) FROM work_sessions", [], |row| {
        row.get::<_, i64>(0)
    })? as usize;
    let parallel_streams = conn.query_row("SELECT COUNT(*) FROM parallel_streams", [], |row| {
        row.get::<_, i64>(0)
    })? as usize;
    let graph_edges = session_graph_edge_count_locked(conn)?;
    Ok(WorkMemorySummary {
        source_events: source_event_count,
        work_sessions,
        parallel_streams,
        graph_edges,
    })
}
