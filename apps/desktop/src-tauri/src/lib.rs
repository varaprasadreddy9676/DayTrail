pub mod active_window;
pub mod app_icons;
pub mod commands;
pub mod error;
pub mod focus;
pub mod llm;
pub mod matching;
pub mod models;
pub mod native_messaging;
pub mod permissions;
pub mod platform;
pub mod project_detection;
pub mod recovery;
pub mod store;
pub mod store_materialization;
pub mod tray;

use std::time::Duration;

use tauri::{Manager, RunEvent};

use crate::{
    active_window::spawn_active_window_watcher,
    models::Task,
    store::WorktraceStore,
    tray::{setup_tray, show_main_window},
};

pub fn run() {
    install_panic_logger();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Remove the macOS quarantine xattr from the app bundle so users
            // don't have to run `xattr -dr com.apple.quarantine` manually after
            // every install/update. Best-effort — failure is non-fatal.
            #[cfg(target_os = "macos")]
            strip_quarantine();

            let store = WorktraceStore::open_default(app.handle())?;
            if let Err(error) = store.ensure_default_launch_at_login() {
                eprintln!("failed to enable launch at login by default: {error:#}");
            }
            if let Err(error) = store.apply_retention_policy() {
                eprintln!("failed to apply data retention policy: {error:#}");
            }
            ensure_notification_permission(app.handle());
            app.manage(store.clone());
            setup_tray(app, store.clone())?;
            spawn_active_window_watcher(
                store.clone(),
                app.handle().clone(),
                Duration::from_secs(2),
            );
            spawn_task_reminder_scheduler(
                store.clone(),
                app.handle().clone(),
                Duration::from_secs(60),
            );
            // Enforce the data-retention policy on a daily schedule. Without this
            // it would only run at startup — and DayTrail is built to never quit,
            // so on an always-on machine the DB would grow unbounded. This is a
            // no-op while retention is disabled (data_retention_days <= 0).
            spawn_retention_scheduler(store, Duration::from_secs(24 * 60 * 60));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!())
        .expect("failed to build DayTrail desktop")
        .run(|app, event| match event {
            RunEvent::Ready => {
                show_main_window(app);
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => {
                show_main_window(app);
            }
            _ => {}
        });
}

/// Ask for system-notification permission once at startup so the "welcome back"
/// away alert can actually be delivered. Best-effort and non-fatal.
fn ensure_notification_permission(app: &tauri::AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    match app.notification().permission_state() {
        Ok(tauri_plugin_notification::PermissionState::Granted) => {}
        _ => {
            let _ = app.notification().request_permission();
        }
    }
}

fn spawn_task_reminder_scheduler(store: WorktraceStore, app: tauri::AppHandle, interval: Duration) {
    std::thread::spawn(move || loop {
        std::thread::sleep(interval);
        let now = chrono::Utc::now().timestamp_millis();
        let Ok(tasks) = store.list_due_task_reminders(now) else {
            continue;
        };
        for task in tasks {
            post_task_reminder(&app, &task);
            let _ = store.mark_task_reminder_sent(task.id, now);
        }
    });
}

fn post_task_reminder(app: &tauri::AppHandle, task: &Task) {
    use tauri_plugin_notification::NotificationExt;
    let title = if task.title.trim().is_empty() {
        "Reminder due"
    } else {
        task.title.trim()
    };
    let context = [task.client_label.as_deref(), task.project_label.as_deref()]
        .into_iter()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" - ");
    let body = task
        .notes
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::trim)
        .or_else(|| (!context.is_empty()).then_some(context.as_str()))
        .unwrap_or("Due now - open DayTrail to complete or snooze.");
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .sound(crate::platform::notification_sound())
        .show();
}

/// Record every panic to a crash log before the default handler runs, so a
/// crash in any thread (UI, watcher, scheduler, command) leaves a durable trace
/// instead of vanishing in a shipped build. The default hook still runs, so
/// stderr/backtrace behaviour is unchanged for `cargo run`.
fn install_panic_logger() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        write_crash_log(&format!(
            "thread '{thread}' panicked at {location}: {payload}"
        ));
        default_hook(info);
    }));
}

fn write_crash_log(message: &str) {
    let Some(dir) = dirs::data_local_dir() else {
        return;
    };
    let path = dir.join("ai.daytrail.desktop").join("crash.log");
    if let Ok(metadata) = std::fs::metadata(&path) {
        if metadata.len() > 1_000_000 {
            let _ = std::fs::remove_file(&path);
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let _ = writeln!(file, "{timestamp} {message}");
    }
}

/// Periodically apply the data-retention policy so it is enforced on an
/// always-on app, not just at launch. Each sweep is panic-isolated so a single
/// failure can never kill the scheduler for the rest of the process lifetime.
/// A no-op when retention is disabled (the default), so it never deletes data
/// the user has not opted into removing.
fn spawn_retention_scheduler(store: WorktraceStore, interval: Duration) {
    std::thread::spawn(move || loop {
        std::thread::sleep(interval);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            store.apply_retention_policy()
        }));
        match outcome {
            Ok(Ok(summary)) => {
                if summary.deleted_rows > 0 {
                    eprintln!("retention sweep pruned {} rows", summary.deleted_rows);
                }
            }
            Ok(Err(error)) => eprintln!("retention sweep failed: {error:#}"),
            Err(_) => eprintln!("retention sweep panicked — recovered, continuing"),
        }
    });
}

/// Strip the macOS quarantine xattr from the running app bundle. This removes
/// the need for users to run `xattr -dr com.apple.quarantine` manually after
/// installing or updating from a GitHub release DMG.
#[cfg(target_os = "macos")]
fn strip_quarantine() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    // Walk up from .app/Contents/MacOS/binary → .app bundle root.
    let app_path = exe.ancestors().find(|p| {
        p.extension()
            .map(|e| e.eq_ignore_ascii_case("app"))
            .unwrap_or(false)
    });
    let target = match app_path {
        Some(p) => p.to_owned(),
        None => exe, // fallback: strip from the binary itself
    };
    let _ = std::process::Command::new("xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&target)
        .status();
}
