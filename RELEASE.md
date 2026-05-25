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
- Date-range export includes source-backed activity and AI contribution rows.

## macOS Distribution

- Build unsigned internal app/DMG.
- Sign with Developer ID Application.
- Notarize with Apple.
- Staple notarization ticket.
- Verify Gatekeeper on a clean macOS user account.

## Windows Distribution

- Build NSIS/MSI artifacts on Windows.
- Sign executable and installer artifacts.
- Install on a clean Windows account.
- Verify foreground-window capture, Credential Manager API key storage, startup launch, tray controls, browser native host registration, and PowerShell terminal bridge.

## Release Artifacts

- Include checksums for every installer/archive.
- Attach release notes listing supported OS status and known limitations.
- Do not publish local databases, generated test output, or unsigned artifacts as production builds.

