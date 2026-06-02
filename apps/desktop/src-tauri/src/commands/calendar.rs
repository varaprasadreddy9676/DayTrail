use tauri::State;

use crate::{
    error::CommandError,
    models::{CalendarEvent, CalendarEventInput},
    store::WorktraceStore,
};

#[tauri::command]
pub fn upsert_calendar_event(
    store: State<'_, WorktraceStore>,
    input: CalendarEventInput,
) -> Result<CalendarEvent, CommandError> {
    store.upsert_calendar_event(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_calendar_events(
    store: State<'_, WorktraceStore>,
    from_date: Option<String>,
    to_date: Option<String>,
) -> Result<Vec<CalendarEvent>, CommandError> {
    store
        .list_calendar_events_for_dates(from_date.as_deref(), to_date.as_deref())
        .map_err(Into::into)
}
