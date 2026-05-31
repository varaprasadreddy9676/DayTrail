use crate::focus::{self, FocusStatus, FocusSummary, StartFocusInput};

#[tauri::command]
pub fn start_focus_session(input: StartFocusInput) -> FocusStatus {
    focus::start(input)
}

#[tauri::command]
pub fn end_focus_session() -> Option<FocusSummary> {
    focus::end()
}

#[tauri::command]
pub fn get_focus_session() -> Option<FocusStatus> {
    focus::snapshot()
}

#[tauri::command]
pub fn snooze_focus_session(minutes: u32) -> Option<FocusStatus> {
    focus::snooze(minutes)
}
