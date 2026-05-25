use tauri::AppHandle;

use crate::{
    error::CommandError,
    models::CapturePermissionSummary,
    permissions::{
        capture_permission_summary, open_permission_settings,
        request_capture_permission as request_capture_permission_setup,
    },
};

#[tauri::command]
pub fn get_capture_permissions() -> Result<CapturePermissionSummary, CommandError> {
    Ok(capture_permission_summary())
}

#[tauri::command]
pub fn open_capture_permission_settings(
    permission_id: String,
) -> Result<CapturePermissionSummary, CommandError> {
    open_permission_settings(&permission_id).map_err(Into::into)
}

#[tauri::command]
pub fn request_capture_permission(
    permission_id: String,
) -> Result<CapturePermissionSummary, CommandError> {
    request_capture_permission_setup(&permission_id).map_err(Into::into)
}

#[tauri::command]
pub fn restart_app(app: AppHandle) -> Result<bool, CommandError> {
    app.request_restart();
    Ok(true)
}
