---
name: daytrail-helper
description: Use when the user asks about DayTrail data, activity history, tasks, AI usage, reports, work sessions, or what they worked on. This plugin reads the local DayTrail SQLite database through read-only MCP tools.
---

# DayTrail Helper

Use the DayTrail MCP tools when the user asks about their captured workday, open tasks, reports, AI tool usage, or activity history.

## Data Boundary

- The plugin reads the local DayTrail database only.
- It does not write, edit, complete, snooze, or delete DayTrail data.
- If the DayTrail database is not found, ask the user to open DayTrail once or provide `DAYTRAIL_DB_PATH`.
- Prefer summary tools before raw search tools unless the user asks for specific evidence.

## Suggested Tool Flow

1. Use `daytrail_database_status` to confirm the database path and available tables when troubleshooting.
2. Use `daytrail_today_summary` for “what did I do today?” and daily recap questions.
3. Use `daytrail_search_activity` for specific apps, domains, project names, files, or chat/title fragments.
4. Use `daytrail_open_tasks` for backlog and reminder questions.
5. Use `daytrail_recent_reports` when the user asks for generated DayTrail reports.

## Privacy

Treat returned activity titles, domains, paths, and task names as private user data. Summarize only what is relevant to the user’s request.
