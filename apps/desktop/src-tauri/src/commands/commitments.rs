use tauri::State;

use crate::{
    error::CommandError,
    models::{Commitment, CommitmentInput},
    store::WorktraceStore,
};

#[tauri::command]
pub fn create_commitment(
    store: State<'_, WorktraceStore>,
    input: CommitmentInput,
) -> Result<Commitment, CommandError> {
    store.create_commitment(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_open_commitments(
    store: State<'_, WorktraceStore>,
    limit: Option<usize>,
) -> Result<Vec<Commitment>, CommandError> {
    store
        .list_open_commitments(limit.unwrap_or(100))
        .map_err(Into::into)
}
