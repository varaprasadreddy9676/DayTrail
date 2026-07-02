use tauri_plugin_notification::NotificationExt;

use crate::{platform, store::WorktraceStore};

/// Height in points of the main display's physical notch (MacBook Pro
/// 14"/16", 2021+), resolved once at startup — see
/// `platform::main_screen_notch_height`. 0.0 means no notch. Not currently
/// used to change notification behavior (see `notify`), but kept as
/// groundwork for a future notch-anchored notification UI.
pub struct NotchState(pub f64);

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

pub fn notify(
    app: &tauri::AppHandle,
    store: Option<&WorktraceStore>,
    _kind: DaytrailNotificationKind,
    title: impl Into<String>,
    body: impl Into<String>,
) -> bool {
    let title = title.into();
    let body = body.into();
    let sound = store
        .and_then(|store| store.get_settings().ok())
        .map(|settings| settings.notification_sound)
        .unwrap_or_else(|| "daytrail".to_string());

    post_native(app, &title, &body, &sound)
}

fn post_native(app: &tauri::AppHandle, title: &str, body: &str, sound: &str) -> bool {
    let mut builder = app.notification().builder().title(title).body(body);
    if let Some(native_sound) = native_sound(sound) {
        builder = builder.sound(native_sound);
    }
    builder.show().is_ok()
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
