use tauri::State;

use crate::{
    error::CommandError,
    models::{Task, TaskInput, TaskStatus},
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
pub fn complete_task(store: State<'_, WorktraceStore>, id: i64) -> Result<Task, CommandError> {
    store.complete_task(id).map_err(Into::into)
}
