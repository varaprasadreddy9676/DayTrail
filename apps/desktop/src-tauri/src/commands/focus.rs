use crate::focus::{self, FocusStatus, FocusSummary, StartFocusInput};
use tauri::State;

use crate::{
    error::CommandError,
    models::{FocusSessionInput, FocusSessionSummary},
    store::WorktraceStore,
};

#[tauri::command]
pub fn start_focus_session(input: StartFocusInput) -> FocusStatus {
    focus::start(input)
}

#[tauri::command]
pub fn end_focus_session() -> Option<FocusSummary> {
    focus::end()
}

#[tauri::command]
pub fn get_focus_session() -> Option<FocusStatus> {
    focus::snapshot()
}

#[tauri::command]
pub fn snooze_focus_session(minutes: u32) -> Option<FocusStatus> {
    focus::snooze(minutes)
}

#[tauri::command]
pub fn upsert_focus_session(
    store: State<'_, WorktraceStore>,
    input: FocusSessionInput,
) -> Result<FocusSessionSummary, CommandError> {
    store.upsert_focus_session(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_focus_sessions(
    store: State<'_, WorktraceStore>,
    from_date: Option<String>,
    to_date: Option<String>,
) -> Result<Vec<FocusSessionSummary>, CommandError> {
    store
        .list_focus_sessions_for_dates(from_date.as_deref(), to_date.as_deref())
        .map_err(Into::into)
}
