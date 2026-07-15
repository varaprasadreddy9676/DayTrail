use crate::focus::{self, FocusStatus, FocusSummary, StartFocusInput};
use crate::focus_audio::{self, FocusMusicTrack};
use tauri::State;

use crate::{
    error::CommandError,
    models::{FocusSessionInput, FocusSessionSummary},
    store::WorktraceStore,
};

#[tauri::command]
pub fn start_focus_session(input: StartFocusInput) -> FocusStatus {
    focus::start(input)
}

#[tauri::command]
pub fn end_focus_session() -> Option<FocusSummary> {
    focus::end()
}

#[tauri::command]
pub fn list_focus_music(store: State<'_, WorktraceStore>) -> Result<Vec<FocusMusicTrack>, CommandError> {
    let custom_dir = store.get_settings()?.focus_music_dir;
    Ok(focus_audio::list_tracks(custom_dir.as_deref()))
}

#[tauri::command]
pub fn start_focus_music(
    store: State<'_, WorktraceStore>,
    track_id: String,
    volume: Option<f32>,
) -> Result<Option<FocusMusicTrack>, CommandError> {
    let custom_dir = store.get_settings()?.focus_music_dir;
    Ok(focus_audio::play(
        &track_id,
        volume.unwrap_or(focus_audio::DEFAULT_VOLUME),
        custom_dir.as_deref(),
    ))
}

#[tauri::command]
pub fn stop_focus_music() {
    focus_audio::stop();
}

#[tauri::command]
pub fn set_focus_music_volume(volume: f32) {
    focus_audio::set_volume(volume);
}

#[tauri::command]
pub fn focus_music_status() -> Option<FocusMusicTrack> {
    focus_audio::now_playing()
}

#[tauri::command]
pub fn get_focus_session() -> Option<FocusStatus> {
    focus::snapshot()
}

#[tauri::command]
pub fn snooze_focus_session(minutes: u32) -> Option<FocusStatus> {
    focus::snooze(minutes)
}

#[tauri::command]
pub fn upsert_focus_session(
    store: State<'_, WorktraceStore>,
    input: FocusSessionInput,
) -> Result<FocusSessionSummary, CommandError> {
    store.upsert_focus_session(input).map_err(Into::into)
}

#[tauri::command]
pub fn list_focus_sessions(
    store: State<'_, WorktraceStore>,
    from_date: Option<String>,
    to_date: Option<String>,
) -> Result<Vec<FocusSessionSummary>, CommandError> {
    store
        .list_focus_sessions_for_dates(from_date.as_deref(), to_date.as_deref())
        .map_err(Into::into)
}
