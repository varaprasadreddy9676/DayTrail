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
            app.manage(store.clone());
            setup_tray(app, store.clone())?;
            spawn_active_window_watcher(store, Duration::from_secs(2));
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
