use std::time::{Duration, SystemTime};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Wry,
};

use crate::store::WorktraceStore;

/// Without a heartbeat tick for this long, the tray treats capture as stopped.
/// Keep this in sync with the UI-facing capture health threshold.
const TRAY_STALE_AFTER_MS: i64 = 120_000;
/// How often the tray re-checks capture health.
const TRAY_HEALTH_POLL: Duration = Duration::from_secs(10);

pub fn setup_tray(app: &tauri::App, store: WorktraceStore) -> tauri::Result<()> {
    let is_paused = store.pause_state().map(|s| s.paused).unwrap_or(false);
    // Clones for the background health updater (the originals are moved into the
    // menu-event closure below).
    let updater_app = app.handle().clone();
    let updater_store = store.clone();

    let status = MenuItem::with_id(
        app,
        "status",
        "DayTrail: Tracking Active",
        false,
        None::<&str>,
    )?;
    let show = MenuItem::with_id(app, "show", "Show Command Center", true, None::<&str>)?;
    let quick_note =
        MenuItem::with_id(app, "quick_note", "Quick-Capture Note", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause tracking", !is_paused, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume", "Resume tracking", is_paused, None::<&str>)?;
    let eod = MenuItem::with_id(app, "eod", "Run End-of-Day Ritual", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit DayTrail", true, None::<&str>)?;
    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let separator_c = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &separator_a,
            &show,
            &quick_note,
            &separator_b,
            &pause,
            &resume,
            &eod,
            &separator_c,
            &settings,
            &quit,
        ],
    )?;

    // Clone menu items so the event handler can update their enabled state.
    let pause_item = pause.clone();
    let resume_item = resume.clone();
    let status_item = status.clone();
    // A separate clone for the background health updater.
    let updater_status_item = status.clone();

    let mut tray = TrayIconBuilder::with_id("daytrail-tray")
        .tooltip("DayTrail")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" | "settings" => {
                show_main_window(app);
            }
            "quick_note" => {
                show_main_window(app);
                let _ = app.emit("tray-navigate", "quick_note");
            }
            "eod" => {
                show_main_window(app);
                let _ = app.emit("tray-navigate", "eod");
            }
            "pause" => {
                if let Ok(state) = store.pause("tray") {
                    let _ = pause_item.set_enabled(false);
                    let _ = resume_item.set_enabled(true);
                    let label = if state.paused {
                        "DayTrail: Paused"
                    } else {
                        "DayTrail: Tracking Active"
                    };
                    let _ = status_item.set_text(label);
                }
            }
            "resume" => {
                if let Ok(state) = store.resume() {
                    let _ = pause_item.set_enabled(true);
                    let _ = resume_item.set_enabled(false);
                    let label = if state.paused {
                        "DayTrail: Paused"
                    } else {
                        "DayTrail: Tracking Active"
                    };
                    let _ = status_item.set_text(label);
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                let app = tray.app_handle();
                show_main_window(app);
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    let tray_icon = tray.build(app)?;

    spawn_tray_health_updater(updater_app, updater_store, updater_status_item, tray_icon);

    Ok(())
}

/// Periodically reflect real capture health in the tray menu and tooltip.
/// Keep the macOS menu bar itself to a single DayTrail icon: setting a tray
/// title renders as separate text beside the icon, which looks like a duplicate
/// status item and confused users.
fn spawn_tray_health_updater(
    app: AppHandle,
    store: WorktraceStore,
    status_item: MenuItem<Wry>,
    tray: TrayIcon<Wry>,
) {
    use crate::active_window::{assess_capture_liveness, watcher_heartbeat, CaptureLiveness};

    std::thread::spawn(move || loop {
        std::thread::sleep(TRAY_HEALTH_POLL);

        let paused = store
            .pause_state()
            .map(|state| state.paused)
            .unwrap_or(false);
        let heartbeat = watcher_heartbeat();
        let liveness =
            assess_capture_liveness(heartbeat.as_ref(), tray_now_ms(), TRAY_STALE_AFTER_MS);

        let label: &str = if paused {
            "DayTrail: Paused"
        } else {
            match liveness {
                CaptureLiveness::Stalled => "DayTrail: Capture stopped",
                CaptureLiveness::PermissionLost => "DayTrail: Accessibility needed",
                _ => "DayTrail: Tracking Active",
            }
        };

        let status_item = status_item.clone();
        let tray = tray.clone();
        // Menu/tray mutations must run on the main thread.
        let _ = app.run_on_main_thread(move || {
            let _ = status_item.set_text(label);
            let _ = tray.set_tooltip(Some(label));
            let _ = tray.set_title(None::<&str>);
        });
    });
}

fn tray_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.center();
        let _ = window.set_focus();
        return;
    }

    if let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
    {
        match WebviewWindowBuilder::from_config(app, config).and_then(|builder| builder.build()) {
            Ok(window) => {
                let _ = window.show();
                let _ = window.set_focus();
                return;
            }
            Err(error) => {
                eprintln!("failed to create main window from config: {error}");
            }
        }
    }

    match WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("DayTrail")
        .inner_size(1200.0, 820.0)
        .min_inner_size(900.0, 640.0)
        .center()
        .build()
    {
        Ok(window) => {
            let _ = window.show();
            let _ = window.set_focus();
        }
        Err(error) => {
            eprintln!("failed to create fallback main window: {error}");
        }
    }
}
