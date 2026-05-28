Below is a screen-by-screen review you can share with the generated UI references.

## Overall Product Direction

The app should move from a **data-table-heavy tracker** to a **work memory timeline**.

The core user questions are:

1. **What did I do today?**
2. **Where did my time go?**
3. **Which hour was productive or distracted?**
4. **Inside an app, what exactly was I working on?**
5. **Where did I use AI tools like Copilot, Codex, Claude, ChatGPT, Cursor, Gemini?**
6. **What should I follow up on?**

The current app captures useful data, but the UI makes everything feel equally important. The redesign should use **progressive disclosure**:

```text
Day overview → Hour breakdown → App breakdown → File / website / AI interaction detail
```

---

# 1. Today Screen

## Current Problems

* Too many sections compete at once: captured activity, KPI cards, hour chart, recent work, AI usage, usage by app, attention.
* The **hour-by-hour tracker is the most important part**, but it is visually compressed.
* The right sidebar repeats information that should be integrated into the selected hour or daily summary.
* App bars inside each hour are hard to read when many apps are used.
* The “Last captured activity” card is too large for the value it provides.
* Recent work looks like a table, but the user likely wants a narrative: “What did I work on?”

## Recommended Changes

### A. Make the 24-hour timeline the hero

Use generated screen **1 / 4 / 5** as the main reference.

The Today screen should have:

```text
Top summary cards
↓
Large 24-hour timeline
↓
Insights + top apps + recent highlights
```

### B. Replace the current hour chart with a modern stacked timeline

Each hour should show **multiple app segments**, not one dominant bar.

Example:

```text
9 AM     VS Code 35m | Chrome 12m | Slack 8m | ChatGPT 5m
10 AM    VS Code 28m | ChatGPT 18m | Chrome 10m | Terminal 4m
11 AM    VS Code 41m | Copilot 12m | Brave 8m
```

Use:

* App colors
* AI badges under the hour
* Hover tooltip
* Clickable hour row

### C. Selected hour should open a right-side detail panel

The current selected-hour panel is too narrow and cramped.

When clicking `9 AM`, show:

```text
9:00 AM – 10:00 AM
Total tracked: 60m
Productive: 48m
Apps: 7
AI usage: 23m

VS Code      28m   4 files · 112 edits · Copilot used
Chrome       12m   5 tabs · 3 domains
Slack         8m   4 channels · 12 messages
Notion        6m   2 pages
Terminal      3m   8 commands
Idle          1m
```

Add a clear CTA:

```text
View full hour breakdown →
```

### D. Remove / collapse secondary right widgets

Move these into better places:

| Current Widget     | Recommendation                                |
| ------------------ | --------------------------------------------- |
| AI usage side card | Move into daily summary + selected-hour panel |
| Usage by app       | Move below timeline as “Top apps today”       |
| Attention          | Move to Follow-ups screen                     |
| Recent work        | Convert into “Recent highlights” cards        |

### E. Replace “Last captured activity” with compact live status

Instead of a full-width card:

```text
Capturing now: MySQL Workbench · 2m · LMS-production
```

Put this in the header or a small live status card.

---

# 2. Activity Screen

## Current Problems

* Three-column table layout is functional but visually heavy.
* “Folders, sites, windows” and “Activity details” are hard to scan.
* Repeated rows show the same project/path many times.
* It does not feel like a drill-down from the timeline.
* App detail is not rich enough for developer workflows.
* AI usage is only a tag, not a useful explanation.

## Recommended Changes

### A. Make Activity an app drill-down explorer

This screen should answer:

> “Inside VS Code / Chrome / Slack / Terminal, what exactly did I work on?”

Use generated screen **3** as the main reference for VS Code.

### B. Change structure to this

```text
Activity
Filters: App · Project · AI Tool · Time range · Search

Left: App list
Middle: Places / projects / domains
Right: Detailed activity
```

Current structure is close, but it needs stronger visual grouping.

### C. For VS Code, show developer-specific metadata

When VS Code is selected, show:

```text
VS Code
4h 46m · 27 events · 3 projects · 4 AI tools

Projects
- LMS-production
- CFM-main
- Work tracker

Files edited
- Header.tsx
- auth.service.ts
- useUser.ts
- package.json

Context
Workspace: LMS-production
Path: /Users/.../Downloads/Code-Projects/LMS-production
Git branch: feature/user-auth
Language: TypeScript
Extensions: Copilot, ESLint, Prettier
```

### D. Add AI details inside app activity

For each app/project, show:

```text
AI used:
Copilot · 2h 12m
Codex · 1h 04m
Claude Code · 48m

Signals:
42 completions accepted
6 chat messages
286 generated lines
3 agent sessions
```

This makes the AI tracking meaningful instead of just showing badges.

### E. Reduce repeated path rows

Group repeated events into sessions:

Current:

```text
LMS-production
LMS-production
LMS-production
LMS-production
```

Better:

```text
LMS-production
2h 7m · 10 events · 4 files · Copilot + Codex

Timeline:
9:12 Opened Header.tsx
9:18 Edited auth.service.ts
9:31 Accepted Copilot suggestion
9:44 Ran tests
```

---

# 3. AI Usage Screen

## Current Problems

* The screen shows large numbers but does not explain whether AI was useful.
* “AI time” is not enough. Users need to know:

  * Which tool?
  * In which app?
  * In which project?
  * What was produced?
  * Was it accepted, ignored, or failed?
* The right-side activity records are repetitive.
* “AI-assisted work: 0” conflicts with high AI time, which feels confusing.

## Recommended Changes

### A. Reframe this screen as “AI Impact”

Rename:

```text
AI Usage
```

to:

```text
AI Impact
```

Because the interesting thing is not only time spent, but what AI helped with.

### B. Use these top cards

```text
AI time        17h 39m
Tools used     6
Projects       3
Accepted output / useful actions
Agent sessions
Needs review / failed agents
```

Avoid showing `AI-assisted work: 0` unless the app can explain why.

### C. Group by tool + project + app

Instead of flat records, use:

```text
Copilot
5h 23m
Used in: VS Code
Projects: CFM-main, LMS-production, Work tracker
Signals: completions, chat, generated lines

Codex
5h 23m
Used in: VS Code / Terminal
Projects: CFM-main, LMS-production
Signals: agent sessions, code edits, terminal commands

Claude Code
5h 05m
Used in: Terminal / VS Code
Projects: CFM-main, Work tracker
Signals: file edits, commands, summaries
```

### D. Add AI timeline

A useful AI screen should show **when AI was used during the day**.

Example:

```text
9 AM     Copilot 14m · ChatGPT 6m · Codex 3m
10 AM    Codex 22m · Copilot 18m
11 AM    Claude Code 35m · Copilot 12m
```

### E. Add “AI interactions” detail view

Clicking Copilot or Codex should show:

```text
Tool: GitHub Copilot
App: VS Code
Project: LMS-production
File: auth.service.ts

Events:
9:12 Asked Copilot to optimize function
9:14 Accepted 18-line suggestion
9:18 Rejected inline suggestion
9:24 Generated test case
```

### F. Add quality/status labels

AI usage should distinguish:

```text
Accepted
Edited after generation
Rejected
Observed only
Failed
Needs review
```

This is more valuable than just “captured.”

---

# 4. Follow-ups Screen

## Current Problems

* Empty state dominates the screen.
* The screen does not teach the user what kind of follow-ups will appear.
* The categories are useful, but the page feels unfinished.
* It is disconnected from actual activity, Slack, AI tools, and meetings.

## Recommended Changes

### A. Rename to “Needs Review”

This label is clearer than “Follow-ups.”

Suggested nav:

```text
Needs Review
```

or

```text
Review Queue
```

### B. Improve empty state

Instead of only:

```text
Nothing needs review
```

Use:

```text
Nothing needs review

This page will show:
- Unanswered Slack messages
- Promises you made
- AI agent failures
- Drafts that were not sent
- Long idle gaps
- Meeting action items
- Tasks mentioned but not completed
```

Add buttons:

```text
Review detection settings
Open today’s activity
```

### C. When items exist, group by risk

Use this structure:

```text
High priority
- You said “I’ll send this today” in Slack but no follow-up was detected.
- Claude Code failed during LMS-production session.

Medium priority
- Long idle gap after starting database migration.
- Drafted response in Gmail but did not send.

Low priority
- Meeting note mentioned “check auth bug.”
```

### D. Connect every follow-up to source context

Each item should show:

```text
Source: Slack #project-alpha
Time: 2:05 PM
Related app: VS Code
Related project: LMS-production
AI confidence: 82%
Action: Mark done / Snooze / Ignore / Create task
```

---

# 5. Reports Screen

## Current Problems

* The report generator looks like a markdown editor, not a polished reporting workflow.
* The generated report is empty even though captured facts exist.
* The left “Captured facts” list is too vague.
* The report should explain the day in a useful narrative.

## Recommended Changes

### A. Split report flow into 3 steps

```text
1. Select report type
2. Review included data
3. Generate / edit / export
```

### B. Improve report tabs

Current tabs:

```text
Morning Plan | End-of-Day | Weekly Review
```

Good, but make them more distinct:

```text
Morning Plan
End-of-Day Summary
Weekly Review
Client / Manager Update
Personal Productivity
AI Usage Report
```

### C. Make captured facts meaningful

Current:

```text
Session: CFM-main
Session: LMS-production
Scratchpad: need to plan for kt
```

Better:

```text
CFM-main
1h 49m · VS Code · Copilot + Codex · 6 files edited

LMS-production
2h 07m · VS Code · 10 events · auth.service.ts, Header.tsx

Slack follow-ups
0 open

AI usage
Copilot 5h 23m · Codex 5h 23m · Claude Code 5h 05m
```

### D. Generated report should not be blank

The report should synthesize from the timeline:

```markdown
# Daily Work Execution Report

## Completed work
- Worked on LMS-production for 2h 07m, primarily in VS Code.
- Edited authentication-related TypeScript files.
- Used Copilot and Codex during development.
- Reviewed browser references related to implementation.

## AI assistance
- Copilot supported code completions and refactoring.
- Codex was used during implementation sessions.
- Claude Code was active in CFM-main and Work tracker sessions.

## Follow-ups
- No open follow-ups detected.

## Next best action
- Review LMS-production changes and commit completed work.
```

### E. Add export targets

Use buttons:

```text
Copy Markdown
Export PDF
Send to Slack
Create Jira update
Save report
```

---

# 6. Settings Screen

## Current Problems

* Settings are visually dense and table-like.
* Privacy information is important but buried.
* AI provider settings are technical and should be clearer.
* Status rows are useful but need a dedicated “Capture health” section.
* It is not obvious what data is stored and what is never stored.

## Recommended Changes

### A. Split settings into clear sections

```text
Capture
AI Provider
Privacy
Integrations
Data Storage
Shortcuts
About
```

### B. Add a capture health panel

At the top:

```text
Capture health

Desktop watcher      Connected
Browser bridge       Connected
Editor bridge        Connected
Terminal folders     Connected
AI tools             Connected
Privacy policy       Metadata only
```

Each row should have:

```text
Status · Last signal · Configure
```

### C. Make privacy section more explicit

Current privacy section is good conceptually. Make it more user-readable:

```text
What WorkTrace stores

Apps and windows        Active metadata only
Browsers                Domain + redacted URL
Editor and terminal     Project/folder path
AI prompts              Redacted before analysis
Screenshots             Not captured
Clipboard content       Not captured
File contents           Not captured by default
```

Add a visible privacy badge:

```text
Metadata-first capture
Screenshots off
Clipboard not stored
```

### D. AI provider settings should be easier to understand

Current:

```text
Provider: Ollama Local
Model: llama3.1
Endpoint: http://127.0.0.1...
API key: Stored in OS keychain
```

Better:

```text
AI Analysis Provider

Mode: Local
Provider: Ollama
Model: llama3.1
Status: Ready
Endpoint: Advanced setting
API key: Stored in OS keychain
```

Hide endpoint by default under “Advanced.”

### E. Add data controls

Important for this type of app:

```text
Pause capture
Delete today’s data
Delete all local data
Export raw data
Manage excluded apps
Manage excluded domains
Manage ignored folders
```

---

# Navigation / Information Architecture Changes

## Current Navigation

```text
Today
Activity
AI Usage
Follow-ups
Reports
Settings
```

## Recommended Navigation

```text
Today
Activity
AI Impact
Needs Review
Reports
Settings
```

Or even simpler:

```text
Today
Explore
AI Impact
Review
Reports
Settings
```

## Sidebar Apps Today

The current “Apps Today” sidebar is useful, but it takes too much permanent space.

Recommended:

* Keep it collapsed by default.
* Move app list into Today screen as “Top apps today.”
* Use sidebar only for primary navigation.
* Add app filters inside the content area instead.

---

# Priority Implementation Plan

## Phase 1 — Biggest UX Win

Redesign **Today** screen around generated screen **4 or 5**:

* Large 24-hour stacked timeline
* Top KPI cards
* Clickable hour details panel
* AI usage integrated into selected hour
* Top apps and insights below

## Phase 2 — Drill-down

Redesign **Activity** as app/project drill-down:

```text
App → Project / Site / Folder → Files / Events / AI usage
```

Use generated screen **3** for VS Code detail.

## Phase 3 — AI Impact

Replace current AI Usage table with:

```text
Tool summary → Project usage → Timeline → Interaction details
```

## Phase 4 — Reports

Make reports generate actual useful summaries from captured facts.

## Phase 5 — Settings / Privacy

Clean up settings and make privacy trust much more visible.

---

# Copy-ready Design Direction

Use this as the instruction with the generated screens:

```text
Redesign the app UI to use a progressive work-memory model:

Day overview → Hour breakdown → App/project detail → File/AI interaction detail.

The Today screen should prioritize a large 24-hour stacked timeline where every hour can contain multiple app segments. Clicking an hour opens a detailed breakdown showing apps used, duration, project/folder/site metadata, and AI tools active in that hour.

The Activity screen should become an app drill-down explorer. For apps like VS Code, show projects, folder paths, files edited, branches, extensions, AI tools used, sessions, and command/activity history. Avoid repeated raw event rows; group events into meaningful sessions.

The AI Usage screen should become AI Impact. Show AI usage by tool, app, project, and hour. Include accepted suggestions, generated code, prompts, agent sessions, failures, and needs-review items where available.

The Follow-ups screen should become a review queue for unanswered messages, promises, idle gaps, AI failures, meeting action items, and unfinished work. Improve empty states with clear explanations.

The Reports screen should synthesize actual captured work into readable summaries, not only display raw captured facts. Support markdown, copy, export, and daily/weekly report templates.

The Settings screen should be simplified into Capture, AI Provider, Privacy, Integrations, Data Storage, and Advanced. Make metadata-first privacy guarantees highly visible.
```

## Clear recommendation

Build DayTrail as a **simple workday memory app first**, not as a data dashboard.

Your unique product should be:

```text
DayTrail
Retrace your workday.

A private timeline that turns app/window/AI activity into clean work sessions, reports, and review items.
```

The practical path is:

```text
Desktop-only capture by default
+ Smart grouping
+ Confidence labels
+ Simple/Pro UI modes
+ Optional accuracy helpers
```

Do **not** try to make the app show every raw thing it captures. That is why users feel overwhelmed.

Also: “100% robust” should not mean “capture every internal detail from every app.” That is not realistic. It should mean:

```text
The app is always honest, useful, recoverable, privacy-safe, and clear about what was captured vs inferred.
```

---

# 1. Product Strategy

## What DayTrail should become

DayTrail should have three layers:

```text
1. Simple daily memory
   What did I do today?

2. Review and reporting
   What should I remember, fix, send, or report?

3. Pro activity detail
   What exactly happened inside apps, folders, projects, AI tools, and sessions?
```

The default user should see layer 1. Power users can open layers 2 and 3.

---

# 2. Simple Mode and Pro Mode

This is the biggest product fix.

## Simple Mode — default

For normal users.

Navigation:

```text
Today
Activity
Review
Reports
Settings
```

Simple Mode should hide:

```text
Raw events
Source records
AI seconds
Debug messages
Provider details
System utility noise
Export JSON
Repeated tiny app switches
Confidence internals unless needed
```

Simple Mode should show:

```text
Now working on
Today timeline
Work sessions
Hour breakdown
Simple AI summary
Review items
Daily report
Capture health
```

## Pro Mode — optional

For developers, consultants, billing, debugging, and advanced tracking.

Pro Mode can show:

```text
Raw activity records
Capture source
Confidence
AI interaction details
App/project explorer
Timesheets
Export data
Automation candidates
Advanced settings
```

## Implementation

Add a setting:

```ts
type ExperienceMode = "simple" | "pro";

type UserSettings = {
  experienceMode: ExperienceMode;
  showSystemApps: boolean;
  showRawEvents: boolean;
  showCaptureConfidence: boolean;
  showAiDetails: "summary" | "detailed";
  captureFullUrls: boolean;
  capturePromptContent: boolean;
};
```

In React, do not create two apps. Use the same backend data and render different view models.

```ts
const settings = await invoke<UserSettings>("get_user_settings");

if (settings.experienceMode === "simple") {
  return <SimpleTodayView />;
}

return <ProTodayView />;
```

Tauri is a good fit for this structure because the React frontend can call Rust commands through Tauri’s command system, while Rust owns the native/backend logic. Tauri documents this command-based frontend-to-Rust communication model directly. ([Tauri][1])

---

# 3. Core Technical Architecture

Your stack is fine:

```text
Tauri v2
Rust backend
React 18 frontend
SQLite via rusqlite
Vite
TypeScript
```

But you need a cleaner internal architecture.

## Recommended architecture

```text
React UI
  ↓ invoke()
Rust commands
  ↓
View model services
  ↓
Session engine
  ↓
Normalizer
  ↓
Raw capture events
  ↓
SQLite
```

File layout:

```text
src-tauri/src/
  main.rs
  app_state.rs

  capture/
    macos_active_app.rs
    macos_window.rs
    idle.rs
    permissions.rs

  ingest/
    event_contract.rs
    raw_event_writer.rs

  normalize/
    app_rules.rs
    privacy_redactor.rs
    project_detector.rs
    ai_detector.rs

  sessionize/
    segment_builder.rs
    session_builder.rs
    hour_rollups.rs

  services/
    today_service.rs
    activity_service.rs
    review_service.rs
    report_service.rs
    settings_service.rs

  db/
    migrations.rs
    queries.rs
```

React should never render raw database rows directly. React should receive clean view models:

```ts
TodayView
HourBreakdownView
ActivityView
ReviewView
ReportView
SettingsView
```

---

# 4. Capture Model: Raw First, Then Clean

Your app currently feels overwhelming because raw capture leaks into the UI.

Fix this by using four layers:

```text
raw_events
normalized_events
activity_segments
work_sessions
```

## Layer 1: raw_events

Everything captured goes here.

```sql
CREATE TABLE raw_events (
  id TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  event_type TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  ended_at INTEGER,
  received_at INTEGER NOT NULL,
  confidence TEXT NOT NULL,
  payload_json TEXT NOT NULL
);
```

## Layer 2: normalized_events

Convert messy source data into a consistent format.

```sql
CREATE TABLE normalized_events (
  id TEXT PRIMARY KEY,
  raw_event_id TEXT,
  app_name TEXT,
  app_bundle_id TEXT,
  title TEXT,
  activity_type TEXT,
  project_name TEXT,
  workspace_path TEXT,
  file_path TEXT,
  domain TEXT,
  url_redacted TEXT,
  ai_tool TEXT,
  started_at INTEGER NOT NULL,
  ended_at INTEGER,
  duration_ms INTEGER,
  confidence TEXT NOT NULL,
  metadata_json TEXT
);
```

## Layer 3: activity_segments

This is what timeline/hour views use.

```sql
CREATE TABLE activity_segments (
  id TEXT PRIMARY KEY,
  date_key TEXT NOT NULL,
  hour_key TEXT NOT NULL,
  app_name TEXT NOT NULL,
  project_name TEXT,
  activity_type TEXT,
  started_at INTEGER NOT NULL,
  ended_at INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL,
  is_system_noise INTEGER DEFAULT 0,
  is_idle INTEGER DEFAULT 0,
  confidence TEXT NOT NULL,
  metadata_json TEXT
);
```

## Layer 4: work_sessions

This is what users care about.

```sql
CREATE TABLE work_sessions (
  id TEXT PRIMARY KEY,
  date_key TEXT NOT NULL,
  title TEXT NOT NULL,
  project_name TEXT,
  workspace_path TEXT,
  started_at INTEGER NOT NULL,
  ended_at INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL,
  primary_app TEXT,
  apps_json TEXT,
  ai_tools_json TEXT,
  confidence TEXT NOT NULL,
  status TEXT DEFAULT 'draft'
);
```

The UI should mostly use `work_sessions`, not `raw_events`.

---

# 5. Capture Sources and Confidence

Every captured thing must carry:

```text
source
confidence
```

This is what makes the product trustworthy.

## Source types

```ts
type CaptureSource =
  | "macos_active_app"
  | "macos_window_title"
  | "browser_window_title"
  | "vscode_helper"
  | "browser_helper"
  | "terminal_helper"
  | "manual_context"
  | "ai_inference"
  | "calendar"
  | "slack"
  | "unknown";
```

## Confidence

```ts
type CaptureConfidence =
  | "exact"
  | "high"
  | "medium"
  | "low"
  | "inferred";
```

## Example

```json
{
  "app": "VS Code",
  "project": "DayTrail",
  "duration": "1m 43s",
  "source": "macos_window_title",
  "confidence": "medium"
}
```

If a VS Code helper is installed later:

```json
{
  "app": "VS Code",
  "project": "DayTrail",
  "filePath": "/Users/.../DayTrail/src/App.tsx",
  "duration": "1m 43s",
  "source": "vscode_helper",
  "confidence": "exact"
}
```

The UI should show confidence only in Pro Mode or when something looks uncertain.

---

# 6. Desktop-Only Capture: What to Implement First

Start with desktop-only capture. This gives you a usable app without extensions.

## Capture these by default

```text
Active app
Window title
Window owner/process
Timestamps
Idle time
App switch events
Manual task/context
```

## macOS APIs

Use `NSWorkspace.didActivateApplicationNotification` to know when the active app changes. Apple’s documentation says the notification includes an `NSRunningApplication` for the affected app. ([Apple Developer][2])

Use `CGWindowListCopyWindowInfo` to retrieve window metadata from the current user session. Apple documents it as the Core Graphics API for getting window dictionaries. ([Apple Developer][3])

Use `CGEventSource.secondsSinceLastEventType` to calculate idle time. Apple documents it as returning elapsed time since the last event for a Quartz event source. ([Apple Developer][4])

## Capture loop

Use both:

```text
event-based app activation
+
polling active window every 1–2 seconds
```

Reason: app activation events are useful, but window/title changes inside the same app often need polling.

Pseudo-flow:

```rust
loop {
    let active_app = get_active_app();
    let active_window = get_frontmost_window();
    let idle_seconds = get_idle_seconds();

    let snapshot = DesktopSnapshot {
        app_name,
        bundle_id,
        pid,
        window_title,
        idle_seconds,
        captured_at,
    };

    capture_engine.ingest(snapshot);

    sleep(Duration::from_millis(1000));
}
```

## Segment creation

When app/title changes:

```text
close previous segment
start new segment
```

When idle exceeds threshold:

```text
close active segment
start idle segment
```

Recommended thresholds:

```text
< 10s app switch: ignore or merge
10s–60s utility app: system noise
2m idle: mark idle
10m idle: split work session
15m project gap: start new session
```

---

# 7. Noise Filtering

This is mandatory.

Right now users see too much system noise:

```text
System Settings
Finder
UserNotificationCenter
short Terminal blips
short app switches
```

## Add app rules

```sql
CREATE TABLE app_rules (
  app_bundle_id TEXT PRIMARY KEY,
  app_name TEXT NOT NULL,
  category TEXT NOT NULL,
  default_visibility TEXT NOT NULL,
  min_duration_ms INTEGER NOT NULL,
  productivity_weight REAL DEFAULT 0
);
```

Example rules:

```text
System Settings
category: system
default_visibility: hidden_in_simple
min_duration: 60s

Finder
category: utility
default_visibility: hidden_if_short
min_duration: 30s

VS Code
category: development
default_visibility: visible
min_duration: 5s

ChatGPT
category: ai
default_visibility: visible
min_duration: 5s
```

## Simple Mode behavior

Raw:

```text
System Settings 30s
Finder 6s
Terminal 4s
VS Code 1m43s
ChatGPT 55s
```

Simple Mode output:

```text
DayTrail development
3m 18s · VS Code, ChatGPT, Terminal
AI tools detected: Codex, ChatGPT, Gemini
```

Pro Mode can still show raw records.

---

# 8. Sessionization: The Most Important Engine

The product becomes useful only when raw events become sessions.

## Session object

```ts
type WorkSession = {
  id: string;
  title: string;
  projectName?: string;
  start: string;
  end: string;
  durationMs: number;
  primaryApp: string;
  apps: AppUsage[];
  aiTools: AiUsage[];
  confidence: CaptureConfidence;
  sourceSummary: string[];
};
```

## Session rules

### Rule 1: Project continuity

If VS Code, Terminal, Browser, and ChatGPT happen close together and share the same project signal, group them.

```text
VS Code: DayTrail
Terminal: /Documents/GitHub/DayTrail
Browser: github.com/.../DayTrail
ChatGPT: active during same block
```

Output:

```text
DayTrail development
```

### Rule 2: Short app switches do not become work

```text
Finder 4s
System Settings 8s
Notification Center 3s
```

These should not create separate sessions.

### Rule 3: Idle splits sessions

```text
Idle > 10 minutes
```

Start a new session after idle.

### Rule 4: Same project, short gap

If the same project resumes within 15 minutes, merge.

```text
9:00–9:40 DayTrail
9:40–9:47 Slack
9:47–10:15 DayTrail
```

Could become:

```text
9:00–10:15 DayTrail
with Slack interruption
```

### Rule 5: Manual context overrides inference

If user sets:

```text
Project: Client A
Ticket: PROJ-123
```

Then all new events inherit that context until changed.

This is how you support timesheets without requiring perfect automatic inference.

---

# 9. Today Screen Implementation

Today should not be a dashboard first. It should be a memory page.

## Simple Today layout

```text
Header
  Today
  Search
  Daily report

Now
  Capturing: VS Code · DayTrail · 4s ago

Today so far
  1h 24m captured · 3 work sessions · 5 apps · AI detected

Timeline
  24-hour day trail

Work sessions
  DayTrail development
  Slack communication
  Research

Needs review
  2 items

Generate daily report
```

## Low-data state

This is critical.

When captured time is under 5 minutes, do not show big analytics cards.

Show:

```text
DayTrail is capturing your workday

Captured so far:
3m 18s · 5 apps · 2 AI tools detected

Keep working. Your timeline becomes useful after a few minutes.
```

Implementation:

```ts
if (today.totalTrackedMs < 5 * 60 * 1000) {
  return <EarlyCaptureState today={today} />;
}
```

## Hide bad top-app states

Never show this in Simple Mode:

```text
Top app: System Settings
```

Use:

```text
Top work app: VS Code
System activity: 30s
```

---

# 10. Hour Breakdown Implementation

The hour screen should be useful but not raw.

## Simple version

```text
7 AM – 8 AM
3m 18s captured

What happened
- DayTrail development · 1m 43s · VS Code
- ChatGPT · 55s · AI
- System setup · 30s · hidden system activity

AI observed
- Codex · 1m 43s
- ChatGPT · 55s
- Gemini · 4s

Context
- /Users/.../Documents/GitHub/DayTrail
```

## Pro version

Add:

```text
Raw records
Capture source
Confidence
Window titles
Exact timestamps
```

## Implementation command

```rust
#[tauri::command]
async fn get_hour_breakdown(
    date_key: String,
    hour: u8,
    mode: String,
    state: tauri::State<'_, AppState>,
) -> Result<HourBreakdownView, String> {
    state
        .services
        .today
        .get_hour_breakdown(date_key, hour, mode)
        .await
        .map_err(|e| e.to_string())
}
```

---

# 11. Activity Screen Implementation

Your current Activity screen should become Pro Mode.

## Simple Activity

Default:

```text
Sessions today

DayTrail development
7:43 AM – 7:46 AM · 3m 18s
VS Code · ChatGPT · Terminal
AI: Codex, ChatGPT, Gemini

Open session →
```

## Pro Activity

Your 3-column explorer is useful here:

```text
1. Choose app
2. Select project/workspace
3. Activity details
```

But it should be labeled as advanced.

## Recommended UI tabs

```text
Sessions
Apps
Projects
AI
Raw Activity
```

In Simple Mode, only show:

```text
Sessions
Apps
Projects
```

In Pro Mode, show all.

---

# 12. Review Screen Implementation

Right now “Review” mixes two different ideas:

```text
Needs attention
Timesheet review
```

Split them.

## Needs Review

For:

```text
Unanswered messages
Promises
AI failures
Uncommitted work
Long idle gaps
Drafts
Unclear sessions
```

## Timesheets

For:

```text
Client
Project
Ticket
Billable
Confirmed
Export
```

If you only keep one nav item, use tabs:

```text
Review
  Needs Review
  Timesheets
```

## Review item schema

```sql
CREATE TABLE review_items (
  id TEXT PRIMARY KEY,
  date_key TEXT NOT NULL,
  type TEXT NOT NULL,
  priority TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT,
  source TEXT NOT NULL,
  related_session_id TEXT,
  confidence REAL,
  status TEXT DEFAULT 'open',
  created_at INTEGER NOT NULL,
  metadata_json TEXT
);
```

## Practical review rules

Start simple:

```text
Low-confidence session → review item
Long idle gap inside active work → review item
AI agent failure detected → review item
Draft timesheet missing project → review item
Manual note containing “todo”, “follow up”, “send”, “check” → review item
```

Do not try Slack promise detection until you have proper Slack integration.

---

# 13. Reports Implementation

Reports should be deterministic first, AI-polished second.

Do not rely only on an LLM.

## Pipeline

```text
sessions
+ app usage
+ AI usage
+ review items
+ manual notes
+ timesheet fields
↓
deterministic markdown report
↓
optional local AI rewrite
↓
final report
```

## Report input snapshot

Always save what data produced the report.

```sql
CREATE TABLE reports (
  id TEXT PRIMARY KEY,
  date_key TEXT NOT NULL,
  report_type TEXT NOT NULL,
  markdown TEXT NOT NULL,
  generated_at INTEGER NOT NULL,
  input_snapshot_json TEXT NOT NULL
);
```

## Report template example

```md
# Daily Work Report

## Summary
Worked for {{total_work_time}} across {{session_count}} sessions.

## Main work
{{#sessions}}
- {{title}} — {{duration}}, mostly in {{primary_app}}
{{/sessions}}

## AI assistance
{{#ai_tools}}
- {{tool}} — {{duration}}, used in {{apps}}
{{/ai_tools}}

## Needs review
{{#review_items}}
- {{title}}
{{/review_items}}
```

## UI behavior

If there is no generated report, show:

```text
Generate today’s report

DayTrail will summarize:
- Work sessions
- Apps and projects
- AI usage
- Review items
- Timesheet drafts
```

Do not show an empty markdown editor as the main state.

---

# 14. Settings Implementation

Settings should become your trust center.

## Sections

```text
Capture
AI Provider
Privacy
Integrations
Data Storage
Shortcuts
Advanced
About
```

## Capture Health

Show:

```text
Desktop watcher       Connected
Window titles         Connected / Permission needed
Idle detection        Active
Browser helper        Not installed
VS Code helper        Not installed
Terminal helper       Not installed
AI detection          Basic
```

## Why this matters

When DayTrail cannot capture something, users should know why.

Example:

```text
VS Code file details unavailable.
Install the VS Code helper to capture files, branches, and edit counts.
```

This turns a limitation into a clear upgrade path.

## Background behavior

Use:

```text
Start at login
Menu bar / tray capture status
Pause capture
Resume capture
Set current task
```

Tauri has an autostart plugin for launching the app at startup, and Tauri also supports system tray integration for quick actions. ([Tauri][5])

---

# 15. Optional Accuracy Helpers

Do not require these on day one. Offer them as “Improve accuracy.”

## VS Code helper

Purpose:

```text
Exact workspace
Exact file path
Exact language
Edit counts
Save events
Git branch
Better project detection
```

VS Code’s extension API is designed for extension authors and exposes workspace/editor APIs; this is the correct path for file-level VS Code detail. ([Visual Studio Code][6])

Offer in UI:

```text
Improve VS Code accuracy
Capture files, branches, edit counts, and workspace context.
```

## Browser helper

Purpose:

```text
Exact active tab
Domain
Redacted URL
Tab title
Navigation time
```

For Chromium browsers, use Native Messaging so the extension can communicate with the local DayTrail app. Chrome documents that Native Messaging lets extensions exchange messages with registered native applications over standard input and output. ([Chrome for Developers][7])

Offer in UI:

```text
Improve browser accuracy
Capture domains and redacted URLs instead of only window titles.
```

## Terminal helper

Purpose:

```text
Command start/end
cwd
git branch
exit code
duration
```

Offer:

```text
Install shell integration
zsh / bash / fish
```

Default privacy:

```text
Do not store full command text unless enabled.
```

## Slack / Calendar later

Do not attempt serious promise/follow-up detection from window titles.

Use real integrations later.

---

# 16. AI Impact: Make It Useful, Not Noisy

AI time alone is not valuable.

## Bad

```text
AI time: 17h 39m
Codex: 5h 23m
Copilot: 5h 23m
```

Users ask:

```text
So what?
```

## Better

```text
AI helped with:
- 3 coding sessions
- 2 report drafts
- 1 failed agent run
- 4 sessions with AI context
```

## AI event schema

```sql
CREATE TABLE ai_events (
  id TEXT PRIMARY KEY,
  tool_name TEXT NOT NULL,
  app_name TEXT,
  project_name TEXT,
  workspace_path TEXT,
  event_type TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  ended_at INTEGER,
  duration_ms INTEGER,
  confidence TEXT NOT NULL,
  metadata_json TEXT
);
```

## Event types

```text
observed
chat_active
agent_session
code_generation
completion_accepted
command_generated
failure
```

If you do not know exactly, mark it:

```text
AI observed
```

not:

```text
AI generated code
```

---

# 17. Unique Features That Are Actually Practical

## 1. Workday Memory Graph

Instead of only time bars, build a graph:

```text
Session
  → Apps
  → Projects
  → Files/sites
  → AI tools
  → Review items
  → Report facts
```

This powers everything.

## 2. Confidence-Based UI

Every insight has a source.

```text
Captured from desktop
Inferred from window title
Confirmed by user
Needs helper for exact detail
```

This is rare and valuable.

## 3. Simple Daily Rewind

A user should click a session and see:

```text
You worked on DayTrail from 7:43–7:46.
Main app: VS Code.
AI observed: Codex, ChatGPT.
Context: /Documents/GitHub/DayTrail.
```

Not raw records first.

## 4. Report from Evidence

Reports should be source-backed:

```text
This bullet came from session X.
This AI summary came from ai_event Y.
This follow-up came from review_item Z.
```

In Pro Mode, let users inspect sources.

## 5. Accuracy Setup Center

Instead of hiding limitations:

```text
Your current accuracy: Basic

Desktop activity       On
VS Code helper         Not installed
Browser helper         Not installed
Terminal helper        Not installed
Calendar               Not connected
Slack                  Not connected
```

This is practical and builds trust.

---

# 18. What to Remove or Hide Immediately

Hide from Simple Mode:

```text
Raw export screen
Source records
AI provider technical messages
System Settings as top app
Tiny second-level metrics
Huge empty tables
Repeated path rows
Automation candidates
```

Rename:

```text
Review → Needs Review / Timesheets
AI Usage → AI Impact
Source Events → Raw Activity Records
Open Activity → View Details
```

---

# 19. Implementation Roadmap

## Phase 1 — Make current app usable

Goal: reduce overwhelm.

Build:

```text
Simple/Pro setting
Low-data state
System noise filtering
Session list
Simplified Today
Hide raw events by default
Rename Review sections
```

Do not build new integrations yet.

Deliverable:

```text
A user can open DayTrail and understand today in 10 seconds.
```

---

## Phase 2 — Build the session engine

Goal: make data meaningful.

Build:

```text
raw_events
normalized_events
activity_segments
work_sessions
sessionizer
project detector
app rules
confidence labels
```

Deliverable:

```text
DayTrail groups messy activity into clean sessions.
```

---

## Phase 3 — Improve capture quality

Goal: reliable baseline.

Build:

```text
macOS app watcher
window title polling
idle detection
permissions screen
capture health
autostart
menu bar status
pause/resume
```

Deliverable:

```text
DayTrail runs quietly and captures app/window time reliably.
```

---

## Phase 4 — Add optional helpers

Goal: make Pro Mode powerful.

Build in this order:

```text
VS Code helper
Browser helper
Terminal helper
Calendar
Slack
AI tool-specific integrations
```

Deliverable:

```text
Power users can get exact project/file/browser/terminal details.
```

---

## Phase 5 — Reports and review intelligence

Goal: make the product valuable daily.

Build:

```text
Daily report generator
Weekly report generator
Needs Review rules
Timesheet drafts
Manual corrections
Learning from corrections
```

Deliverable:

```text
DayTrail saves time at the end of the day.
```

---

# 20. Concrete UI Rules

Apply these everywhere.

## Rule 1: Do not show seconds as important metrics

Bad:

```text
Time tracked: 38s
AI time: 8s
```

Better:

```text
Capturing started.
Your timeline will appear after a few minutes.
```

## Rule 2: System apps are not top apps

Never make this the headline:

```text
Top app: System Settings
```

Use:

```text
Top work app: VS Code
```

## Rule 3: Raw data is Pro only

Simple users should not see:

```text
source records
events
payloads
deterministic source-backed analysis
```

## Rule 4: Every missing detail should have a solution

Example:

```text
File-level VS Code details unavailable.
Install VS Code helper.
```

## Rule 5: Every inferred insight should be editable

Users must be able to correct:

```text
Project
Client
Task
Billable
Session title
Category
```

Then use those corrections to improve future inference.

---

# 21. Minimum Backend Commands

Create these Tauri commands.

```rust
#[tauri::command]
async fn get_today_view(date_key: String, mode: String) -> Result<TodayView, String>;

#[tauri::command]
async fn get_hour_breakdown(date_key: String, hour: u8, mode: String) -> Result<HourBreakdownView, String>;

#[tauri::command]
async fn get_activity_sessions(date_key: String, mode: String) -> Result<Vec<SessionView>, String>;

#[tauri::command]
async fn get_session_detail(session_id: String, mode: String) -> Result<SessionDetailView, String>;

#[tauri::command]
async fn update_session_context(input: UpdateSessionContextInput) -> Result<(), String>;

#[tauri::command]
async fn get_review_items(date_key: String) -> Result<Vec<ReviewItemView>, String>;

#[tauri::command]
async fn generate_daily_report(date_key: String) -> Result<ReportView, String>;

#[tauri::command]
async fn get_capture_health() -> Result<CaptureHealthView, String>;

#[tauri::command]
async fn update_user_settings(settings: UserSettings) -> Result<(), String>;
```

Frontend should call these. Do not let frontend query raw SQLite.

---

# 22. Testing for Robustness

Build test fixtures from real captured days.

## Test levels

### 1. Raw event tests

Input:

```text
app switch events
window title changes
idle gaps
```

Assert:

```text
segments are correct
durations are correct
no negative durations
no duplicated active segment
```

### 2. Sessionizer tests

Input:

```text
VS Code 20m
Terminal 5m
Chrome github.com 8m
Slack 2m
VS Code 30m
```

Expected:

```text
1 project session with Slack interruption
```

### 3. Noise tests

Input:

```text
System Settings 20s
Finder 5s
Notification Center 3s
```

Expected:

```text
hidden in Simple Mode
visible in Pro Mode
```

### 4. Low-data UI tests

Input:

```text
38 seconds captured
```

Expected:

```text
early capture state
no dashboard cards
no misleading top app
```

### 5. Report tests

Input:

```text
3 sessions
2 AI events
1 review item
```

Expected:

```text
deterministic markdown includes all important facts
```

This is how you make it robust.

---

# 23. The Practical “100%” Definition

DayTrail cannot be 100% omniscient.

But it can be 100% robust if it follows these rules:

```text
1. Capture what the OS exposes.
2. Never pretend inferred data is exact.
3. Hide low-value noise.
4. Group activity into human sessions.
5. Show simple views by default.
6. Offer Pro detail only when needed.
7. Explain missing data.
8. Provide optional helpers for higher accuracy.
9. Let users correct the timeline.
10. Learn from corrections.
```

That is the practical path.

---

## Final build target

Your next milestone should be:

```text
DayTrail Simple Mode v1
```

It should do only this:

```text
Run quietly in the background.
Capture app/window/idle activity.
Group it into work sessions.
Show a clean 24-hour timeline.
Let users click an hour/session.
Generate a useful daily report.
Explain missing detail honestly.
Hide raw/pro data unless enabled.
```

Once that feels excellent, add Pro Mode accuracy helpers.

That will make DayTrail usable, defensible, and different from generic time trackers.

[1]: https://v2.tauri.app/develop/calling-rust/?utm_source=chatgpt.com "Calling Rust from the Frontend - Tauri"
[2]: https://developer.apple.com/documentation/appkit/nsworkspace/didactivateapplicationnotification?utm_source=chatgpt.com "didActivateApplicationNotification | Apple Developer Documentation"
[3]: https://developer.apple.com/documentation/coregraphics/cgwindowlistcopywindowinfo%28_%3A_%3A%29?utm_source=chatgpt.com "CGWindowListCopyWindowInfo(_:_:) | Apple Developer Documentation"
[4]: https://developer.apple.com/documentation/coregraphics/cgeventsource/secondssincelasteventtype%28_%3Aeventtype%3A%29?utm_source=chatgpt.com "secondsSinceLastEventType(_:eventType:) | Apple Developer Documentation"
[5]: https://v2.tauri.app/plugin/autostart/?utm_source=chatgpt.com "Autostart - Tauri"
[6]: https://code.visualstudio.com/api/references/vscode-api?utm_source=chatgpt.com "VS Code API | Visual Studio Code Extension API"
[7]: https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging?utm_source=chatgpt.com "Native messaging | Chrome for Developers"