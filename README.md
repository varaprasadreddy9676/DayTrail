# DayTrail

DayTrail is a local-first desktop work memory app for people and teams who need to understand where work went, where AI was used, and what still needs closure.

It tracks metadata from active apps, browser tabs, editors, terminals, local notes, AI-assisted reports, and exportable activity rows. The product goal is to answer: what am I working on now, what did I do today, which projects/apps used AI, and what evidence backs that summary?

## Current Status

This repository is public-source ready for review and local builds. macOS has been exercised manually with the installed Tauri app. Windows support is implemented for foreground-window capture, Credential Manager API-key storage, startup launch, browser native-host registration, and PowerShell terminal bridge, but it still needs real Windows machine QA before a signed public Windows release.

The requirements files in `docs/` are product targets, not a claim that every requirement is complete in the current build.

## What It Captures

- Active app/window metadata and foreground project context.
- Browser domain, title, and redacted URL through the browser bridge.
- VS Code/Cursor editor project metadata through the editor bridge.
- Terminal current folder and redacted last command through shell hooks.
- AI tool usage signals for ChatGPT, Claude, Codex, Copilot, Gemini, Cursor, Cline, Aider, Continue, and similar tools when detectable from app/browser/editor metadata.
- Local scratchpad notes, commitments, loop risks, reports, and export payloads.

Screenshots and full clipboard contents are off by default. The app is designed around metadata-first capture.

## Repository Layout

- `apps/desktop/` - React/Vite desktop UI.
- `apps/desktop/src-tauri/` - Tauri v2 Rust desktop shell and local SQLite store.
- `apps/browser-extension/` - Browser bridge for tab metadata.
- `apps/vscode-extension/` - VS Code/Cursor editor bridge.
- `crates/` - Rust domain crates for privacy, AI context, sessions, reports, and loops.
- `scripts/` - Packaging, bridge installation, and release checks.
- `docs/` - Product and technical requirements backlog.

## Requirements

- Node.js 20 or newer.
- Rust stable.
- Tauri v2 prerequisites for your OS.
- macOS: Accessibility permission is required for reliable app/window title capture.
- Windows: WebView2 runtime is required by Tauri; PowerShell is used for the terminal bridge installer.

## Development

Install desktop dependencies:

```bash
npm ci --prefix apps/desktop
```

Run the desktop UI in development:

```bash
npm --prefix apps/desktop run dev
```

Run Rust checks:

```bash
cargo test --workspace --all-targets
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets
```

Run the full local release check:

```bash
npm run release:check
```

## Building

Build the frontend:

```bash
npm --prefix apps/desktop run build
```

Build the Tauri shell:

```bash
cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --release
```

Build an unsigned macOS app/DMG for internal testing:

```bash
npm run desktop:dmg
```

Production distribution still requires platform signing, notarization on macOS, Windows code signing, and release artifact checksums.

## Browser Bridge

macOS/Linux native-host install:

```bash
CHROME_EXTENSION_ID=<installed-extension-id> scripts/install-browser-host.sh
```

Windows native-host install:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install-browser-host.ps1 -ChromeExtensionId <installed-extension-id> -AppBin "C:\Path\To\DayTrail.exe"
```

## Terminal Bridge

zsh:

```bash
scripts/worktrace-terminal-bridge.sh --print-zsh-hook >> ~/.zshrc
```

bash:

```bash
scripts/worktrace-terminal-bridge.sh --print-bash-hook >> ~/.bashrc
```

PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\worktrace-terminal-bridge.ps1 -PrintProfileSnippet >> $PROFILE
```

## AI Providers

Settings support Ollama Local, LM Studio, OpenAI-compatible endpoints, OpenAI, OpenRouter, Groq, Gemini, Anthropic, and custom APIs. API keys are stored in the OS keychain where supported: macOS Keychain, Linux Secret Service, and Windows Credential Manager.

Gemini endpoints are tied to the selected Gemini model so the UI cannot show a Gemini provider with an unrelated model endpoint.

## Data and Exports

Data is stored locally in SQLite under the OS app-data location. The export view produces raw JSON for a selected date range, observed activity rows, AI contribution rows, app usage summaries, and source evidence. Exported activity rows are source-backed drafts, not billing-approved timesheets.

## Security and Privacy

See `PRIVACY.md` and `SECURITY.md`.

Do not commit real API keys, local databases, generated bundles, or captured user data. The repository includes secret scanning and dependency audit workflows.

## License

Licensed under MIT OR Apache-2.0. See `LICENSE`, `LICENSE-MIT`, and `LICENSE-APACHE`.

