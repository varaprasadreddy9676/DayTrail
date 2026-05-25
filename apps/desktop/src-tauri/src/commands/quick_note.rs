use tauri::State;

use crate::{
    error::CommandError,
    models::{PrivacyDeleteSummary, QuickNote},
    store::WorktraceStore,
};

#[tauri::command]
pub fn add_quick_note(
    store: State<'_, WorktraceStore>,
    body: String,
    source: Option<String>,
    project_path: Option<String>,
) -> Result<QuickNote, CommandError> {
    store
        .add_quick_note(&body, source.as_deref(), project_path.as_deref())
        .map_err(Into::into)
}

#[tauri::command]
pub fn delete_quick_note(
    store: State<'_, WorktraceStore>,
    id: i64,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store.delete_quick_note(id).map_err(Into::into)
}
