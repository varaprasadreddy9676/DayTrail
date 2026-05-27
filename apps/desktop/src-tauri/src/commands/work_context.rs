use tauri::State;

use crate::{
    error::CommandError,
    models::{ActiveWorkContext, ActiveWorkContextInput},
    store::WorktraceStore,
};

#[tauri::command]
pub fn get_active_work_context(
    store: State<'_, WorktraceStore>,
) -> Result<Option<ActiveWorkContext>, CommandError> {
    store.get_active_work_context().map_err(Into::into)
}

#[tauri::command]
pub fn set_active_work_context(
    store: State<'_, WorktraceStore>,
    input: ActiveWorkContextInput,
) -> Result<ActiveWorkContext, CommandError> {
    store.set_active_work_context(input).map_err(Into::into)
}

#[tauri::command]
pub fn clear_active_work_context(store: State<'_, WorktraceStore>) -> Result<(), CommandError> {
    store.clear_active_work_context().map_err(Into::into)
}
