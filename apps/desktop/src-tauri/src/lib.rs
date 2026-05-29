pub mod active_window;
pub mod app_icons;
pub mod commands;
pub mod error;
pub mod llm;
pub mod models;
pub mod native_messaging;
pub mod permissions;
pub mod platform;
pub mod project_detection;
pub mod store;
pub mod store_materialization;
pub mod tray;

use std::time::Duration;

use tauri::{Manager, RunEvent};

use crate::{
    active_window::spawn_active_window_watcher,
    store::WorktraceStore,
    tray::{setup_tray, show_main_window},
};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let store = WorktraceStore::open_default(app.handle())?;
            if let Err(error) = store.ensure_default_launch_at_login() {
                eprintln!("failed to enable launch at login by default: {error:#}");
            }
            if let Err(error) = store.apply_retention_policy() {
                eprintln!("failed to apply data retention policy: {error:#}");
            }
            app.manage(store.clone());
            setup_tray(app, store.clone())?;
            spawn_active_window_watcher(store.clone(), Duration::from_secs(2));
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

/// Periodically apply the data-retention policy so it is enforced on an
/// always-on app, not just at launch. Each sweep is panic-isolated so a single
/// failure can never kill the scheduler for the rest of the process lifetime.
/// A no-op when retention is disabled (the default), so it never deletes data
/// the user has not opted into removing.
fn spawn_retention_scheduler(store: WorktraceStore, interval: Duration) {
    std::thread::spawn(move || loop {
        std::thread::sleep(interval);
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.apply_retention_policy()));
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
