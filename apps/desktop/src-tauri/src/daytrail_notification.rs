use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::{platform, store::WorktraceStore};

const EVENT_NAME: &str = "daytrail-notification";
const DEFAULT_TTL_MS: i64 = 6200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaytrailNotificationKind {
    Focus,
    Recovery,
    Away,
    Task,
    Insight,
    Update,
    Info,
}

impl DaytrailNotificationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::Recovery => "recovery",
            Self::Away => "away",
            Self::Task => "task",
            Self::Insight => "insight",
            Self::Update => "update",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaytrailNotificationPayload {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub sound: String,
    pub created_at_ms: i64,
    pub ttl_ms: i64,
}

pub fn notify(
    app: &tauri::AppHandle,
    store: Option<&WorktraceStore>,
    kind: DaytrailNotificationKind,
    title: impl Into<String>,
    body: impl Into<String>,
) {
    let title = title.into();
    let body = body.into();
    let settings = store.and_then(|store| store.get_settings().ok());
    let premium_enabled = settings
        .as_ref()
        .is_some_and(|settings| settings.premium_notifications_enabled);
    let sound = settings
        .as_ref()
        .map(|settings| settings.notification_sound.as_str())
        .unwrap_or("daytrail");

    if premium_enabled && emit_island(app, kind, &title, &body, sound) {
        return;
    }

    post_native(app, &title, &body, sound);
}

fn emit_island(
    app: &tauri::AppHandle,
    kind: DaytrailNotificationKind,
    title: &str,
    body: &str,
    sound: &str,
) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    if window.is_visible().ok() != Some(true) {
        return false;
    }

    let now = chrono::Utc::now().timestamp_millis();
    let payload = DaytrailNotificationPayload {
        id: format!("{}-{now}", kind.as_str()),
        kind: kind.as_str().to_string(),
        title: title.to_string(),
        body: body.to_string(),
        sound: normalize_sound(sound).to_string(),
        created_at_ms: now,
        ttl_ms: DEFAULT_TTL_MS,
    };
    window.emit(EVENT_NAME, payload).is_ok()
}

fn post_native(app: &tauri::AppHandle, title: &str, body: &str, sound: &str) {
    let mut builder = app.notification().builder().title(title).body(body);
    if let Some(native_sound) = native_sound(sound) {
        builder = builder.sound(native_sound);
    }
    let _ = builder.show();
}

fn normalize_sound(sound: &str) -> &'static str {
    match sound.trim().to_ascii_lowercase().as_str() {
        "glass" => "glass",
        "subtle" => "subtle",
        "none" => "none",
        _ => "daytrail",
    }
}

fn native_sound(sound: &str) -> Option<&'static str> {
    match normalize_sound(sound) {
        "none" => None,
        "glass" => Some(platform::notification_sound_named("glass")),
        "subtle" => Some(platform::notification_sound_named("subtle")),
        _ => Some(platform::notification_sound_named("daytrail")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_unknown_sound_to_daytrail() {
        assert_eq!(normalize_sound(""), "daytrail");
        assert_eq!(normalize_sound("Subtle"), "subtle");
        assert_eq!(normalize_sound("none"), "none");
        assert_eq!(normalize_sound("bad"), "daytrail");
    }

    #[test]
    fn none_sound_skips_native_sound() {
        assert_eq!(native_sound("none"), None);
        assert!(native_sound("daytrail").is_some());
    }
}
