use tauri::State;

use crate::{error::CommandError, models::PlanningOutput, store::WorktraceStore};

#[tauri::command]
pub fn generate_morning_plan(
    store: State<'_, WorktraceStore>,
) -> Result<PlanningOutput, CommandError> {
    store.generate_morning_plan().map_err(Into::into)
}

#[tauri::command]
pub fn generate_weekly_plan(
    store: State<'_, WorktraceStore>,
) -> Result<PlanningOutput, CommandError> {
    store.generate_weekly_plan().map_err(Into::into)
}
