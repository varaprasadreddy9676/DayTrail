use tauri::State;

use crate::{
    error::CommandError,
    models::{PrivacyDeleteSummary, Task, TaskDraft, TaskInput, TaskStatus},
    store::WorktraceStore,
};

#[tauri::command]
pub fn list_tasks(
    store: State<'_, WorktraceStore>,
    status: Option<TaskStatus>,
) -> Result<Vec<Task>, CommandError> {
    store.list_tasks(status).map_err(Into::into)
}

#[tauri::command]
pub fn create_task(
    store: State<'_, WorktraceStore>,
    input: TaskInput,
) -> Result<Task, CommandError> {
    store.create_task(input).map_err(Into::into)
}

#[tauri::command]
pub fn draft_tasks_from_text(
    store: State<'_, WorktraceStore>,
    text: String,
    default_priority: Option<String>,
) -> Result<Vec<TaskDraft>, CommandError> {
    store
        .draft_tasks_from_text(&text, default_priority)
        .map_err(Into::into)
}

#[tauri::command]
pub fn complete_task(store: State<'_, WorktraceStore>, id: i64) -> Result<Task, CommandError> {
    store.complete_task(id).map_err(Into::into)
}

#[tauri::command]
pub fn snooze_task(
    store: State<'_, WorktraceStore>,
    id: i64,
    due_at: i64,
) -> Result<Task, CommandError> {
    store.snooze_task(id, due_at).map_err(Into::into)
}

#[tauri::command]
pub fn delete_task(
    store: State<'_, WorktraceStore>,
    id: i64,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store.delete_task(id).map_err(Into::into)
}
