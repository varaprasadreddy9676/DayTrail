use tauri::State;

use crate::{
    error::CommandError,
    models::{ChatMessage, ChatResponse},
    store::WorktraceStore,
};

#[tauri::command]
pub fn chat_query(
    message: String,
    history: Vec<ChatMessage>,
    store: State<'_, WorktraceStore>,
) -> Result<ChatResponse, CommandError> {
    store.handle_chat_query(&message, &history).map_err(Into::into)
}
