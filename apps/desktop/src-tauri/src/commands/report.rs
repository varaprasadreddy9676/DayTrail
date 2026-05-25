use tauri::State;

use crate::{error::CommandError, models::ReportOutput, store::WorktraceStore};

#[tauri::command]
pub fn generate_daily_report(
    store: State<'_, WorktraceStore>,
) -> Result<ReportOutput, CommandError> {
    store.generate_daily_report().map_err(Into::into)
}

#[tauri::command]
pub fn generate_weekly_review(
    store: State<'_, WorktraceStore>,
) -> Result<ReportOutput, CommandError> {
    store.generate_weekly_review().map_err(Into::into)
}
