# DayTrail

Local-first work memory for the modern desktop.

DayTrail records lightweight metadata from your apps, browser tabs, editors,
terminal sessions, and AI tools so you can reconstruct the day without running
a timer or writing status notes from memory.

Use it to answer:

- What did I work on today?
- Which app, project, site, file, or chat took time?
- Where did AI tools help?
- What should go into my daily update, client note, or review summary?
- Is capture healthy, or did a permission / integration break?

> Status: pre-1.0. macOS has been exercised manually. Windows installers build
> in CI and pass automated checks, but a real Windows smoke test is still
> required before a signed public release.

[![Windows Build](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/windows-release.yml/badge.svg)](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/windows-release.yml)
[![macOS Build](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/macos-release.yml/badge.svg)](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/macos-release.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Support on Ko-fi](https://img.shields.io/badge/Support-Ko--fi-ff5e5b?logo=ko-fi&logoColor=white)](https://ko-fi.com/iamsai)

## Why Try It

DayTrail is for people whose work moves across too many places to remember
cleanly at the end of the day: IDEs, browser tabs, terminals, Slack, meetings,
AI chats, issue trackers, documents, and internal tools.

Instead of asking you to manually start and stop timers, DayTrail builds a local
activity trail from system metadata. You can inspect a 24-hour timeline, drill
into an hour, tag meetings or offline work, review AI usage, and generate a
source-backed daily report.

The goal is not surveillance. The goal is a private work memory that helps you
write better updates, recover context faster, and notice when important work was
missed.

## See it in action

### Today — your whole day, reconstructed automatically

![DayTrail Today dashboard: current work is "Claude · Deep dive into project codebase", with daily stats and a 24-hour timeline](docs/screenshots/01-today.png)

One glance answers "what did I do today?": what you're on right now (with the real
document/chat title, not just the app name), time tracked, active apps, AI time,
and a 24-hour timeline you can drill into hour by hour.

### AI Impact — how much of your work actually flows through AI

![AI Impact view showing per-tool usage: ChatGPT, Claude, and Claude Code with durations](docs/screenshots/02-ai-impact.png)

DayTrail treats AI tools as first-class work and measures them — which tools
(ChatGPT, Claude, Codex, Copilot, Cursor…), for how long, and on which projects.

### Activity — sessions, apps, and projects

![Activity view showing a work session broken down by app and AI tool](docs/screenshots/03-activity.png)

Every work session is broken down into the apps, projects, and AI tools behind
it — so a single block of time tells the whole story.

### Capture Health — it tells you when it breaks

![Capture Health settings showing all capture sources green and Accessibility granted](docs/screenshots/06-capture-health.png)

Most trackers fail silently and you lose a day before noticing. DayTrail watches
its own capture engine and every source — if a permission is revoked or capture
stalls, it shows you exactly what's wrong and how to fix it.

> Screenshots use a sample project name; DayTrail keeps all real data on your machine.

## What It Captures

DayTrail is metadata-first. By default, it focuses on enough context to explain
work without storing private content unnecessarily.

- Apps: foreground app, window title, active duration.
- Browsers: supported browser tab title, domain, and redacted URL when the
  browser allows automation.
- Editors: project / workspace, active file metadata, and folder context from
  editor integrations.
- Terminals: shell working directory and commands when shell integration is
  installed.
- AI tools: detected AI apps, browser tools, editor assistants, and terminal
  agents.
- Manual context: meetings, offline work, client / project / task labels, and
  billable flags.

## What It Avoids

- Screenshots are off by default.
- Clipboard content is not stored.
- Browser URLs are redacted before storage where possible.
- API keys should be stored in the OS keychain / credential store, not committed
  to the repo.
- Generated reports are based on captured local facts and user-approved AI
  provider settings.

See [PRIVACY.md](PRIVACY.md) for the full privacy model.

## How It Works

1. The desktop app samples active work metadata.
2. Browser, editor, and terminal bridges add deeper context where installed.
3. DayTrail groups events into sessions, hours, apps, projects, sites, and AI
   usage.
4. You can tag gaps, meetings, or current work context manually.
5. Reports summarize the captured facts and keep the source trail visible.

## Setup For A Real Trial

1. Install the desktop app.
2. Grant macOS Accessibility permission or the Windows equivalent when prompted.
3. Enable browser automation / extension support if you want tab titles and
   domains.
4. Install editor and terminal integrations if you want project, file, folder,
   and command context.
5. Add an optional AI provider in Settings if you want generated reports.
6. Leave DayTrail running from startup so the day is captured automatically.

DayTrail is most useful after one full workday of capture.

## Platform Status

| Platform | Status |
| --- | --- |
| macOS | Primary development target. Manual install, permissions, tray, capture, and reporting flows have been exercised. |
| Windows | Backend support, startup registration, credential storage, terminal bridge scripts, and installer builds are implemented. CI produces NSIS and MSI installers. Real Windows smoke testing is still required before production publishing. |
| Linux | Not a release target yet. Some Tauri pieces may work, but capture behavior is not validated. |

## Repository Layout

```text
apps/desktop/              Tauri desktop app, Rust capture backend, React UI
apps/browser-extension/    Browser extension for tab context
apps/vscode-extension/     VS Code / editor bridge
scripts/                   Build, release, bridge, and verification scripts
docs/                      Supporting docs and screenshot assets
```

## Development

Requirements:

- Node.js 20+
- npm
- Rust stable
- Tauri platform prerequisites for your OS

Install desktop dependencies:

```bash
npm ci --prefix apps/desktop
```

Run the desktop app in development:

```bash
cd apps/desktop
npm run tauri dev
```

Run the main quality gate:

```bash
npm run release:check
```

Useful targeted checks:

```bash
npm run desktop:check
npm run desktop:test
npm run browser-extension:check
npm run vscode-extension:check
npm run test:scripts
```

## Build Installers

macOS unsigned local build:

```bash
npm run desktop:dmg
```

Windows installer build from Windows:

```powershell
npm run desktop:windows
```

The Windows CI workflow also builds installer artifacts. See [RELEASE.md](RELEASE.md)
for the full release checklist, signing notes, and manual verification steps.

## Documentation

- [PRIVACY.md](PRIVACY.md) explains local storage, metadata capture, redaction,
  exports, and AI provider behavior.
- [SECURITY.md](SECURITY.md) explains how to report security issues.
- [RELEASE.md](RELEASE.md) lists the release verification checklist.
- [docs/screenshots/README.md](docs/screenshots/README.md) defines the public
  screenshot set used by this README.
- `docs/functional-requirements.txt`, `docs/technical-requirements.txt`, and
  `docs/design-guide.txt` are product targets and backlog references, not a
  guarantee that every listed item has shipped.

## Support DayTrail

DayTrail is free and open source, built by one developer in the open. If it
helps you remember your days — or you just want to see it get better — you can
support development here:

[![Buy me a coffee on Ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/iamsai)

Stars, issues, and pull requests are just as welcome.

## License

Licensed under MIT OR Apache-2.0. See [LICENSE](LICENSE),
[LICENSE-MIT](LICENSE-MIT), and [LICENSE-APACHE](LICENSE-APACHE).
