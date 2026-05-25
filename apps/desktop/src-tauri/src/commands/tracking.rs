use tauri::State;

use crate::{error::CommandError, models::PauseState, store::WorktraceStore};

#[tauri::command]
pub fn pause_tracking(
    store: State<'_, WorktraceStore>,
    reason: Option<String>,
) -> Result<PauseState, CommandError> {
    store
        .pause(reason.as_deref().unwrap_or("manual"))
        .map_err(Into::into)
}

#[tauri::command]
pub fn resume_tracking(store: State<'_, WorktraceStore>) -> Result<PauseState, CommandError> {
    store.resume().map_err(Into::into)
}
