fn main() {
    if std::env::args().any(|arg| arg == "--native-messaging-host")
        || std::env::var_os("WORKTRACE_NATIVE_MESSAGING").is_some()
    {
        std::process::exit(worktrace_ai_desktop::native_messaging::run());
    }

    worktrace_ai_desktop::run();
}
