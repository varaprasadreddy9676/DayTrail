# Premium Notification Island Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add optional DayTrail premium notifications that render as a compact Dynamic Island-style overlay with configurable sound while preserving native notification fallback.

**Architecture:** Rust routes DayTrail-originated notifications through one helper that reads local settings, emits a typed frontend event when the main window can show the island, and falls back to native OS notifications otherwise. React listens for that event, renders a single accessible island component, and plays a short Web Audio chime when enabled. Existing focus, recovery, away, task reminder, and proactive insight notifications call the shared helper.

**Tech Stack:** Tauri v2 event emitter, Tauri notification plugin, React state/effects, CSS animations, Web Audio API, SQLite-backed settings.

---

### Task 1: Persist Notification Preferences

**Files:**
- Modify: `apps/desktop/src-tauri/src/models.rs`
- Modify: `apps/desktop/src-tauri/src/store.rs`
- Modify: `apps/desktop/src/App.tsx`

- [x] **Step 1: Add settings fields**

Add `premium_notifications_enabled: bool` and `notification_sound: String` to `Settings`, with default values `false` and `daytrail`.

- [x] **Step 2: Add patch fields**

Add `premium_notifications_enabled: Option<bool>` and `notification_sound: Option<String>` to `SettingsPatch`.

- [x] **Step 3: Read settings from storage**

Teach `get_settings` to parse `premium_notifications_enabled` and `notification_sound`.

- [x] **Step 4: Validate writes**

Teach `update_settings` to save `premium_notifications_enabled` and validate `notification_sound` as one of `daytrail`, `glass`, `subtle`, `none`.

### Task 2: Centralize DayTrail Notifications

**Files:**
- Create: `apps/desktop/src-tauri/src/daytrail_notification.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/focus.rs`
- Modify: `apps/desktop/src-tauri/src/recovery.rs`
- Modify: `apps/desktop/src-tauri/src/active_window.rs`

- [x] **Step 1: Add typed payload**

Create a serializable `DaytrailNotificationPayload` with `id`, `kind`, `title`, `body`, `sound`, `created_at_ms`, and `ttl_ms`.

- [x] **Step 2: Emit island events**

When premium notifications are enabled and the main webview is visible, emit `daytrail-notification` to the frontend.

- [x] **Step 3: Native fallback**

Always use native notification fallback when the island is disabled, hidden, or emit fails. Honor `notification_sound = none` by skipping sound.

- [x] **Step 4: Route existing notifications**

Replace direct `app.notification().builder()` calls for focus, recovery, away, task reminders, and proactive insights with the shared helper.

### Task 3: Render The Island

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/styles.css`

- [x] **Step 1: Listen for backend events**

Add a `daytrail-notification` event listener, keep one active island at a time, and auto-dismiss after the payload TTL.

- [x] **Step 2: Play configurable sound**

Use Web Audio for `daytrail` and `subtle`; skip playback for `none`; let native fallback keep using macOS system sounds for `glass`.

- [x] **Step 3: Build accessible UI**

Render a compact top-center island with DayTrail icon, title, message, close button, glow, and reduced-motion handling.

- [x] **Step 4: Add settings controls**

In Settings → Capture Health, add controls for native/premium notification style and sound choice.

### Task 4: Verify

**Files:**
- Test: `apps/desktop/src-tauri/src/store.rs`
- Test: `apps/desktop/tests/App.test.tsx`

- [x] **Step 1: Add focused tests where practical**

Cover settings persistence/validation and ensure the app test suite still renders.

- [x] **Step 2: Run checks**

Run `npm --prefix apps/desktop run check`, `npm --prefix apps/desktop test -- --run`, and focused Rust tests for settings/notification-related code.
