# DayTrail

Local-first work memory for the modern desktop.

You sit down at 6pm sure you coded all day — but where did the time *actually*
go? DayTrail quietly records lightweight metadata from your apps, browser tabs,
editors, terminals, and AI tools, then shows you the honest answer. No timers to
start, no notes to write, and nothing ever leaves your machine.

It helps you answer questions like:

- **Where did my day really go?** You'll see the 90 minutes in email and the
  hour on YouTube you'd have sworn was deep work.
- **Can I trust my memory?** Compare what you remember with the captured
  timeline, apps, projects, and AI trail.
- **What's eating my time?** Spot recurring distractions and time-sinks so you
  can actually cut them.
- **Can I stay focused right now?** Start a focus block and get a gentle native
  nudge if you drift into WhatsApp, YouTube, Reddit, or other distractions;
  timer sessions are saved so you can review focus drift later.
- **Am I working sustainably?** Optional Smart Breaks notice sustained input and
  send blink, posture, and break reminders without adding another dashboard card.
- **Can I recover after an interruption?** Replay your day and jump back to the
  app/project/context you were in before the break.
- **How much of my work runs through AI** (ChatGPT, Claude, Codex, Copilot…) —
  and on which projects?
- **What did I do this week** for my standup, client update, or OSS changelog?
  Generate a source-backed weekly digest, with optional AI drafting.
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

## People Are Already Trying DayTrail

<p align="center">
  <a href="https://github.com/varaprasadreddy9676/DayTrail/releases">
    <img alt="Total downloads" src="https://img.shields.io/github/downloads/varaprasadreddy9676/DayTrail/total?label=Total%20Downloads&style=for-the-badge&color=2ea44f&logo=github&cacheSeconds=300">
  </a>
  <a href="https://github.com/varaprasadreddy9676/DayTrail/releases/latest">
    <img alt="Latest release downloads" src="https://img.shields.io/github/downloads/varaprasadreddy9676/DayTrail/latest/total?label=latest%20release&style=for-the-badge&color=0969da&logo=github">
  </a>
</p>

## Lightweight By Design

DayTrail is built with Tauri + Rust, so it does not ship a bundled Chromium
runtime like Electron apps. Current release artifacts are tiny for a desktop
work tracker: the macOS Apple Silicon DMG is about **9.8 MB**, and Windows CI
produces installers under **6 MB** (`.exe` about **4.5 MB**, `.msi` about
**5.9 MB** in the latest measured build).

## Download

**macOS (Apple Silicon):** grab the latest `.dmg` from the
[**Releases page**](https://github.com/varaprasadreddy9676/DayTrail/releases/latest).

> ⚠️ The build is **not notarized** (no paid Apple Developer ID yet), so macOS
> tags the download and shows **"DayTrail.app is damaged and can't be opened."**
> It isn't damaged — that's Gatekeeper. Clear it once:
>
> 1. Open the DMG and drag **DayTrail** into **Applications**.
> 2. Open **Terminal** and run:
>    ```bash
>    xattr -dr com.apple.quarantine /Applications/DayTrail.app
>    ```
> 3. Open DayTrail normally, then grant **Accessibility** and **Allow notifications**.
>
> (Right-click → Open sometimes works, but the `xattr` command is the reliable
> fix for the "damaged" message.)

**Windows:** download the `.msi`/`.exe` installer from the same Releases page
(built by CI). **Other Macs / build from source:** see [Try it](#try-it-build-from-source) below.

## Troubleshooting

<details open>
<summary><b>macOS: "DayTrail.app is damaged and can't be opened. You should move it to the Trash."</b></summary>

It is **not** damaged. Because the app isn't notarized (no paid Apple Developer
ID), macOS quarantines the download and shows this scary message. Fix it once:

```bash
xattr -dr com.apple.quarantine /Applications/DayTrail.app
```

Then open DayTrail normally. (Drag it into **Applications** first if you haven't.
If you downloaded with a non-Safari browser, this is the expected behavior.)
</details>

<details>
<summary><b>Windows: "Windows protected your PC" (SmartScreen)</b></summary>

The installer isn't code-signed yet, so SmartScreen warns on first run. Click
**More info → Run anyway**.
</details>

<details>
<summary><b>macOS: capture stopped / titles show only the app name</b></summary>

DayTrail needs **Accessibility** permission to read app/window titles. Open
**Settings → Capture Health** in the app — if Accessibility shows as missing,
use **Fix accessibility** to re-grant it (a macOS update can reset this).
</details>

## Who it's for

DayTrail is for people whose work sprawls across too many places to remember —
IDEs, browser tabs, terminals, Slack, meetings, AI chats, issue trackers, docs,
and internal tools — and who suspect their time isn't going where they think.
It is especially useful for interruption-heavy or ADHD-style workdays where the
hard part is not only tracking time, but recovering context.

**A real example (the kind of person who built this):** a developer with a day
job *and* open-source side projects, losing track of where the hours go. After a
day with DayTrail it's obvious — "I thought I shipped features all morning, but
90 minutes went to email and an hour to YouTube." Now you can:

- **Cut the time-sinks** you didn't realize were so big (YouTube, inbox, doom-scrolling).
- **Catch drift while it is happening** with Focus Mode nudges instead of only
  discovering the damage at the end of the day.
- **Keep long work runs humane** with optional Smart Breaks for blink checks,
  posture resets, and short pauses after sustained keyboard or mouse input.
- **Recover context after interruptions** with replay/restore views and
  unclassified away-time prompts.
- **See enough context to resume quickly** without rebuilding the whole day from
  memory.
- **Separate day-job work from side-project work** without two timers.
- **Find the routines** you repeat every day and decide what to automate or drop.
- **Back your standup / invoice / changelog / weekly update with facts**, not
  memory.

Instead of starting and stopping timers, DayTrail builds the trail automatically
from system metadata: a timeline you can drill into by the hour, app/project/AI
breakdowns, and source-backed reports. It's not surveillance — it's a **private,
local memory of your own work** that only you can see.

## See it in action

### Today — your whole day, reconstructed automatically

![DayTrail Today dashboard: current work is "Claude · Deep dive into project codebase", with daily stats and a 24-hour timeline](docs/screenshots/01-today.png)

One glance answers "what did I do today?": what you're on right now (with the real
document/chat title, not just the app name), time tracked, top work app, AI time,
review count, tasks, and a 24-hour timeline you can drill into hour by hour.

### Focus Mode — a gentle nudge before a distraction becomes an hour

![Focus Mode sidebar controls with focus label and duration choices](docs/screenshots/07-focus-mode.png)

Start an open-ended, 25-minute, 50-minute, or 90-minute focus block from the
sidebar. DayTrail keeps watching the same local foreground-window metadata it
already captures and sends a native notification if you spend too long on known
distractions. Persisted focus sessions can also be compared against the actual
apps/projects used, so you can see whether a block stayed on track. It reminds
you; it never blocks apps or sends data anywhere.

### Smart Breaks — blink, posture, and break reminders without UI noise

Smart Breaks are optional and live in Settings. When enabled, DayTrail watches
the same local foreground-window and OS idle signals it already uses for capture.
It only nudges after sustained keyboard or mouse activity, resets when you stop
interacting for a few minutes, and stays quiet during calls or presentation-like
contexts. Reminders are staged: a blink check, a posture reset, then a short
break notification at the interval you choose. It does not make medical claims,
capture keystrokes, block apps, or add another card to the Today screen.

### Weekly digest and replay — source-backed updates without reconstructing memory

Daily reports, weekly reviews, and replay/restore flows are generated from the
same local evidence: work sessions, AI usage, outputs, meetings, idle recovery
notes, focus sessions, and Smart Break events. With an AI provider configured,
DayTrail can turn the last seven local days into a first draft for a standup,
client update, or changelog while keeping the source trail visible.

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
- Idle recovery: away/resume gaps that need classification, plus user-approved
  notes for meetings, calls, lunch, errands, or ignored time.
- Focus Mode and focus timer: active focus label, duration choice, persisted
  focus sessions, off-task time, nudge count, and distraction nudges based on
  local foreground-window metadata.
- Smart Breaks: optional local blink, posture, and break notifications based on
  sustained input and quieted by idle/call/presentation context.

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
4. Focus sessions are reconciled against captured activity when those facts
   exist.
5. You can tag gaps, meetings, or current work context manually.
6. Focus Mode can nudge you during an active focus block when a distraction
   pattern is detected.
7. Optional Smart Breaks can nudge after sustained input with blink, posture,
   and break reminders configured in Settings.
8. Daily reports, weekly digests, and replay/restore views summarize the
   captured facts and keep the source trail visible.

## Setup For A Real Trial

1. Install the desktop app.
2. Grant macOS Accessibility permission or the Windows equivalent when prompted.
3. Enable browser automation / extension support if you want tab titles and
   domains.
4. Install editor and terminal integrations if you want project, file, folder,
   and command context.
5. Allow notifications if you want Focus Mode, Smart Breaks, and away-time
   nudges.
6. DayTrail checks for available releases on startup and can remind you again
   after 8 hours when an update is available.
7. Add an optional AI provider in Settings if you want generated report drafts
   and weekly digests.
8. Leave DayTrail running from startup so the day is captured automatically.

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

## Release Automation

Every non-release push to `main` creates a desktop release candidate. If the
commit already bumps the desktop version, GitHub Actions tags that version. If
the current version is already tagged, Actions bumps the patch version, commits
the metadata update, tags it, and dispatches the macOS and Windows release
builds.

Use `scripts/release.sh <version>` only when you need to cut a specific version
manually.

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
