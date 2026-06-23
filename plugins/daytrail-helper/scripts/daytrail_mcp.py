#!/usr/bin/env python3
"""Read-only MCP server for the local DayTrail SQLite database."""

from __future__ import annotations

import os
import sqlite3
import sys
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from mcp.server.fastmcp import FastMCP


APP_ID = "ai.daytrail.desktop"
DB_NAME = "daytrail.sqlite3"

mcp = FastMCP(
    "daytrail",
    instructions=(
        "Read-only tools for querying the local DayTrail database: activity, "
        "AI usage, tasks, reports, and proactive insights."
    ),
)


def _candidate_db_paths() -> list[Path]:
    env_path = os.environ.get("DAYTRAIL_DB_PATH")
    home = Path.home()
    paths: list[Path] = []
    if env_path:
        paths.append(Path(env_path).expanduser())
    paths.extend(
        [
            home / "Library" / "Application Support" / APP_ID / DB_NAME,
            home / "AppData" / "Roaming" / APP_ID / DB_NAME,
            home / ".local" / "share" / APP_ID / DB_NAME,
        ]
    )
    return paths


def _db_path() -> Path:
    for path in _candidate_db_paths():
        if path.exists():
            return path
    return _candidate_db_paths()[0]


def _connect() -> sqlite3.Connection:
    path = _db_path()
    if not path.exists():
        raise FileNotFoundError(
            f"DayTrail database not found. Checked: {', '.join(str(p) for p in _candidate_db_paths())}"
        )
    uri = f"file:{path}?mode=ro&cache=shared"
    conn = sqlite3.connect(uri, uri=True, timeout=2)
    conn.row_factory = sqlite3.Row
    return conn


def _table_exists(conn: sqlite3.Connection, table: str) -> bool:
    row = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?",
        (table,),
    ).fetchone()
    return row is not None


def _rows(conn: sqlite3.Connection, sql: str, params: tuple[Any, ...] = ()) -> list[dict[str, Any]]:
    return [dict(row) for row in conn.execute(sql, params).fetchall()]


def _ms_to_iso(value: Any) -> str | None:
    if value is None:
        return None
    try:
        return datetime.fromtimestamp(int(value) / 1000).isoformat(timespec="seconds")
    except Exception:
        return None


def _iso_to_day_bounds(day: str | None) -> tuple[int, int, str]:
    if day:
        start = datetime.strptime(day, "%Y-%m-%d")
    else:
        now = datetime.now()
        start = datetime(now.year, now.month, now.day)
    end = start + timedelta(days=1)
    return int(start.timestamp() * 1000), int(end.timestamp() * 1000), start.strftime("%Y-%m-%d")


def _duration_label(ms: int | None) -> str:
    total = max(0, int(ms or 0) // 1000)
    hours, rem = divmod(total, 3600)
    minutes, seconds = divmod(rem, 60)
    if hours:
        return f"{hours}h {minutes}m"
    if minutes:
        return f"{minutes}m {seconds}s"
    return f"{seconds}s"


def _limit(value: int, default: int = 25, max_value: int = 100) -> int:
    try:
        parsed = int(value)
    except Exception:
        parsed = default
    return min(max(parsed, 1), max_value)


@mcp.tool()
def daytrail_database_status() -> dict[str, Any]:
    """Return DayTrail database path, size, table availability, and latest capture time."""
    path = _db_path()
    status: dict[str, Any] = {
        "path": str(path),
        "exists": path.exists(),
        "size_bytes": path.stat().st_size if path.exists() else 0,
        "read_only": True,
    }
    if not path.exists():
        status["checked_paths"] = [str(p) for p in _candidate_db_paths()]
        return status

    with _connect() as conn:
        tables = [
            "source_events",
            "work_sessions",
            "tasks",
            "ai_usage",
            "reports",
            "proactive_insights",
        ]
        status["tables"] = {
            table: (
                conn.execute(f"SELECT COUNT(*) AS count FROM {table}").fetchone()["count"]
                if _table_exists(conn, table)
                else None
            )
            for table in tables
        }
        if _table_exists(conn, "source_events"):
            latest = conn.execute("SELECT MAX(ended_at) AS latest FROM source_events").fetchone()["latest"]
            status["latest_activity_at"] = _ms_to_iso(latest)
    return status


@mcp.tool()
def daytrail_today_summary(day: str | None = None, top_n: int = 8) -> dict[str, Any]:
    """Summarize DayTrail activity for a date in YYYY-MM-DD format, defaulting to today."""
    start_ms, end_ms, label = _iso_to_day_bounds(day)
    top_n = _limit(top_n, default=8, max_value=20)
    with _connect() as conn:
        summary: dict[str, Any] = {"date": label, "read_only": True}

        if _table_exists(conn, "source_events"):
            total = conn.execute(
                """
                SELECT COALESCE(SUM(duration_ms), 0) AS total_ms, COUNT(*) AS count
                FROM source_events
                WHERE started_at >= ? AND started_at < ?
                """,
                (start_ms, end_ms),
            ).fetchone()
            summary["activity"] = {
                "total_ms": total["total_ms"],
                "total": _duration_label(total["total_ms"]),
                "event_count": total["count"],
                "top_apps": _rows(
                    conn,
                    """
                    SELECT COALESCE(app, source, 'Unknown') AS app,
                           COALESCE(SUM(duration_ms), 0) AS duration_ms,
                           COUNT(*) AS events
                    FROM source_events
                    WHERE started_at >= ? AND started_at < ?
                    GROUP BY COALESCE(app, source, 'Unknown')
                    ORDER BY duration_ms DESC
                    LIMIT ?
                    """,
                    (start_ms, end_ms, top_n),
                ),
                "top_contexts": _rows(
                    conn,
                    """
                    SELECT COALESCE(domain, workspace_key, title, app, source, 'Unknown') AS context,
                           COALESCE(app, source, 'Unknown') AS app,
                           COALESCE(SUM(duration_ms), 0) AS duration_ms,
                           COUNT(*) AS events
                    FROM source_events
                    WHERE started_at >= ? AND started_at < ?
                    GROUP BY COALESCE(domain, workspace_key, title, app, source, 'Unknown'), COALESCE(app, source, 'Unknown')
                    ORDER BY duration_ms DESC
                    LIMIT ?
                    """,
                    (start_ms, end_ms, top_n),
                ),
            }
            for row in summary["activity"]["top_apps"]:
                row["duration"] = _duration_label(row["duration_ms"])
            for row in summary["activity"]["top_contexts"]:
                row["duration"] = _duration_label(row["duration_ms"])

        if _table_exists(conn, "ai_usage"):
            ai_rows = _rows(
                conn,
                """
                SELECT COALESCE(tool_name, provider, 'AI tool') AS tool,
                       COALESCE(SUM(duration_ms), 0) AS duration_ms,
                       COUNT(*) AS events
                FROM ai_usage
                WHERE COALESCE(started_at, created_at) >= ? AND COALESCE(started_at, created_at) < ?
                GROUP BY COALESCE(tool_name, provider, 'AI tool')
                ORDER BY duration_ms DESC
                LIMIT ?
                """,
                (start_ms, end_ms, top_n),
            )
            for row in ai_rows:
                row["duration"] = _duration_label(row["duration_ms"])
            summary["ai_usage"] = ai_rows

        if _table_exists(conn, "tasks"):
            summary["open_tasks"] = _rows(
                conn,
                """
                SELECT id, title, notes, priority, due_date, due_at, client_label, project_label
                FROM tasks
                WHERE status = 'open'
                ORDER BY COALESCE(due_at, 9223372036854775807), id DESC
                LIMIT ?
                """,
                (top_n,),
            )
            for task in summary["open_tasks"]:
                task["due_at_iso"] = _ms_to_iso(task.get("due_at"))

        if _table_exists(conn, "proactive_insights"):
            summary["recent_insights"] = _rows(
                conn,
                """
                SELECT title, body, priority, generated_at
                FROM proactive_insights
                WHERE dismissed_at IS NULL
                ORDER BY generated_at DESC
                LIMIT ?
                """,
                (min(top_n, 5),),
            )
            for insight in summary["recent_insights"]:
                insight["generated_at_iso"] = _ms_to_iso(insight.get("generated_at"))

    return summary


@mcp.tool()
def daytrail_search_activity(query: str, day: str | None = None, limit: int = 25) -> dict[str, Any]:
    """Search captured activity titles, apps, domains, and workspaces."""
    if not query.strip():
        return {"query": query, "matches": [], "error": "query is required"}
    start_ms, end_ms, label = _iso_to_day_bounds(day)
    limit = _limit(limit)
    needle = f"%{query.strip()}%"
    with _connect() as conn:
        matches = _rows(
            conn,
            """
            SELECT id, app, source, title, domain, workspace_key, started_at, ended_at, duration_ms
            FROM source_events
            WHERE started_at >= ? AND started_at < ?
              AND (
                title LIKE ? OR app LIKE ? OR source LIKE ? OR domain LIKE ? OR workspace_key LIKE ?
              )
            ORDER BY started_at DESC
            LIMIT ?
            """,
            (start_ms, end_ms, needle, needle, needle, needle, needle, limit),
        )
    for row in matches:
        row["started_at_iso"] = _ms_to_iso(row.get("started_at"))
        row["ended_at_iso"] = _ms_to_iso(row.get("ended_at"))
        row["duration"] = _duration_label(row.get("duration_ms"))
    return {"query": query, "date": label, "matches": matches}


@mcp.tool()
def daytrail_recent_activity(hours: int = 4, limit: int = 40) -> dict[str, Any]:
    """Return recent captured activity events from the last N hours."""
    hours = min(max(int(hours or 4), 1), 168)
    limit = _limit(limit, default=40, max_value=100)
    since_ms = int((datetime.now() - timedelta(hours=hours)).timestamp() * 1000)
    with _connect() as conn:
        events = _rows(
            conn,
            """
            SELECT id, app, source, title, domain, workspace_key, started_at, ended_at, duration_ms
            FROM source_events
            WHERE ended_at >= ?
            ORDER BY started_at DESC
            LIMIT ?
            """,
            (since_ms, limit),
        )
    for row in events:
        row["started_at_iso"] = _ms_to_iso(row.get("started_at"))
        row["ended_at_iso"] = _ms_to_iso(row.get("ended_at"))
        row["duration"] = _duration_label(row.get("duration_ms"))
    return {"hours": hours, "events": events}


@mcp.tool()
def daytrail_open_tasks(limit: int = 50, include_done: bool = False) -> dict[str, Any]:
    """List DayTrail tasks, defaulting to open tasks only."""
    limit = _limit(limit, default=50, max_value=100)
    with _connect() as conn:
        if include_done:
            sql = """
                SELECT id, title, status, notes, priority, due_date, due_at, client_label, project_label, source
                FROM tasks
                ORDER BY CASE status WHEN 'open' THEN 0 ELSE 1 END,
                         COALESCE(due_at, 9223372036854775807),
                         id DESC
                LIMIT ?
            """
            params = (limit,)
        else:
            sql = """
                SELECT id, title, status, notes, priority, due_date, due_at, client_label, project_label, source
                FROM tasks
                WHERE status = 'open'
                ORDER BY COALESCE(due_at, 9223372036854775807), id DESC
                LIMIT ?
            """
            params = (limit,)
        tasks = _rows(conn, sql, params)
    for task in tasks:
        task["due_at_iso"] = _ms_to_iso(task.get("due_at"))
    return {"include_done": include_done, "tasks": tasks}


@mcp.tool()
def daytrail_recent_reports(limit: int = 5) -> dict[str, Any]:
    """Return recent generated DayTrail reports."""
    limit = _limit(limit, default=5, max_value=20)
    with _connect() as conn:
        if not _table_exists(conn, "reports"):
            return {"reports": []}
        reports = _rows(
            conn,
            """
            SELECT id, report_type, title,
                   COALESCE(content_markdown, body_markdown) AS markdown,
                   COALESCE(updated_at, created_at, generated_at) AS timestamp
            FROM reports
            ORDER BY COALESCE(updated_at, created_at, generated_at) DESC
            LIMIT ?
            """,
            (limit,),
        )
    for report in reports:
        report["timestamp_iso"] = _ms_to_iso(report.get("timestamp"))
    return {"reports": reports}


if __name__ == "__main__":
    try:
        mcp.run()
    except Exception as exc:
        print(f"DayTrail MCP server failed: {exc}", file=sys.stderr)
        raise
