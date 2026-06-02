# Release Checklist

Use this checklist before publishing a public release.

## Local Verification

```bash
npm run release:check
```

Also verify the installed desktop app manually:

- App launches and hides to tray instead of exiting on close.
- Start, pause, resume, and quit work from the tray.
- Active app/window capture updates while switching apps.
- VS Code/Cursor project capture distinguishes multiple projects.
- Browser bridge captures domains and redacted URLs.
- Terminal bridge captures current folder and redacted commands.
- AI usage appears for observed AI tools and DayTrail-generated reports.
- Calendar events supplied through local import/integration appear in Today and
  reconcile against captured work as matched, missed, or partial.
- Focus Mode nudges still work, and focus timer sessions are persisted with
  goal, target duration, elapsed time, and drift summary.
- Smart Recovery shows the Today recovery card, sends local nudges after a long
  uninterrupted screen run, and records take/snooze/skip actions in recovery
  stats without blocking apps.
- Away/resume gaps create idle recovery prompts without surfacing sleep-sized
  gaps as classification work.
- Daily report, weekly digest, and replay/restore flows generate source-backed
  output from the expected local date range, including weekly recovery rhythm.
- Date-range export includes source-backed activity and AI contribution rows.

## Public README And Screenshots

- Root `README.md` explains the product clearly for a first-time user.
- All screenshots referenced by the README exist and render:
  `01-today.png`, `07-focus-mode.png`, `02-ai-impact.png`,
  `03-activity.png`, and `06-capture-health.png`.
- Focus Mode is documented with its current behavior: local distraction nudges,
  native notifications, duration choices, snooze/end controls, and no app
  blocking. Persisted focus timer/review behavior is also documented.
- Smart Recovery is documented as local sustainable-focus nudges with
  take/snooze/skip controls, not as medical or eye-care advice.
- README claims about calendar support are limited to imported/local events; do
  not imply Google/Outlook sync unless that connector is actually shipped.
- README mentions weekly digest, replay/restore, idle recovery, and
  interruption-friendly positioning consistently with the app UI.
- Screenshots use realistic demo data and do not expose secrets, private
  customers, private email addresses, internal IPs, or local-only paths.
- Platform status and known limitations are accurate for the release being
  published.
- Install, build, and verification commands in the README have been checked.

## macOS Distribution

- Build unsigned internal app/DMG.
- Sign with Developer ID Application.
- Notarize with Apple.
- Staple notarization ticket.
- Verify Gatekeeper on a clean macOS user account.

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
