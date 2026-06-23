# DayTrail Helper Plugin

DayTrail Helper is a Codex plugin that lets Codex read your local DayTrail work history.

It is useful for questions like:

- "Summarize my DayTrail activity today."
- "Show my open DayTrail tasks."
- "Search DayTrail for Slack yesterday."
- "What AI tools did I use this week?"
- "Show my recent DayTrail reports."

## What It Does

The plugin exposes a read-only MCP server over the local DayTrail SQLite database. It can query:

- database status and capture health basics
- today's activity summary
- recent activity events
- activity search by app, title, domain, project, or workspace
- open tasks and reminders
- recent generated reports
- AI usage and proactive insights

## Privacy

This plugin does not include, upload, or sync any DayTrail data.

It only reads the DayTrail database on the machine where it is installed. All returned titles, domains, paths, reports, and task names are private local user data.

The MCP server opens SQLite in read-only mode and does not modify tasks, reports, sessions, or activity.

## Requirements

- DayTrail installed and opened at least once
- Codex with plugin support
- Python 3 with the `mcp` package available in the Codex environment

Default database locations:

```text
macOS:   ~/Library/Application Support/ai.daytrail.desktop/daytrail.sqlite3
Windows: ~/AppData/Roaming/ai.daytrail.desktop/daytrail.sqlite3
Linux:   ~/.local/share/ai.daytrail.desktop/daytrail.sqlite3
```

To use a different database path, set:

```sh
export DAYTRAIL_DB_PATH="/path/to/daytrail.sqlite3"
```

## Install From This Repository

This repository includes a Codex marketplace file:

```text
.agents/plugins/marketplace.json
```

The marketplace entry points to:

```text
plugins/daytrail-helper
```

In the Codex app, open or install the `daytrail-helper` plugin from that marketplace. Once enabled, start a new Codex thread so the plugin's MCP tools and skill are loaded.

## Included MCP Tools

- `daytrail_database_status`
- `daytrail_today_summary`
- `daytrail_recent_activity`
- `daytrail_search_activity`
- `daytrail_open_tasks`
- `daytrail_recent_reports`

## Development Check

From the repository root:

```sh
python3 -m py_compile plugins/daytrail-helper/scripts/daytrail_mcp.py
python3 /Users/sai/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py plugins/daytrail-helper
```

For non-Sai machines, replace the validation script path with the local Codex `plugin-creator` skill path.
