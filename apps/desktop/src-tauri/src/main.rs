// Hide the Windows console window in release builds — DayTrail is a GUI/tray app.
// Without this, Windows opens a cmd window beside it and closing that window kills
// the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--native-messaging-host")
        || std::env::var_os("WORKTRACE_NATIVE_MESSAGING").is_some()
    {
        std::process::exit(worktrace_ai_desktop::native_messaging::run());
    }

    worktrace_ai_desktop::run();
}
