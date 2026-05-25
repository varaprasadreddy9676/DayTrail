use tauri::State;

use crate::{error::CommandError, models::WorkMemorySummary, store::WorktraceStore};

#[tauri::command]
pub fn materialize_work_memory(
    store: State<'_, WorktraceStore>,
) -> Result<WorkMemorySummary, CommandError> {
    store.materialize_work_memory().map_err(Into::into)
}
