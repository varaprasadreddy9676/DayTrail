# Manual Time Blocks Design

## Goal

Let users correct or explain parts of the captured day by marking one or more timeline hours as work, meeting, break, personal time, or ignored time.

## Product Behavior

- The Day tracker supports single-hour selection, command/control-click multi-selection, and shift-click range selection.
- Right-clicking a selected hour opens a context menu action named "Mark selected time".
- Marking time opens a generic modal with type, client, project, task/notes, ticket/issue, and billable fields.
- Saved manual context appears in the hour breakdown before raw app evidence.
- Saved manual context appears in selected-hour summaries and app breakdowns as context active for that time.
- Saved manual context can be edited or cleared from the hour breakdown and app breakdown.
- DayTrail keeps raw capture data intact; manual blocks explain the data instead of overwriting it.
- When a long idle gap is detected and not classified, DayTrail prompts the user to mark it as meeting, break, offline work, or ignored.

## Data Approach

Manual time blocks are stored through the existing `idle_blocks` table and command path. The `category` field stores the visible block type. `evidence_json` stores optional user-entered context fields as JSON, including client, project, task, ticket, billable, and source.

## UI Approach

The app uses one reusable "Mark time" modal for timeline selections, sidebar offline logging, and idle recovery. Timeline rows show selected state and manual context labels. Hour details render manual context above app rows so a user first sees what they said the time meant, then sees captured evidence underneath.

## Testing

Verification should include TypeScript build, Rust tests, release check, and an installed-app smoke test. Manual QA should cover command-click selection, shift-click range selection, saving a meeting block, viewing it in the hour breakdown, and dismissing/marking an idle prompt.
