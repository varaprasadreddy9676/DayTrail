use tauri::State;

use crate::{
    error::CommandError,
    models::{ExportPayload, ExportRangeInput, ReportOutput},
    store::WorktraceStore,
};

#[tauri::command]
pub fn export_data(store: State<'_, WorktraceStore>) -> Result<ExportPayload, CommandError> {
    store.export_data().map_err(Into::into)
}

#[tauri::command]
pub fn export_data_range(
    store: State<'_, WorktraceStore>,
    range: ExportRangeInput,
) -> Result<ExportPayload, CommandError> {
    store.export_data_range(range).map_err(Into::into)
}

#[tauri::command]
pub fn analyze_export_range(
    store: State<'_, WorktraceStore>,
    range: ExportRangeInput,
) -> Result<ReportOutput, CommandError> {
    store.analyze_export_range(range).map_err(Into::into)
}
