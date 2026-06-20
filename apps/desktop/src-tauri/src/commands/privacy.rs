use tauri::State;

use crate::{error::CommandError, models::PrivacyDeleteSummary, store::WorktraceStore};

#[tauri::command]
pub fn clear_clipboard_history(
    store: State<'_, WorktraceStore>,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store.clear_clipboard_history().map_err(Into::into)
}

#[tauri::command]
pub fn delete_context_data(
    store: State<'_, WorktraceStore>,
    context_id: String,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store.delete_context_data(&context_id).map_err(Into::into)
}

#[tauri::command]
pub fn purge_captured_data(
    store: State<'_, WorktraceStore>,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store.purge_captured_data().map_err(Into::into)
}

#[tauri::command]
pub fn prune_captured_data(
    store: State<'_, WorktraceStore>,
    days: i64,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store
        .prune_captured_data_older_than_days(days)
        .map_err(Into::into)
}


#[tauri::command]
pub fn prune_completed_tasks(
    store: State<'_, WorktraceStore>,
    days: i64,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store
        .prune_completed_tasks_older_than_days(days)
        .map_err(Into::into)
}

#[tauri::command]
pub fn apply_retention_policy(
    store: State<'_, WorktraceStore>,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store.apply_retention_policy().map_err(Into::into)
}
