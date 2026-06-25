# Release Checklist

Use this checklist before publishing a public release.

## Local Verification

```bash
npm run release:check
```

## Versioning

Every non-release push to `main` is release-oriented. The `Auto-tag release`
workflow uses an explicit version bump when one is present; otherwise, if the
current desktop version is already tagged, it bumps the patch version,
commits the metadata update, creates the next `vX.Y.Z` tag, and dispatches the
macOS and Windows release builds.

For a deliberate manual version, run `scripts/release.sh <version>` from a
clean `main` branch. That script updates all desktop version metadata,
commits `chore(release): vX.Y.Z`, pushes the commit and tag together, and the
release workflows build the installer assets.

Also verify the installed desktop app manually:

- App launches and hides to tray instead of exiting on close.
- Start, pause, resume, and quit work from the tray.
- Active app/window capture updates while switching apps.
- VS Code/Cursor project capture distinguishes multiple projects.
- Browser bridge captures domains and redacted URLs.
- Terminal bridge captures current folder and redacted commands.
- AI usage appears for observed AI tools and DayTrail-generated reports.
- Calendar/planned-work reconciliation is not presented as a user-facing feature
  unless a connector or clear planned-block entry point is shipped.
- Focus Mode nudges still work, and focus timer sessions are persisted with
  goal, target duration, elapsed time, and drift summary.
- Smart Breaks are optional, configurable in Settings, send blink/posture/break
  system notifications after sustained input, reset on idle, and stay quiet in
  calls or presentation-like contexts.
- Away/resume gaps create idle recovery prompts without surfacing sleep-sized
  gaps as classification work.
- Working hours setting: gaps outside the configured window are auto-classified
  as off-hours (no "were you away?" prompt at 1am or on weekends).
- Startup and focus-return update checks surface available builds automatically
  and allow an 8-hour reminder pause.
- In-app updater metadata is complete: the release `latest.json` is reachable
  from `/releases/latest/download/latest.json` and includes both
  `darwin-aarch64` and `windows-x86_64` platform entries with signed updater
  package URLs.
- Daily report, weekly digest, and replay/restore flows generate source-backed
  output from the expected local date range, including weekly Smart Breaks.
- Report generate buttons disable and show a loading state during LLM calls;
  Refresh buttons show "Refreshing…" while context is being rebuilt.
- Date-range export includes source-backed activity and AI contribution rows.
- Proactive AI insights: with an AI provider configured, the background scheduler
  runs every 3 hours (7am–10pm local), generates 1–3 data-backed observations,
  fires OS notifications for high-priority findings, and surfaces all insights in
  the Insights nav view with dismiss and "Explore in chat" actions.
- Ask AI chat: queries are routed to the relevant captured data and answered by
  the configured LLM; conversation history is maintained within the session;
  eight suggested starter prompts are shown in the empty state.
- Task Activity Timeline: expanding a task in the Tasks view shows a Timeline
  tab with total tracked time, per-app breakdown, AI tools, and work sessions
  grouped by 30-minute gaps; the Links & Rules tab remains accessible via the
  second tab.
- Auto-link suggestions: the Timeline tab surfaces scored candidate activities
  matching the task title's keywords (title, project path, domain); Accept
  links the activity, Dismiss hides the suggestion for the session.
- Goal tracking: goals created in Settings → Daily Goals appear as progress
  bars in the Today view; progress is computed from merged source event
  intervals for the matching app, project path, or category.
- Streak & momentum: the Today view shows current consecutive-day streak,
  best streak in the last 30 days, average daily tracked time, and active day
  count; the threshold is 30 minutes of tracked time per day.
- Git commit capture: `git commit -m "..."` commands in the terminal bridge
  produce a `git_commit` source event; Today view shows a "Code shipped" panel
  listing commit messages, repo names, branches, and timestamps.

## Public README And Screenshots

- Root `README.md` explains the product clearly for a first-time user.
- All screenshots referenced by the README exist and render:
  `01-today.png`, `07-focus-mode.png`, `02-ai-impact.png`,
  `03-activity.png`, and `06-capture-health.png`.
- Focus Mode is documented with its current behavior: local distraction nudges,
  native notifications, duration choices, snooze/end controls, and no app
  blocking. Persisted focus timer/review behavior is also documented.
- Smart Breaks are documented as optional local sustainable-work nudges with
  configurable timing and context awareness, not as medical or eye-care advice.
- Proactive AI insights and Ask AI chat are documented as requiring an AI
  provider to be configured in Settings.
- README does not claim user-facing calendar/planned-block support unless that
  workflow is actually shipped.
- Regenerate `01-today.png` after UI layout changes so the screenshot matches
  the current Today screen.
- README mentions weekly digest, replay/restore, idle recovery, proactive
  insights, Ask AI chat, and interruption-friendly positioning consistently with
  the app UI.
- README mentions Task Activity Timeline, auto-link suggestions, goal tracking,
  streak & momentum, and git commit capture with accurate descriptions matching
  current app behavior.
- Screenshots use realistic demo data and do not expose secrets, private
  customers, private email addresses, internal IPs, or local-only paths.
- Platform status and known limitations are accurate for the release being
  published.
- Install, build, and verification commands in the README have been checked.

## macOS Distribution

- Build unsigned DMG via CI (`Auto-tag release` workflow) or locally with
  `npm run desktop:dmg`.
- DayTrail strips its own quarantine xattr on first launch, so the `xattr`
  manual step is no longer required for most users.
- Verify Gatekeeper behaviour: app should open after the first-launch
  quarantine strip without requiring the user to run `xattr` manually.
- Confirm the Homebrew cask in `varaprasadreddy9676/homebrew-tap` was updated
  automatically by CI (check for a new commit to `Casks/daytrail.rb`).
- Homebrew install path (`brew install --cask daytrail`) should work and
  install the correct version without any quarantine warning.

## Windows Distribution

- Run the `Windows Build` GitHub Actions workflow and download the uploaded NSIS/MSI artifacts.
- For a local Windows artifact build, run `npm run desktop:windows` on a Windows machine.
- Build NSIS/MSI artifacts on Windows.
- Sign executable and installer artifacts.
- Install on a clean Windows account.
- Verify foreground-window capture, Credential Manager API key storage, startup launch, tray controls, browser native host registration, and PowerShell terminal bridge.

## Release Artifacts

- Check artifact size before publishing. Current targets are roughly:
  macOS DMG under 10 MB, Windows NSIS `.exe` under 5 MB, and Windows MSI under
  6 MB.
- Include checksums for every installer/archive.
- Attach release notes listing supported OS status and known limitations.
- Do not publish local databases, generated test output, or unsigned artifacts as production builds.
