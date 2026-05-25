use tauri::State;

use crate::{
    error::CommandError,
    models::{AgentRun, AgentRunInput, LoopRisk},
    store::WorktraceStore,
};

#[tauri::command]
pub fn record_agent_run(
    store: State<'_, WorktraceStore>,
    input: AgentRunInput,
) -> Result<AgentRun, CommandError> {
    store.record_agent_run(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_loop_risks(store: State<'_, WorktraceStore>) -> Result<Vec<LoopRisk>, CommandError> {
    store.detect_loop_risks().map_err(Into::into)
}
