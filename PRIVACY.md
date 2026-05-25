# Privacy

DayTrail is designed as a local-first, metadata-first work memory app.

## Default Capture Policy

- Screenshots: off.
- Full clipboard text: off.
- Browser capture: title, domain, and redacted URL.
- Editor capture: app, workspace/project, active file metadata, and optional content hash only when enabled by the editor bridge.
- Terminal capture: current directory and redacted command metadata.
- AI capture: observed tool/provider usage and source-backed output records when detectable.

## Data Location

The desktop app stores data in a local SQLite database in the OS application data directory. Browser/editor/terminal bridges write local metadata that is ingested by the desktop app.

## AI Execution

AI analysis runs only when the user asks for reports or analysis. API keys are stored in the OS keychain where supported. Before AI execution, DayTrail applies configured redaction to context sent to the provider.

## What Not To Store

Do not use DayTrail to capture secrets, regulated personal data, passwords, tokens, private keys, or confidential content unless your organization has explicitly approved that capture policy.

## Exports

Exports may include app names, window titles, project paths, domains, redacted URLs, timestamps, notes, and AI usage evidence. Review exports before sharing them outside your organization.

