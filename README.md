# DayTrail

> *You worked hard today. But do you actually know where the time went?*

Most of us don't. We remember the feeling of a busy day — the tabs, the switching, the "just one more thing" — but when someone asks what we shipped, or why the week felt wasted, the honest answer is a guess.

DayTrail is the app that tells you the truth about your own time. Quietly, privately, entirely on your machine — it builds a real record of your day from the apps you use, the projects you work in, the tabs you visit, the AI tools you lean on. No timers to start. No notes to write. No data leaving your computer.

At the end of the day, you'll know exactly where the hours went. And slowly, that knowledge changes everything.

[![Windows Build](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/windows-release.yml/badge.svg)](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/windows-release.yml)
[![macOS Build](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/macos-release.yml/badge.svg)](https://github.com/varaprasadreddy9676/DayTrail/actions/workflows/macos-release.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Download for macOS](https://img.shields.io/github/v/release/varaprasadreddy9676/DayTrail?label=Download%20macOS&color=2ea44f&logo=apple)](https://github.com/varaprasadreddy9676/DayTrail/releases/latest)
[![Support on Ko-fi](https://img.shields.io/badge/Support-Ko--fi-ff5e5b?logo=ko-fi&logoColor=white)](https://ko-fi.com/iamsai)

<p align="center">
  <a href="https://github.com/varaprasadreddy9676/DayTrail/releases">
    <img alt="Total downloads" src="docs/badges/total-downloads.svg">
  </a>
  <a href="https://github.com/varaprasadreddy9676/DayTrail/releases/latest">
    <img alt="Latest release downloads" src="https://img.shields.io/github/downloads/varaprasadreddy9676/DayTrail/latest/total?label=latest%20release&style=for-the-badge&color=0969da&logo=github">
  </a>
</p>

---

## Quick Start

**macOS Apple Silicon — Homebrew recommended:**

```sh
brew tap varaprasadreddy9676/tap && brew trust varaprasadreddy9676/tap && brew install --cask daytrail
```

**macOS — one-line installer, no Homebrew needed:**

```sh
curl -fsSL https://raw.githubusercontent.com/varaprasadreddy9676/DayTrail/main/scripts/install-macos.sh | bash
```

**Windows:** download the latest `.msi` or `.exe` from the [Releases page](https://github.com/varaprasadreddy9676/DayTrail/releases/latest).

If macOS or Windows warns because the app is not code-signed yet, see [Troubleshooting](#troubleshooting). DayTrail is open source, local-first, and built from this repository.

---

## The moment that made this app necessary

It's 6pm. You're tired. You're pretty sure you worked hard — you had your coffee, opened your laptop, kept things moving. But you can't really account for the day. There was a PR review, some Slack, a bug you got pulled into, a YouTube video you "just needed a minute" for. Your standup tomorrow is going to be vague. Your timesheet is going to be a reconstruction. You'll say you spent time on the project when really you spent time *near* it.

This isn't a discipline problem. **Human memory is simply not built for tracking time.** We remember events, not durations. We remember the last thing, not the whole arc. We remember effort as output — and when the output was invisible (reading, researching, getting unstuck), we forget it happened at all.

DayTrail closes that gap. Not by watching you, but by remembering what your computer already knows.

---

## Why DayTrail feels different

| What matters | DayTrail | Traditional time trackers | Cloud AI trackers |
| --- | --- | --- | --- |
| Privacy | Local-first, no account, no backend | Often local, but manual | Usually cloud-based |
| Effort | No timers, no manual notes | Start/stop timers and edits | Passive, but opaque |
| Footprint | Small Tauri + Rust desktop app | Often heavier desktop stacks | Higher CPU/RAM from rich capture |
| AI | Optional, BYO key or local Ollama | Usually none | Built in, usually subscription-based |
| Developer context | Apps, tabs, projects, files, terminals, AI tools, git commits | Mostly app names and categories | General activity, often screenshot-heavy |
| Task evidence | Time proof per task — apps used, work sessions, AI tools, auto-matched activity | Manual time entries | None |

DayTrail is not trying to be surveillance software. It is trying to be a private work memory: enough context to reconstruct your day, without sending your life to someone else's server.

---

## What a day with DayTrail looks like

### Your whole day, reconstructed — automatically

![DayTrail Today dashboard: current work is "Claude · Deep dive into project codebase", with daily stats and a 24-hour timeline](docs/screenshots/01-today.png)

One view. No setup required. You'll see what you're doing right now (not just "Chrome" — the actual tab, the actual project), the real breakdown of where your hours went, and a timeline you can scroll through hour by hour.

The first time you see it, you'll feel something. Maybe recognition. Maybe a little discomfort. Both are useful.

---

## What it helps you see, and why it matters

**Where your day actually went**
Not a rough estimate — the real thing. The 90 minutes in email you'd have called "a quick check." The hour on YouTube you'd have written off as five minutes. When you see it clearly, you can change it.

**Whether you can trust your own memory**
Compare what you remember with the captured timeline. Most people are surprised. Not because they're lazy — because memory is unreliable about time. Once you know this about yourself, you stop blaming yourself for "feeling" productive while getting less done than expected.

**Where focus disappears**
DayTrail shows you the exact moments your attention fragmented — context switches, tab jumps, the gap between intention and action. Patterns repeat. Once you see yours, they're hard to unsee.

**How much of your work runs through AI**
ChatGPT, Claude, Copilot, Cursor — DayTrail tracks all of them as first-class work. You'll know which tools you actually rely on, for how long, and on which projects. As AI becomes woven into how we work, this matters more every week.

**What actually happened on each task**
Not just "I worked on this task today" — but which apps you used, for how long, across which work sessions, and whether AI tools were involved. Every task becomes evidence-backed. When you need to know if you actually touched something last Tuesday, DayTrail tells you.

**What to say at your standup**
Source-backed. Specific. No reconstructing from memory the night before.

---

## Features that quietly change your relationship with your own work

### Light as a feather, tight as a drum

Built with Tauri + Rust — no bundled Chromium runtime. DayTrail runs quietly in the background, keeps installer sizes small, and stays out of your way while it captures the context you would otherwise forget.

### Focus Mode — catch the drift before it becomes an hour

![Focus Mode sidebar controls with focus label and duration choices](docs/screenshots/07-focus-mode.png)

Start a focus block — 25, 50, or 90 minutes — and DayTrail sends a gentle notification when you've drifted to YouTube, Reddit, WhatsApp, or other distractions. It reminds you. It never blocks apps. It never judges. And every session is saved, so you can review later whether the block actually stayed on track.

By default these are quiet native OS notifications. If you want something more polished, enable **Premium notification island** in Settings → Capture Health. DayTrail then shows compact Dynamic Island-style nudges with a subtle glow when the app window is visible, falls back to native notifications in the background, and lets you choose DayTrail, Glass, Subtle, or Silent sounds.

The difference between a nudge at minute three and discovering the drift at hour two is enormous.

### AI-native capabilities — optional, private, and actually useful

![Ask AI chat showing a real question about app usage and a detailed AI response with actual data](docs/screenshots/04-ask-ai.png)

DayTrail does not just log data. With Claude, GPT-4, Gemini, DeepSeek, or a local Ollama model configured, it helps you reason about what happened:

- **Ask AI:** ask plain-English questions like "Which projects did I ignore this week?" or "What did I spend the most time on today?"
- **Proactive insights:** background analysis surfaces patterns you would not have thought to search for.
- **AI impact tracking:** ChatGPT, Claude, Codex, Copilot, Cursor, and similar tools become first-class work sessions instead of disappearing into "browser time."
- **Daily and weekly reports:** turn captured work into a standup, timesheet, client update, or weekly retro draft.

Because the provider is optional and configured locally, you choose whether DayTrail stays fully offline or uses your own AI key.

![AI Insights showing proactive pattern cards: AI dominance, fragmented sessions, open loops](docs/screenshots/05-insights.png)

Examples of insights DayTrail can surface:

- You've had three days without a real deep work block
- Your context-switching spiked 60% this week compared to last
- You have two open commitments that haven't been touched in four days
- Your AI tool usage doubled but the project output didn't

High-priority insights fire a notification. All of them live in the Insights view — dismissable, filterable, with a one-click "Explore in chat" button that takes you directly into a conversation about what was found.

![Native macOS notification from DayTrail showing a proactive AI usage insight](docs/screenshots/09-proactive-notification.png)

Native notifications mean the useful patterns come to you while the day is still happening — not only after you remember to open a dashboard.

This is what makes DayTrail feel like an AI-native app rather than a tracker with a dashboard.

![AI Impact view showing per-tool usage: ChatGPT, Claude, and Claude Code with durations](docs/screenshots/02-ai-impact.png)

DayTrail also tracks which AI tools you actually rely on, for how long, and on which projects — because AI work is real work, not random browser time.

### Smart Breaks — sustainable work without another dashboard

Enabled optionally in Settings. When turned on, DayTrail watches the same foreground-window signals it already uses and notices when you've been at it for a while. It sends blink reminders, posture resets, and short break prompts — at the interval you choose, using your native notification style or the optional premium island. It stays quiet during calls, presentation-like contexts, or when you step away. No extra card on your Today screen. No medical claims. Just the kind of nudge a good colleague might give you.

### Replay / restore — pick up exactly where you were

After an interruption — a meeting, a lunch, an unexpected call — DayTrail shows you what you were in before you left. The app, the project, the file, the context. You don't have to rebuild the mental model from scratch. The trail is there.

### Daily & Weekly Reports — a digest you're not embarrassed to share

![Reports view showing a generated daily work report with session breakdown and AI tools detected](docs/screenshots/08-reports.png)

One click generates a source-backed report of everything you worked on — sessions, apps, projects, AI tools used, and items to review. With an AI provider connected, it turns the raw log into a first draft for your standup, client update, or weekly retro. You edit; you don't invent.

### Activity — the story behind each session

![Activity view showing a work session broken down by app and AI tool](docs/screenshots/03-activity.png)

Every work session breaks down into the apps, projects, and AI tools behind it. A single block of time tells the whole story — not just "I worked on Project X" but how you moved through it.

### Task Activity Timeline — tasks with proof of work

Most task apps say "task exists." DayTrail says "here is what actually happened on this task."

When you expand any task, the **Timeline** tab shows the full evidence record: total time tracked, every app involved, work sessions grouped by continuous blocks, and any AI tools detected. No manual logging. The connection between your work and your task list forms automatically.

**Auto-link suggestions** go further. DayTrail scores every unlinked activity against your task's title keywords — if a source event mentions the same project path, domain, or keyword, it surfaces as a candidate. One click links it. One click dismisses it. You decide; DayTrail does the finding.

Over time, tasks become receipts: "Worked 1h 42m across VS Code, GitLab, and Claude. Three sessions, two days."

### Goal Tracking — targets you can actually verify

Set a daily time target for any app, project path, or category ("3h/day on Code", "2h/day on `/Users/me/client-project`", "1h/day on development"). DayTrail tracks progress through the day using the same interval-merged event data that powers the timeline — no double-counting, no inflation from tab switches.

Progress bars appear in the Today view as you work. Add and remove goals in **Settings → Daily Goals**.

### Momentum View — know if the streak is real

At the bottom of the Today view, a streak summary shows how many consecutive days you've had meaningful tracked time (≥ 30 minutes, configurable). Current streak, best streak in the last 30 days, average daily tracked time, and active day count.

It doesn't require habits or manual check-ins. It's just the record, automatically.

### Git Commit Integration — code shipped, captured automatically

If you use the terminal bridge, DayTrail now detects `git commit` commands and captures the commit message, repository, and branch as a first-class event. A **Code shipped** panel in the Today view shows every commit made today — without you doing anything extra.

### Capture Health — it tells you when something breaks

![Capture Health settings showing all capture sources green and Accessibility granted](docs/screenshots/06-capture-health.png)

Most trackers fail silently. You lose a full day of data before noticing. DayTrail watches its own capture engine — if a permission gets revoked or a bridge stops working, it tells you exactly what's wrong and how to fix it.

---

## Your data stays yours. Completely.

This is non-negotiable for us. Everything DayTrail captures stays on your machine. Always.

- No cloud sync. No account. No backend you're trusting someone else to secure.
- Screenshots are off by default.
- Clipboard content is never stored.
- Browser URLs are redacted before storage where possible.
- AI providers are optional, configured locally, and queried only when you ask.
- If you uninstall DayTrail, your data stays on your machine — in your control.

See [PRIVACY.md](PRIVACY.md) for the complete model.

---

## Codex plugin — ask your local DayTrail history

DayTrail also ships with **DayTrail Helper**, a read-only Codex plugin for people who want to ask questions about their captured work from inside Codex.

It connects to the local DayTrail SQLite database on your machine and can answer things like:

- "Summarize my DayTrail activity today."
- "Show my open DayTrail tasks."
- "Search DayTrail for Slack yesterday."
- "What AI tools did I use this week?"
- "Show my recent DayTrail reports."

The plugin does not upload, sync, or modify your data. It reads your local database in read-only mode through an MCP server.

See [plugins/daytrail-helper](plugins/daytrail-helper) for install and usage details.

---

## Tiny footprint. No Electron bloat.

Built with Tauri + Rust — no bundled Chromium runtime. DayTrail ships as a small native desktop app instead of bundling a whole browser engine. Current release installers are roughly **10-12 MB on macOS** and **6-9 MB on Windows**, depending on installer format.

---

## Download

The fastest install paths are also listed in [Quick Start](#quick-start).

**macOS Apple Silicon — Homebrew recommended:**

```sh
brew tap varaprasadreddy9676/tap
brew trust varaprasadreddy9676/tap
brew install --cask daytrail
```

> **`brew trust` is required once.** Recent versions of Homebrew require you to explicitly trust third-party taps before installing casks from them. You only need to run this once per machine.

To update to the latest version:

```sh
brew update && brew upgrade --cask daytrail
```

> **`brew update` first is required.** `brew upgrade` alone uses your local tap cache — without `brew update`, Homebrew won't know a new version exists and will say "already installed". Always run both commands together.

**macOS — one-line installer (no Homebrew needed):**

Paste this in Terminal — it downloads the latest release, installs to `/Applications`, and clears the Gatekeeper flag automatically:

```sh
curl -fsSL https://raw.githubusercontent.com/varaprasadreddy9676/DayTrail/main/scripts/install-macos.sh | bash
```

**macOS — manual DMG:** grab the latest `.dmg` from the [**Releases page**](https://github.com/varaprasadreddy9676/DayTrail/releases/latest), drag to Applications, then run this once to clear the Gatekeeper flag:

```bash
xattr -dr com.apple.quarantine /Applications/DayTrail.app
```

**Windows:** download the `.msi` or `.exe` installer from the same Releases page.

**Other Macs / build from source:** see [Try it](#try-it-build-from-source) below.

---

## Troubleshooting

<details open>
<summary><b>macOS: "DayTrail.app is damaged and can't be opened."</b></summary>

It is **not** damaged. Because the app isn't notarized (no paid Apple Developer ID), macOS blocks the first launch. There are three ways to fix this — pick the easiest:

**Option 1 — use Homebrew** (handles it automatically):
```sh
brew tap varaprasadreddy9676/tap
brew trust varaprasadreddy9676/tap
brew install --cask daytrail
```

**Option 2 — use the one-line installer** (handles it automatically):
```sh
curl -fsSL https://raw.githubusercontent.com/varaprasadreddy9676/DayTrail/main/scripts/install-macos.sh | bash
```

**Option 3 — manual fix after DMG install** (one command, one time per version):
```bash
xattr -dr com.apple.quarantine /Applications/DayTrail.app
```
Drag the app to **Applications** first, then run the command above, then open normally.
</details>

<details>
<summary><b>Windows: "Windows protected your PC" (SmartScreen)</b></summary>

The installer isn't code-signed yet, so SmartScreen warns on first run. Click **More info → Run anyway**.
</details>

<details>
<summary><b>Homebrew: "Refusing to load cask from untrusted tap"</b></summary>

Recent versions of Homebrew require you to trust third-party taps before installing casks. Run once:

```sh
brew trust varaprasadreddy9676/tap
brew install --cask daytrail
```

</details>

<details>
<summary><b>Homebrew: "already installed" or installs an old version</b></summary>

Homebrew caches tap metadata locally. If `brew upgrade --cask daytrail` says the latest is already installed but you know a newer version exists, your local cache is stale.

Fix:

```sh
brew update && brew upgrade --cask daytrail
```

If it still shows the wrong version, force a clean reinstall:

```sh
brew uninstall --cask daytrail
brew update
brew install --cask daytrail
```

</details>

<details>
<summary><b>macOS: capture stopped / titles show only the app name</b></summary>

DayTrail needs **Accessibility** permission to read window titles. Open **Settings → Capture Health** in the app — if Accessibility shows as missing, use **Fix accessibility** to re-grant it. A macOS update can silently reset this.
</details>

---

## Set up for a real trial

1. Install the app and grant macOS Accessibility (or Windows equivalent) when prompted.
2. Enable browser extension support if you want tab titles and domains.
3. Install editor and terminal integrations for project and file context.
4. Allow OS notifications for Focus Mode nudges, Smart Breaks, proactive AI insights, and task reminders.
5. Set your **working hours** in Settings → Capture Health so DayTrail never asks "were you away?" at midnight.
6. Add an AI provider in Settings if you want generated digests, proactive insights, and Ask AI answers. Claude, GPT-4, Gemini, or a local Ollama model all work.
7. Optionally add daily time goals in **Settings → Daily Goals** — pick an app, project path, or category and set a target.
8. Leave DayTrail running from startup. One full workday of capture is when it starts to get interesting.

---

## Platform status

| Platform | Status |
| --- | --- |
| macOS | Primary target. Fully exercised — install, permissions, tray, capture, reporting, Focus Mode, and AI flows. |
| Windows | Backend, tray, terminal bridge, credential storage, and CI installers are implemented. Real smoke testing still required before a signed public release. |
| Linux | Not a release target yet. Some Tauri pieces may work; capture behavior isn't validated. |

---

## Repository layout

```
apps/desktop/              Tauri desktop app — Rust backend, React UI
apps/browser-extension/    Browser extension for tab context
apps/vscode-extension/     VS Code / editor bridge
plugins/daytrail-helper/   Codex plugin for read-only local DayTrail queries
.agents/plugins/           Repo-local Codex marketplace entry
scripts/                   Build, release, bridge, and verification scripts
docs/                      Supporting docs and screenshot assets
```

---

## Development

Requirements: Node.js 20+, npm, Rust stable, Tauri platform prerequisites for your OS.

```bash
npm ci --prefix apps/desktop
cd apps/desktop && npm run tauri dev
```

Run the full quality gate:

```bash
npm run release:check
```

Targeted checks:

```bash
npm run desktop:check
npm run desktop:test
npm run browser-extension:check
npm run vscode-extension:check
npm run test:scripts
```

---

## Build installers

macOS unsigned local build:

```bash
npm run desktop:dmg
```

Windows installer from a Windows machine:

```powershell
npm run desktop:windows
```

See [RELEASE.md](RELEASE.md) for the full release checklist, signing notes, and verification steps.

---

## Release automation

Every non-release push to `main` triggers a release candidate. If the commit bumps the desktop version, GitHub Actions tags that version. If the current version is already tagged, Actions bumps the patch version, commits, tags, and dispatches macOS and Windows builds automatically.

Use `scripts/release.sh <version>` to cut a specific version manually.

---

## Documentation

- [PRIVACY.md](PRIVACY.md) — local storage, metadata capture, redaction, exports, AI provider behavior
- [SECURITY.md](SECURITY.md) — how to report security issues
- [RELEASE.md](RELEASE.md) — release verification checklist
- [docs/screenshots/README.md](docs/screenshots/README.md) — public screenshot set

---

## Support DayTrail

DayTrail is free and open source, built by one developer in the open. If it's helped you understand your own days — or you just want to see it keep growing — you can support development here:

[![Buy me a coffee on Ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/iamsai)

Stars, issues, and pull requests are equally welcome.

---

## License

MIT OR Apache-2.0. See [LICENSE](LICENSE), [LICENSE-MIT](LICENSE-MIT), and [LICENSE-APACHE](LICENSE-APACHE).
