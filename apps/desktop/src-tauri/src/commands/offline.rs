use tauri::State;

use crate::{
    error::CommandError,
    models::{FieldVisit, FieldVisitInput, IdleBlock, IdleBlockInput, Meeting, MeetingInput},
    store::WorktraceStore,
};

#[tauri::command]
pub fn upsert_meeting(
    store: State<'_, WorktraceStore>,
    input: MeetingInput,
) -> Result<Meeting, CommandError> {
    store.upsert_meeting(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_meetings(
    store: State<'_, WorktraceStore>,
    limit: Option<usize>,
) -> Result<Vec<Meeting>, CommandError> {
    store
        .list_meetings(limit.unwrap_or(100))
        .map_err(Into::into)
}

#[tauri::command]
pub fn upsert_field_visit(
    store: State<'_, WorktraceStore>,
    input: FieldVisitInput,
) -> Result<FieldVisit, CommandError> {
    store.upsert_field_visit(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_field_visits(
    store: State<'_, WorktraceStore>,
    limit: Option<usize>,
) -> Result<Vec<FieldVisit>, CommandError> {
    store
        .list_field_visits(limit.unwrap_or(100))
        .map_err(Into::into)
}

#[tauri::command]
pub fn upsert_idle_block(
    store: State<'_, WorktraceStore>,
    input: IdleBlockInput,
) -> Result<IdleBlock, CommandError> {
    store.upsert_idle_block(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_idle_blocks(
    store: State<'_, WorktraceStore>,
    limit: Option<usize>,
) -> Result<Vec<IdleBlock>, CommandError> {
    store
        .list_idle_blocks(limit.unwrap_or(100))
        .map_err(Into::into)
}
