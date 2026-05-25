use tauri::State;

use crate::{
    error::CommandError,
    models::{DatabaseTransferResult, Settings, SettingsPatch, StorageLocationInfo},
    store::WorktraceStore,
};

#[tauri::command]
pub fn get_settings(store: State<'_, WorktraceStore>) -> Result<Settings, CommandError> {
    store.get_settings().map_err(Into::into)
}

#[tauri::command]
pub fn update_settings(
    store: State<'_, WorktraceStore>,
    patch: SettingsPatch,
) -> Result<Settings, CommandError> {
    store.update_settings(patch).map_err(Into::into)
}

#[tauri::command]
pub fn set_ai_api_key(
    store: State<'_, WorktraceStore>,
    provider: String,
    api_key: String,
) -> Result<Settings, CommandError> {
    store
        .set_ai_api_key(&provider, &api_key)
        .map_err(Into::into)
}

#[tauri::command]
pub fn get_storage_locations(
    store: State<'_, WorktraceStore>,
) -> Result<StorageLocationInfo, CommandError> {
    store.storage_locations().map_err(Into::into)
}

#[tauri::command]
pub fn export_settings_config(store: State<'_, WorktraceStore>) -> Result<String, CommandError> {
    store.export_settings_config_json().map_err(Into::into)
}

#[tauri::command]
pub fn import_settings_config(
    store: State<'_, WorktraceStore>,
    config_json: String,
) -> Result<Settings, CommandError> {
    store
        .import_settings_config_json(&config_json)
        .map_err(Into::into)
}

#[tauri::command]
pub fn backup_database(
    store: State<'_, WorktraceStore>,
) -> Result<DatabaseTransferResult, CommandError> {
    store.backup_database_to_default().map_err(Into::into)
}

#[tauri::command]
pub fn restore_database(
    store: State<'_, WorktraceStore>,
    path: String,
) -> Result<DatabaseTransferResult, CommandError> {
    store.restore_database_from_path(path).map_err(Into::into)
}
