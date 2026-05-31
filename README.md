# DayTrail

Local-first work memory for the modern desktop.

You sit down at 6pm sure you coded all day — but where did the time *actually*
go? DayTrail quietly records lightweight metadata from your apps, browser tabs,
editors, terminals, and AI tools, then shows you the honest answer. No timers to
start, no notes to write, and nothing ever leaves your machine.

It helps you answer questions like:

- **Where did my day really go?** You'll see the 90 minutes in email and the
  hour on YouTube you'd have sworn was deep work.
- **What's eating my time?** Spot recurring distractions and time-sinks so you
  can actually cut them.
- **How much of my work runs through AI** (ChatGPT, Claude, Codex, Copilot…) —
  and on which projects?
- **What did I do this week** for my standup, client update, or OSS changelog?
- **What routines do I repeat daily** that might be worth streamlining?

Look at **today, yesterday, the last 7 days, this month, or any custom range** —
the dashboard defaults to today, but the whole history is yours to slice.

> Status: pre-1.0. macOS has been exercised manually. Windows installers build
> in CI and pass automated checks, but a real Windows smoke test is still
> required before a signed public release.

[![Windows Build](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/windows-release.yml/badge.svg)](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/windows-release.yml)
[![macOS Build](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/macos-release.yml/badge.svg)](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/macos-release.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Download for macOS](https://img.shields.io/github/v/release/varaprasadreddy9676/DayTrail?label=Download%20macOS&color=2ea44f&logo=apple)](https://github.com/varaprasadreddy9676/DayTrail/releases/latest)
[![Support on Ko-fi](https://img.shields.io/badge/Support-Ko--fi-ff5e5b?logo=ko-fi&logoColor=white)](https://ko-fi.com/iamsai)

## Download

**macOS (Apple Silicon):** grab the latest `.dmg` from the
[**Releases page**](https://github.com/varaprasadreddy9676/DayTrail/releases/latest).

> ⚠️ The build is **not notarized** (no paid Apple Developer ID yet), so macOS
> blocks a normal double-click. It's a one-time step:
>
> 1. Open the DMG, drag **DayTrail** to **Applications**.
> 2. **Right-click** DayTrail → **Open** → **Open** (or run
>    `xattr -dr com.apple.quarantine /Applications/DayTrail.app`).
> 3. Grant **Accessibility** and **Allow notifications** on first launch.

**Windows:** download the `.msi`/`.exe` installer from the same Releases page
(built by CI). **Other Macs / build from source:** see [Try it](#try-it-build-from-source) below.

## Who it's for

DayTrail is for people whose work sprawls across too many places to remember —
IDEs, browser tabs, terminals, Slack, meetings, AI chats, issue trackers, docs,
and internal tools — and who suspect their time isn't going where they think.

**A real example (the kind of person who built this):** a developer with a day
job *and* open-source side projects, losing track of where the hours go. After a
day with DayTrail it's obvious — "I thought I shipped features all morning, but
90 minutes went to email and an hour to YouTube." Now you can:

- **Cut the time-sinks** you didn't realize were so big (YouTube, inbox, doom-scrolling).
- **Separate day-job work from side-project work** without two timers.
- **Find the routines** you repeat every day and decide what to automate or drop.
- **Back your standup / invoice / changelog with facts**, not memory.

Instead of starting and stopping timers, DayTrail builds the trail automatically
from system metadata: a timeline you can drill into by the hour, app/project/AI
breakdowns, and source-backed reports. It's not surveillance — it's a **private,
local memory of your own work** that only you can see.

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
