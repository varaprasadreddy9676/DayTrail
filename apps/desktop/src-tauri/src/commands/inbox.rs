use tauri::State;

use crate::{
    error::CommandError,
    models::{EmailThread, EmailThreadInput},
    store::WorktraceStore,
};

#[tauri::command]
pub fn upsert_email_thread(
    store: State<'_, WorktraceStore>,
    input: EmailThreadInput,
) -> Result<EmailThread, CommandError> {
    store.upsert_email_thread(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_pending_replies(
    store: State<'_, WorktraceStore>,
    limit: Option<usize>,
) -> Result<Vec<EmailThread>, CommandError> {
    store
        .list_pending_replies(limit.unwrap_or(100))
        .map_err(Into::into)
}
