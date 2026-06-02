# Smart Breaks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add optional, configurable Smart Breaks that nudge users after sustained input and fold break evidence into weekly review and docs without adding Today-screen noise.

**Architecture:** Store break events in SQLite as first-class local evidence, compute a deterministic summary from source events plus break events, and expose it through `today`, export, and small Tauri commands. Keep the user-facing controls in Settings and native notifications; do not add full-screen blocking, Today cards, or medical claims.

**Tech Stack:** Rust/Tauri + rusqlite + serde models, React 18 + TypeScript + existing CSS, Vitest and Cargo tests.

---

### Task 1: Recovery Models And Summary Tests

**Files:**
- Modify: `apps/desktop/src-tauri/src/models.rs`
- Modify: `apps/desktop/src-tauri/src/store.rs`
- Test: `apps/desktop/src-tauri/tests/core_behavior.rs`

- [x] Add failing Cargo test `smart_breaks_score_long_work_and_logged_breaks`.
- [x] Expected red result: missing `RecoveryEventInput` type and `recovery_summary` field.
- [x] Add `RecoveryEvent`, `RecoveryEventInput`, `RecoverySummary`, and `RecoveryPrompt` models using `camelCase` serde.
- [x] Add `recovery_events` table and indexes in `migrate`.
- [x] Implement `record_recovery_event`, `list_recovery_events_for_dates`, and deterministic `build_recovery_summary`.
- [x] Add `recovery_summary` to `TodaySnapshot` and `ExportPayload`.
- [x] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml smart_recovery`.

### Task 2: Smart Break Commands And Runtime Scheduler

**Files:**
- Create: `apps/desktop/src-tauri/src/recovery.rs`
- Create: `apps/desktop/src-tauri/src/commands/recovery.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/src/recovery.rs`

- [x] Add failing unit tests for prompt threshold, snooze gate, and skip limit behavior.
- [x] Implement a tiny in-memory scheduler with a configurable uninterrupted threshold, 5m snooze, max 3 skips per day, and no prompt while app is paused.
- [x] Register commands: `get_recovery_summary`, `record_recovery_event`, `snooze_recovery`, `skip_recovery`, `take_recovery_break`.
- [x] Wire watcher evaluation so recovery nudges use the same foreground metadata and notification plugin as Focus Mode.
- [x] Run focused recovery tests.

### Task 3: Settings UI And View Model Cleanup

**Files:**
- Modify: `apps/desktop/src/lib/viewModels/todayViewModel.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/styles.css`
- Test: `apps/desktop/tests/viewModels.test.ts`
- Test: `apps/desktop/tests/App.test.tsx`

- [x] Remove stale Today recovery score labels from `buildTodayView`.
- [x] Add React coverage that Smart Breaks live in Settings instead of the sidebar or Today screen.
- [x] Extend frontend snapshot types with `recoverySummary`.
- [x] Remove the compact recovery card and keep Smart Break controls in Settings.
- [x] Reuse existing `panel-block`, `report-settings-list`, and compact button styles; add only targeted CSS.
- [x] Run `npm --prefix apps/desktop run test -- --run`.

### Task 4: Weekly Review And Docs

**Files:**
- Modify: `apps/desktop/src-tauri/src/store.rs`
- Modify: `README.md`
- Modify: `docs/screenshots/README.md`

- [x] Add weekly review assertions for a `Smart Breaks` section.
- [x] Include Smart Break stats in deterministic weekly markdown and AI prompt context.
- [x] Update README feature bullets, What It Captures, How It Works, and Setup sections.
- [x] Update screenshots README to note a Smart Break Settings screenshot when regenerated.
- [x] Avoid medical guarantees; position Smart Breaks as sustainable focus.

### Task 5: Verification, Commit, Push, CI

**Files:**
- No new files unless tests reveal a needed fixture.

- [x] Run `npm --prefix apps/desktop run test -- --run`.
- [x] Run `npm --prefix apps/desktop run build`.
- [x] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets`.
- [x] Run `cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings`.
- [x] Run `npm run release:check`.
- [x] Do local UI smoke with a browser or dev server if needed.
- [ ] Commit with conventional message.
- [ ] Push to GitHub.
- [ ] Monitor GitHub Actions for macOS and Windows builds; fix and push until passing.
