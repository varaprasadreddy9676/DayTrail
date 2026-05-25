use tauri::State;

use crate::{
    error::CommandError,
    models::{AiUsage, AiUsageInput, WorkOutput, WorkOutputInput},
    store::WorktraceStore,
};

#[tauri::command]
pub fn record_ai_usage(
    store: State<'_, WorktraceStore>,
    input: AiUsageInput,
) -> Result<AiUsage, CommandError> {
    store.record_ai_usage(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_ai_usage(
    store: State<'_, WorktraceStore>,
    limit: Option<usize>,
) -> Result<Vec<AiUsage>, CommandError> {
    store
        .list_ai_usage(limit.unwrap_or(100))
        .map_err(Into::into)
}

#[tauri::command]
pub fn record_work_output(
    store: State<'_, WorktraceStore>,
    input: WorkOutputInput,
) -> Result<WorkOutput, CommandError> {
    store.record_work_output(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_work_outputs(
    store: State<'_, WorktraceStore>,
    limit: Option<usize>,
) -> Result<Vec<WorkOutput>, CommandError> {
    store
        .list_work_outputs(limit.unwrap_or(100))
        .map_err(Into::into)
}
