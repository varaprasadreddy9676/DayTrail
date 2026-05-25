use tauri::State;

use crate::{
    error::CommandError,
    models::{LoopAction, LoopActionInput, MenuBarSummary, ProjectContext, TodaySnapshot},
    project_detection::{default_project_sources, detect_project_from_sources},
    store::WorktraceStore,
};

#[tauri::command]
pub fn today(store: State<'_, WorktraceStore>) -> Result<TodaySnapshot, CommandError> {
    store.today_snapshot().map_err(Into::into)
}

#[tauri::command]
pub fn menu_bar_summary(store: State<'_, WorktraceStore>) -> Result<MenuBarSummary, CommandError> {
    store
        .today_snapshot()
        .map(|snapshot| snapshot.menu_bar_summary)
        .map_err(Into::into)
}

#[tauri::command]
pub fn record_loop_action(
    store: State<'_, WorktraceStore>,
    input: LoopActionInput,
) -> Result<LoopAction, CommandError> {
    store.record_loop_action(input).map_err(Into::into)
}

#[tauri::command]
pub fn detect_project_context() -> Result<ProjectContext, CommandError> {
    detect_project_from_sources(default_project_sources()).map_err(Into::into)
}
