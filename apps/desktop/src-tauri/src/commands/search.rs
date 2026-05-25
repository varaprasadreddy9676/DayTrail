use tauri::State;

use crate::{error::CommandError, models::SearchResult, store::WorktraceStore};

#[tauri::command]
pub fn search_work_memory(
    store: State<'_, WorktraceStore>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, CommandError> {
    store
        .search_work_memory(&query, limit.unwrap_or(12))
        .map_err(Into::into)
}
