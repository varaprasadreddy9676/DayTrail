use tauri::State;

use crate::{
    error::CommandError,
    models::{
        ReturnMarker, ScratchpadNote, ScratchpadNoteInput, StateSnapshot, StateSnapshotInput,
    },
    store::WorktraceStore,
};

#[tauri::command]
pub fn add_scratchpad_note(
    store: State<'_, WorktraceStore>,
    input: ScratchpadNoteInput,
) -> Result<ScratchpadNote, CommandError> {
    store.add_scratchpad_note(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_scratchpad_notes(
    store: State<'_, WorktraceStore>,
    context_id: String,
    limit: Option<usize>,
) -> Result<Vec<ScratchpadNote>, CommandError> {
    store
        .list_scratchpad_notes(&context_id, limit.unwrap_or(50))
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_state_snapshot(
    store: State<'_, WorktraceStore>,
    input: StateSnapshotInput,
) -> Result<StateSnapshot, CommandError> {
    store.create_state_snapshot(input).map_err(Into::into)
}

#[tauri::command]
pub fn get_return_marker(
    store: State<'_, WorktraceStore>,
    context_id: String,
) -> Result<ReturnMarker, CommandError> {
    store.get_return_marker(&context_id).map_err(Into::into)
}
