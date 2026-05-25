use tauri::State;

use crate::{error::CommandError, models::BrowserBridgeEvent, store::WorktraceStore};

#[tauri::command]
pub fn ingest_browser_event(
    store: State<'_, WorktraceStore>,
    event: BrowserBridgeEvent,
) -> Result<bool, CommandError> {
    store.ingest_browser_event(event).map_err(Into::into)
}
