use tauri::State;

use crate::{
    error::CommandError,
    models::{TaskActivitySummary, TaskLinkSuggestion},
    store::WorktraceStore,
};

#[tauri::command]
pub fn get_task_activity_summary(
    store: State<'_, WorktraceStore>,
    task_id: i64,
) -> Result<TaskActivitySummary, CommandError> {
    store.get_task_activity_summary(task_id).map_err(Into::into)
}

#[tauri::command]
pub fn suggest_task_links(
    store: State<'_, WorktraceStore>,
    task_id: i64,
    limit: Option<usize>,
) -> Result<Vec<TaskLinkSuggestion>, CommandError> {
    store
        .suggest_task_links(task_id, limit.unwrap_or(10))
        .map_err(Into::into)
}
