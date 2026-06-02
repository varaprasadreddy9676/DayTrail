use tauri::State;

use crate::{
    error::CommandError,
    models::{RecoveryEvent, RecoveryEventInput, RecoverySummary},
    recovery,
    store::WorktraceStore,
};

#[tauri::command]
pub fn record_recovery_event(
    store: State<'_, WorktraceStore>,
    input: RecoveryEventInput,
) -> Result<RecoveryEvent, CommandError> {
    recovery::record_event(&store, input).map_err(Into::into)
}

#[tauri::command]
pub fn get_recovery_summary(
    store: State<'_, WorktraceStore>,
    from_date: Option<String>,
    to_date: Option<String>,
) -> Result<RecoverySummary, CommandError> {
    recovery::summary(&store, from_date.as_deref(), to_date.as_deref()).map_err(Into::into)
}

#[tauri::command]
pub fn list_recovery_events(
    store: State<'_, WorktraceStore>,
    from_date: Option<String>,
    to_date: Option<String>,
) -> Result<Vec<RecoveryEvent>, CommandError> {
    store
        .list_recovery_events_for_dates(from_date.as_deref(), to_date.as_deref())
        .map_err(Into::into)
}

#[tauri::command]
pub fn snooze_recovery(
    store: State<'_, WorktraceStore>,
    minutes: Option<u32>,
) -> Result<RecoveryEvent, CommandError> {
    recovery::snooze(&store, minutes.unwrap_or(5)).map_err(Into::into)
}

#[tauri::command]
pub fn skip_recovery(store: State<'_, WorktraceStore>) -> Result<RecoveryEvent, CommandError> {
    recovery::skip(&store).map_err(Into::into)
}

#[tauri::command]
pub fn take_recovery_break(
    store: State<'_, WorktraceStore>,
    minutes: Option<u32>,
) -> Result<RecoveryEvent, CommandError> {
    recovery::take_break(&store, minutes.unwrap_or(3)).map_err(Into::into)
}
