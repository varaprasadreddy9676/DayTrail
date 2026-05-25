use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};

use crate::store::WorktraceStore;

pub fn setup_tray(app: &tauri::App, store: WorktraceStore) -> tauri::Result<()> {
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
    let pause = MenuItem::with_id(app, "pause", "Pause tracking", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume", "Resume tracking", true, None::<&str>)?;
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

    let mut tray = TrayIconBuilder::with_id("daytrail-tray")
        .tooltip("DayTrail")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .icon_as_template(true)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" | "quick_note" | "eod" | "settings" => {
                show_main_window(app);
            }
            "pause" => {
                let _ = store.pause("tray");
            }
            "resume" => {
                let _ = store.resume();
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

    tray.build(app)?;

    Ok(())
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
