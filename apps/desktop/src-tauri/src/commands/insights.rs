use tauri::State;

use crate::{error::CommandError, models::ProactiveInsight, store::WorktraceStore};

#[tauri::command]
pub fn list_proactive_insights(
    store: State<'_, WorktraceStore>,
) -> Result<Vec<ProactiveInsight>, CommandError> {
    store.list_proactive_insights().map_err(Into::into)
}

#[tauri::command]
pub fn dismiss_insight(id: i64, store: State<'_, WorktraceStore>) -> Result<(), CommandError> {
    store.dismiss_insight(id).map_err(Into::into)
}

#[tauri::command]
pub fn mark_insights_seen(store: State<'_, WorktraceStore>) -> Result<(), CommandError> {
    store.mark_insights_seen().map_err(Into::into)
}

#[tauri::command]
pub fn count_unseen_insights(store: State<'_, WorktraceStore>) -> Result<i64, CommandError> {
    store.count_unseen_insights().map_err(Into::into)
}
