# Manual Time Blocks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add multi-hour timeline selection, manual time marking, breakdown visibility, and idle-return recovery prompts.

**Architecture:** Reuse `idle_blocks` as the persisted manual time block model, with structured context in `evidence_json`. Add frontend helpers to derive blocks from the today snapshot and attach them to hour buckets for timeline, selected-hour, and hour-detail display.

**Tech Stack:** React 18, TypeScript, Tauri command bridge, Rust SQLite store.

---

### Task 1: Manual Block Types And Persistence

**Files:**
- Modify: `apps/desktop/src/App.tsx`

- [ ] Extend frontend idle block typing to include time, category, and evidence JSON.
- [ ] Add helpers that parse manual block context safely and format it for display.
- [ ] Add a `saveManualTimeBlock` path that calls `upsert_idle_block`.
- [ ] Add a `delete_idle_block` command so users can clear mistaken manual blocks.

### Task 2: Timeline Selection

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/styles.css`

- [ ] Track selected hours in `TodayView`.
- [ ] Support click, command/control-click, and shift-click selection in `HourlyTimelinePanel`.
- [ ] Replace "Set current task from this hour" with "Mark selected time".
- [ ] Show selected rows clearly without changing captured data.

### Task 3: Context Visibility

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/styles.css`

- [ ] Attach manual blocks to hour buckets.
- [ ] Show manual block summaries in the selected-hour panel.
- [ ] Show manual context before app evidence in the full hour breakdown.
- [ ] Show manual context inside app breakdown modal when relevant.
- [ ] Add Edit and Clear actions to manual context rows.

### Task 4: Idle Recovery Prompt

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/styles.css`

- [ ] Detect long unclassified idle gaps from hour buckets.
- [ ] Show one in-app recovery prompt when the user returns.
- [ ] Let the user classify the gap through the same mark-time modal or ignore it.

### Task 5: Verification

**Commands:**
- `npm --prefix apps/desktop run check`
- `npm run desktop:test`
- `npm run release:check`

**Manual QA:**
- Select one hour and save a meeting block.
- Command/control-click multiple hours and save a work block.
- Shift-click a contiguous range and save a break block.
- Open the hour breakdown and confirm manual context appears above captured apps.
- Leave an idle gap, return, and classify or ignore the prompt.
