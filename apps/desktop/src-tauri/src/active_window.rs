use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime},
};

#[cfg(target_os = "macos")]
use std::ffi::CStr;
#[cfg(target_os = "windows")]
use std::{ffi::OsString, os::windows::ffi::OsStringExt};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    models::TerminalBridgeMetadata,
    project_detection::{default_project_sources, detect_project_candidates_from_sources},
    store::WorktraceStore,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWindowInfo {
    pub app_name: String,
    pub window_title: Option<String>,
    pub process_id: Option<u32>,
    pub url: Option<String>,
    pub workspace_key: Option<String>,
    pub workspace_candidates: Vec<String>,
    pub ai_tools: Vec<String>,
    pub captured_at: String,
}

pub fn spawn_active_window_watcher(store: WorktraceStore, interval: Duration) {
    thread::spawn(move || loop {
        let _ = store.ingest_local_bridge_files();
        if let Some(info) = active_window_fallback() {
            if !is_self_app(&info.app_name) {
                let metadata = serde_json::to_string(&info).ok();
                let _ = store.record_active_window_context(
                    &info.app_name,
                    info.window_title.as_deref(),
                    info.url.as_deref(),
                    info.workspace_key.as_deref(),
                    metadata.as_deref(),
                    Some(interval),
                );
                let _ = store.materialize_work_memory();
            }
        }
        thread::sleep(interval);
    });
}

pub fn active_window_fallback() -> Option<ActiveWindowInfo> {
    platform_active_window()
}

#[cfg(target_os = "macos")]
fn platform_active_window() -> Option<ActiveWindowInfo> {
    native_frontmost_application().or_else(applescript_frontmost_application)
}

#[cfg(target_os = "macos")]
fn native_frontmost_application() -> Option<ActiveWindowInfo> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    fn ns_string_to_string(value: &NSString) -> Option<String> {
        let ptr = value.UTF8String();
        if ptr.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .ok()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let app_name = app
        .localizedName()
        .as_deref()
        .and_then(ns_string_to_string)
        .or_else(|| {
            app.bundleIdentifier()
                .as_deref()
                .and_then(ns_string_to_string)
        })?;
    let process_id = u32::try_from(app.processIdentifier()).ok();
    let title = process_id
        .and_then(accessibility_focused_window_title)
        .or_else(|| window_title_for_app(&app_name));
    let url = browser_url_for_app(&app_name);
    let mut workspace_candidates = process_id
        .map(workspace_candidates_from_process)
        .unwrap_or_default();
    if workspace_candidates.is_empty() {
        workspace_candidates = editor_workspace_candidates_from_storage(&app_name);
    }
    let bridge_hint = terminal_bridge_hint_for_app(&app_name);
    let workspace_key = workspace_from_title(title.as_deref())
        .or_else(|| workspace_from_candidates(title.as_deref(), &workspace_candidates))
        .or_else(|| single_workspace_candidate(&workspace_candidates))
        .or_else(|| workspace_from_editor_storage(&app_name, title.as_deref()))
        .or_else(|| bridge_hint.as_ref().map(|hint| hint.cwd.clone()));
    let mut ai_tools = ai_tools_from_processes(&app_name, process_id);
    if let Some(hint) = bridge_hint {
        for tool in hint.ai_tools {
            push_tool(&mut ai_tools, &tool);
        }
    }

    Some(ActiveWindowInfo {
        app_name,
        window_title: title,
        process_id,
        url,
        workspace_key,
        workspace_candidates,
        ai_tools,
        captured_at: now_utc(),
    })
}

#[cfg(target_os = "macos")]
fn applescript_frontmost_application() -> Option<ActiveWindowInfo> {
    let output = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to set frontApp to name of first application process whose frontmost is true"#,
            "-e",
            r#"tell application "System Events" to set frontTitle to "" "#,
            "-e",
            r#"tell application "System Events" to set frontPid to unix id of first application process whose frontmost is true"#,
            "-e",
            r#"tell application "System Events" to tell process frontApp to if exists window 1 then set frontTitle to name of window 1"#,
            "-e",
            r#"return frontApp & linefeed & frontTitle & linefeed & frontPid"#,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let app_name = lines.next()?.trim().to_string();
    if app_name.is_empty() {
        return None;
    }
    let title = lines
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let process_id = lines
        .next()
        .map(str::trim)
        .and_then(|value| value.parse::<u32>().ok());
    let url = browser_url_for_app(&app_name);
    let title = process_id
        .and_then(accessibility_focused_window_title)
        .or(title.map(ToOwned::to_owned));
    let mut workspace_candidates = process_id
        .map(workspace_candidates_from_process)
        .unwrap_or_default();
    if workspace_candidates.is_empty() {
        workspace_candidates = editor_workspace_candidates_from_storage(&app_name);
    }
    let bridge_hint = terminal_bridge_hint_for_app(&app_name);
    let workspace_key = workspace_from_title(title.as_deref())
        .or_else(|| workspace_from_candidates(title.as_deref(), &workspace_candidates))
        .or_else(|| single_workspace_candidate(&workspace_candidates))
        .or_else(|| workspace_from_editor_storage(&app_name, title.as_deref()))
        .or_else(|| bridge_hint.as_ref().map(|hint| hint.cwd.clone()));
    let mut ai_tools = ai_tools_from_processes(&app_name, process_id);
    if let Some(hint) = bridge_hint {
        for tool in hint.ai_tools {
            push_tool(&mut ai_tools, &tool);
        }
    }

    Some(ActiveWindowInfo {
        app_name,
        window_title: title,
        process_id,
        url,
        workspace_key,
        workspace_candidates,
        ai_tools,
        captured_at: now_utc(),
    })
}

#[cfg(target_os = "macos")]
fn accessibility_focused_window_title(pid: u32) -> Option<String> {
    use core_foundation::{
        base::{CFRelease, CFTypeRef, TCFType},
        string::{CFString, CFStringRef},
    };
    use std::{ffi::c_void, ptr};

    type AXError = i32;
    type AXUIElementRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
    }

    unsafe fn copy_attribute(element: AXUIElementRef, attribute: &str) -> Option<CFTypeRef> {
        let attribute = CFString::new(attribute);
        let mut value: CFTypeRef = ptr::null();
        let error =
            AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value);
        (error == 0 && !value.is_null()).then_some(value)
    }

    unsafe {
        let app = AXUIElementCreateApplication(pid as i32);
        if app.is_null() {
            return None;
        }

        let window =
            copy_attribute(app, "AXFocusedWindow").or_else(|| copy_attribute(app, "AXMainWindow"));
        CFRelease(app as CFTypeRef);

        let window = window?;
        let title = copy_attribute(window as AXUIElementRef, "AXTitle");
        CFRelease(window);

        let title = title?;
        let title = CFString::wrap_under_create_rule(title as CFStringRef)
            .to_string()
            .trim()
            .to_string();
        (!title.is_empty()).then_some(title)
    }
}

#[cfg(target_os = "macos")]
fn window_title_for_app(app_name: &str) -> Option<String> {
    let script = format!(
        r#"tell application "System Events" to tell process "{}" to if exists window 1 then return name of window 1"#,
        app_name.replace('"', "")
    );
    run_osascript(&[&script])
}

#[cfg(target_os = "macos")]
fn workspace_candidates_from_process(pid: u32) -> Vec<String> {
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string(), "-Fn"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let home = dirs::home_dir();
    let mut seen = HashSet::new();
    let mut candidates = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .filter_map(|value| workspace_candidate_from_path(Path::new(value), home.as_deref()))
        .filter(|value| seen.insert(value.clone()))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        project_name(right)
            .len()
            .cmp(&project_name(left).len())
            .then_with(|| left.cmp(right))
    });
    candidates
}

fn workspace_from_title(title: Option<&str>) -> Option<String> {
    let title = title?;
    title
        .split_whitespace()
        .find(|part| part.starts_with('/') || part.starts_with("~/"))
        .and_then(|part| {
            let clean = part.trim_matches(|ch: char| {
                matches!(ch, ',' | ';' | ':' | '"' | '\'' | ')' | '(' | '[' | ']')
            });
            if let Some(rest) = clean.strip_prefix("~/") {
                dirs::home_dir().map(|home| home.join(rest).display().to_string())
            } else {
                Some(clean.to_string())
            }
        })
}

fn workspace_from_candidates(title: Option<&str>, candidates: &[String]) -> Option<String> {
    let title = title?.to_ascii_lowercase();
    candidates
        .iter()
        .filter(|candidate| {
            let name = project_name(candidate).to_ascii_lowercase();
            !name.is_empty() && title.contains(&name)
        })
        .max_by_key(|candidate| project_name(candidate).len())
        .cloned()
}

fn single_workspace_candidate(candidates: &[String]) -> Option<String> {
    (candidates.len() == 1).then(|| candidates[0].clone())
}

fn project_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_string()
}

fn workspace_from_editor_storage(app_name: &str, title: Option<&str>) -> Option<String> {
    let candidates = editor_workspace_candidates_from_storage(app_name);
    workspace_from_candidates(title, &candidates)
        .or_else(|| single_workspace_candidate(&candidates))
        .or_else(|| candidates.into_iter().next())
}

fn editor_workspace_candidates_from_storage(app_name: &str) -> Vec<String> {
    if !is_editor_app(app_name) {
        return Vec::new();
    }

    let mut sources = default_project_sources();
    sources.terminal_bridge_metadata_paths.clear();
    let app_lower = app_name.to_ascii_lowercase();
    sources.workspace_storage_roots.retain(|root| {
        let rendered = root.display().to_string().to_ascii_lowercase();
        if app_lower.contains("cursor") {
            rendered.contains("cursor")
        } else if app_lower.contains("code") || app_lower.contains("vscodium") {
            rendered.contains("code") && !rendered.contains("cursor")
        } else {
            true
        }
    });
    detect_project_candidates_from_sources(sources)
        .unwrap_or_default()
        .into_iter()
        .map(|context| context.path)
        .collect()
}

#[cfg(target_os = "macos")]
fn ai_tools_from_processes(app_name: &str, process_id: Option<u32>) -> Vec<String> {
    let mut tools = Vec::new();
    for tool in terminal_ai_tools_from_processes(app_name, process_id) {
        push_tool(&mut tools, &tool);
    }
    tools
}

#[cfg(target_os = "macos")]
fn editor_ai_tools_from_processes(app_name: &str) -> Vec<String> {
    // Do not count loaded editor extension/helper processes as usage. VS Code, Cursor,
    // and JetBrains keep AI extensions resident even when the user did not invoke them.
    // Usage must come from an editor bridge event, browser URL, AI app, or terminal command.
    let _ = app_name;
    Vec::new()
}

#[cfg(target_os = "macos")]
fn editor_ai_tools_from_processes_legacy_scan(app_name: &str) -> Vec<String> {
    if !is_editor_app(app_name) {
        return Vec::new();
    }

    let output = Command::new("ps").args(["-axo", "command"]).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let mut tools = Vec::new();
    if app_name == "Cursor" {
        push_tool(&mut tools, "Cursor");
    }

    for command in String::from_utf8_lossy(&output.stdout).lines() {
        let lower = command.to_ascii_lowercase();
        let relevant = lower.contains("/.vscode/extensions/")
            || lower.contains("/.cursor/extensions/")
            || lower.contains("/visual studio code.app/")
            || lower.contains("code helper")
            || lower.contains("codex app-server")
            || lower.contains("claude")
            || lower.contains("gemini");
        if !relevant {
            continue;
        }

        if lower.contains("codex app-server")
            || lower.contains("extensionid=openai.chatgpt")
            || lower.contains("/openai.chatgpt-")
        {
            push_tool(&mut tools, "Codex");
        }
        if lower.contains("github.copilot")
            || lower.contains("@vscode/copilot")
            || lower.contains("/extensions/copilot")
            || lower.contains("copilot")
        {
            push_tool(&mut tools, "Copilot");
        }
        if lower.contains("claude-code")
            || lower.contains("claude code")
            || lower.contains("anthropic.claude")
        {
            push_tool(&mut tools, "Claude Code");
        }
        if lower.contains("gemini") || lower.contains("@google/gemini-cli") {
            push_tool(&mut tools, "Gemini");
        }
        if lower.contains("saoudrizwan.claude-dev") || lower.contains("cline") {
            push_tool(&mut tools, "Cline");
        }
        if lower.contains("continue.continue") || lower.contains("/continue") {
            push_tool(&mut tools, "Continue");
        }
        if lower.contains("aider") {
            push_tool(&mut tools, "Aider");
        }
        if lower.contains("windsurf") {
            push_tool(&mut tools, "Windsurf");
        }
    }

    push_recent_editor_log_tools(&mut tools, app_name);
    tools
}

#[cfg(target_os = "macos")]
fn terminal_ai_tools_from_processes(app_name: &str, process_id: Option<u32>) -> Vec<String> {
    if !is_terminal_app(app_name) && !is_editor_app(app_name) {
        return Vec::new();
    }
    let Some(process_id) = process_id else {
        return Vec::new();
    };

    let output = Command::new("ps")
        .args(["-axo", "pid,ppid,command"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    terminal_ai_tools_from_ps_output(&String::from_utf8_lossy(&output.stdout), process_id)
}

fn terminal_ai_tools_from_ps_output(output: &str, root_pid: u32) -> Vec<String> {
    let rows = parse_process_rows(output);
    let mut children_by_parent: HashMap<u32, Vec<&ProcessRow>> = HashMap::new();
    for row in &rows {
        children_by_parent.entry(row.ppid).or_default().push(row);
    }

    let mut descendants = HashSet::from([root_pid]);
    let mut stack = vec![root_pid];
    while let Some(parent) = stack.pop() {
        if let Some(children) = children_by_parent.get(&parent) {
            for child in children {
                if descendants.insert(child.pid) {
                    stack.push(child.pid);
                }
            }
        }
    }

    let mut tools = Vec::new();
    for row in rows
        .iter()
        .filter(|row| row.pid != root_pid && descendants.contains(&row.pid))
    {
        push_cli_ai_tools_from_command(&mut tools, &row.command);
    }
    tools
}

#[derive(Debug)]
struct ProcessRow {
    pid: u32,
    ppid: u32,
    command: String,
}

fn parse_process_rows(output: &str) -> Vec<ProcessRow> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let command = parts.collect::<Vec<_>>().join(" ");
            (!command.is_empty()).then_some(ProcessRow { pid, ppid, command })
        })
        .collect()
}

fn push_cli_ai_tools_from_command(tools: &mut Vec<String>, command: &str) {
    let lower = command.to_ascii_lowercase();
    if lower.contains("codex app-server") || lower.contains("/.vscode/extensions/") {
        return;
    }

    if command_invokes_tool(command, &["gemini"]) || lower.contains("@google/gemini-cli") {
        push_tool(tools, "Gemini");
    }
    if command_invokes_tool(command, &["codex"]) || lower.contains("@openai/codex") {
        push_tool(tools, "Codex");
    }
    if command_invokes_tool(command, &["claude", "claude-code"]) || lower.contains("claude-code") {
        push_tool(tools, "Claude Code");
    }
    if command_invokes_tool(command, &["aider"]) {
        push_tool(tools, "Aider");
    }
    if command_invokes_tool(command, &["cline"]) {
        push_tool(tools, "Cline");
    }
    if command_invokes_tool(command, &["continue"]) || lower.contains("continue.continue") {
        push_tool(tools, "Continue");
    }
    if command_invokes_tool(command, &["opencode"]) {
        push_tool(tools, "OpenCode");
    }
}

fn command_invokes_tool(command: &str, names: &[&str]) -> bool {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    let Some(first) = tokens.first() else {
        return false;
    };
    let first_name = executable_name(first);
    if names.iter().any(|name| first_name == *name) {
        return true;
    }

    let first_is_launcher = matches!(
        first_name.as_str(),
        "node" | "npm" | "npx" | "pnpm" | "bun" | "uvx"
    );
    if !first_is_launcher {
        return false;
    }

    tokens.iter().skip(1).any(|token| {
        let lower = token.to_ascii_lowercase();
        (lower.contains('/') || lower.contains('\\') || lower.starts_with('@'))
            && names.iter().any(|name| executable_name(token) == *name)
    })
}

fn executable_name(token: &str) -> String {
    let clean = token.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | '[' | ']'));
    Path::new(clean)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(clean)
        .trim_end_matches(".cmd")
        .trim_end_matches(".exe")
        .to_ascii_lowercase()
}

fn push_recent_editor_log_tools(tools: &mut Vec<String>, app_name: &str) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let roots = editor_log_roots(app_name, &home);
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(36 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    for root in roots {
        scan_recent_editor_log_tools(tools, &root, cutoff, 0);
    }
}

fn editor_log_roots(app_name: &str, home: &Path) -> Vec<PathBuf> {
    let is_cursor = app_name.eq_ignore_ascii_case("cursor");
    #[cfg(target_os = "macos")]
    {
        if is_cursor {
            vec![home.join("Library/Application Support/Cursor/logs")]
        } else {
            vec![home.join("Library/Application Support/Code/logs")]
        }
    }
    #[cfg(target_os = "windows")]
    {
        if is_cursor {
            vec![home.join("AppData/Roaming/Cursor/logs")]
        } else {
            vec![home.join("AppData/Roaming/Code/logs")]
        }
    }
    #[cfg(target_os = "linux")]
    {
        if is_cursor {
            vec![home.join(".config/Cursor/logs")]
        } else {
            vec![home.join(".config/Code/logs")]
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Vec::new()
    }
}

fn scan_recent_editor_log_tools(
    tools: &mut Vec<String>,
    path: &Path,
    cutoff: SystemTime,
    depth: u8,
) {
    if depth > 8 || !path.exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let modified = path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if modified >= cutoff {
            push_ai_tool_from_path(tools, &path);
        }
        if path.is_dir() {
            scan_recent_editor_log_tools(tools, &path, cutoff, depth + 1);
        }
    }
}

fn push_ai_tool_from_path(tools: &mut Vec<String>, path: &Path) {
    let lower = path.display().to_string().to_ascii_lowercase();
    if lower.contains("anthropic.claude-code") || lower.contains("claude-code") {
        push_tool(tools, "Claude Code");
    }
    if lower.contains("github.copilot") || lower.contains("copilot-chat") {
        push_tool(tools, "Copilot");
    }
    if lower.contains("openai.chatgpt") {
        push_tool(tools, "Codex");
    }
    if lower.contains("gemini") || lower.contains("google.gemini") {
        push_tool(tools, "Gemini");
    }
    if lower.contains("continue.continue") {
        push_tool(tools, "Continue");
    }
    if lower.contains("saoudrizwan.claude-dev") || lower.contains("cline") {
        push_tool(tools, "Cline");
    }
}

fn push_tool(tools: &mut Vec<String>, label: &str) {
    if !tools.iter().any(|tool| tool == label) {
        tools.push(label.to_string());
    }
}

#[cfg(target_os = "macos")]
fn workspace_candidate_from_path(path: &Path, home: Option<&Path>) -> Option<String> {
    if !path.is_absolute() || !path.exists() || path.is_dir() && is_ignored_path(path) {
        return None;
    }
    if let Some(home) = home {
        if !path.starts_with(home) {
            return None;
        }
    }

    let mut cursor = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    loop {
        if is_project_root(&cursor) {
            return Some(cursor.display().to_string());
        }
        if !cursor.pop() {
            break;
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn is_project_root(path: &Path) -> bool {
    [
        ".git",
        "package.json",
        "pnpm-workspace.yaml",
        "tsconfig.json",
        "Cargo.toml",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "gradlew",
        "go.mod",
        "pyproject.toml",
        "requirements.txt",
        "setup.py",
        "composer.json",
        ".idea",
        "nbproject",
    ]
    .iter()
    .any(|marker| path.join(marker).exists())
}

#[cfg(target_os = "macos")]
fn is_ignored_path(path: &Path) -> bool {
    let rendered = path.display().to_string();
    [
        "/Library/",
        "/Library/Caches/",
        "/Library/Application Support/",
        "/.cache/",
        "/node_modules/",
        "/target/",
        "/.git/",
    ]
    .iter()
    .any(|needle| rendered.contains(needle))
}

#[cfg(target_os = "macos")]
fn browser_url_for_app(app_name: &str) -> Option<String> {
    if app_name == "Safari" {
        return run_osascript(&[
            r#"tell application "Safari" to if (count of windows) > 0 then return URL of front document"#,
        ]);
    }

    if is_chromium_browser(app_name) {
        let script = format!(
            r#"tell application "{}" to if (count of windows) > 0 then return URL of active tab of front window"#,
            app_name.replace('"', "")
        );
        return run_osascript(&[&script]);
    }

    None
}

#[cfg(target_os = "macos")]
fn run_osascript(lines: &[&str]) -> Option<String> {
    let mut command = Command::new("osascript");
    for line in lines {
        command.arg("-e").arg(line);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(target_os = "macos")]
fn is_chromium_browser(app_name: &str) -> bool {
    matches!(
        app_name,
        "Google Chrome"
            | "Google Chrome Canary"
            | "Microsoft Edge"
            | "Brave Browser"
            | "ChatGPT Atlas"
            | "Arc"
            | "Chromium"
            | "Opera"
            | "Vivaldi"
    )
}

fn is_editor_app(app_name: &str) -> bool {
    matches!(
        app_name.to_ascii_lowercase().as_str(),
        "code"
            | "vs code"
            | "visual studio code"
            | "cursor"
            | "vscodium"
            | "webstorm"
            | "intellij idea"
            | "pycharm"
            | "phpstorm"
            | "goland"
            | "rubymine"
            | "netbeans"
            | "netbeans ide"
    )
}

fn is_terminal_app(app_name: &str) -> bool {
    matches!(
        app_name.to_ascii_lowercase().as_str(),
        "terminal"
            | "iterm"
            | "iterm2"
            | "warp"
            | "warp terminal"
            | "kitty"
            | "alacritty"
            | "wezterm"
            | "ghostty"
            | "windows terminal"
            | "powershell"
            | "command prompt"
    )
}

#[derive(Debug)]
struct TerminalBridgeHint {
    cwd: String,
    ai_tools: Vec<String>,
}

fn terminal_bridge_hint_for_app(app_name: &str) -> Option<TerminalBridgeHint> {
    if !is_terminal_app(app_name) && !is_editor_app(app_name) {
        return None;
    }

    let now = SystemTime::now();
    for path in default_project_sources().terminal_bridge_metadata_paths {
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if now.duration_since(modified).unwrap_or_default() > Duration::from_secs(5 * 60) {
            continue;
        }

        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(bridge) = serde_json::from_str::<TerminalBridgeMetadata>(&contents) else {
            continue;
        };
        let cwd = bridge.cwd.trim();
        if cwd.is_empty() {
            continue;
        }
        if !terminal_bridge_matches_app(app_name, &bridge) {
            continue;
        }

        let mut ai_tools = Vec::new();
        if let Some(command) = bridge.last_command.as_deref() {
            push_cli_ai_tools_from_command(&mut ai_tools, command);
        }
        return Some(TerminalBridgeHint {
            cwd: cwd.to_string(),
            ai_tools,
        });
    }

    None
}

fn terminal_bridge_matches_app(app_name: &str, bridge: &TerminalBridgeMetadata) -> bool {
    let app = app_name.to_ascii_lowercase();
    let terminal = bridge
        .terminal
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if !is_terminal_app(app_name) {
        return !terminal.is_empty()
            && (terminal == app
                || app.contains(&terminal)
                || terminal.contains(&app)
                || (app == "vs code" && terminal == "code")
                || (app == "visual studio code" && terminal == "code"));
    }

    terminal.is_empty()
        || terminal == app
        || app.contains(&terminal)
        || terminal.contains(&app)
        || (app == "vs code" && terminal == "code")
        || (app == "visual studio code" && terminal == "code")
}

#[cfg(target_os = "windows")]
fn platform_active_window() -> Option<ActiveWindowInfo> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 {
            return None;
        }

        let mut pid = 0_u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }

        let title = windows_window_title(hwnd);
        let executable = windows_process_image_path(pid);
        let app_name = executable
            .as_deref()
            .and_then(display_app_name_from_executable)
            .unwrap_or_else(|| format!("Process {pid}"));
        let workspace_candidates = editor_workspace_candidates_from_storage(&app_name);
        let bridge_hint = terminal_bridge_hint_for_app(&app_name);
        let workspace_key = workspace_from_title(title.as_deref())
            .or_else(|| workspace_from_candidates(title.as_deref(), &workspace_candidates))
            .or_else(|| single_workspace_candidate(&workspace_candidates))
            .or_else(|| workspace_from_editor_storage(&app_name, title.as_deref()))
            .or_else(|| bridge_hint.as_ref().map(|hint| hint.cwd.clone()));
        let mut ai_tools = ai_tools_from_processes(&app_name, Some(pid));
        if let Some(hint) = bridge_hint {
            for tool in hint.ai_tools {
                push_tool(&mut ai_tools, &tool);
            }
        }

        Some(ActiveWindowInfo {
            app_name,
            window_title: title,
            process_id: Some(pid),
            url: None,
            workspace_key,
            workspace_candidates,
            ai_tools,
            captured_at: now_utc(),
        })
    }
}

#[cfg(target_os = "windows")]
unsafe fn windows_window_title(hwnd: isize) -> Option<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return None;
    }
    let mut buffer = vec![0_u16; (len as usize) + 1];
    let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    if copied <= 0 {
        return None;
    }
    utf16_to_string(&buffer[..copied as usize])
}

#[cfg(target_os = "windows")]
unsafe fn windows_process_image_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle == 0 {
        return None;
    }

    let mut size = 32_768_u32;
    let mut buffer = vec![0_u16; size as usize];
    let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
    let _ = CloseHandle(handle);
    if ok == 0 || size == 0 {
        return None;
    }
    utf16_to_string(&buffer[..size as usize])
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn display_app_name_from_executable(path: &str) -> Option<String> {
    let stem = executable_stem_from_path(path)?;
    let lower = stem.to_ascii_lowercase();
    let label = match lower.as_str() {
        "code" => "VS Code",
        "cursor" => "Cursor",
        "vscodium" => "VSCodium",
        "chrome" => "Google Chrome",
        "msedge" => "Microsoft Edge",
        "brave" => "Brave Browser",
        "firefox" => "Firefox",
        "windowsterminal" | "wt" => "Windows Terminal",
        "powershell" | "pwsh" => "PowerShell",
        "cmd" => "Command Prompt",
        "idea64" | "idea" => "IntelliJ IDEA",
        "pycharm64" | "pycharm" => "PyCharm",
        "webstorm64" | "webstorm" => "WebStorm",
        "netbeans64" | "netbeans" => "NetBeans",
        _ => stem,
    };
    Some(label.to_string())
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn executable_stem_from_path(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\'])
        .next()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .strip_suffix(".exe")
                .or_else(|| value.strip_suffix(".EXE"))
                .unwrap_or(value)
        })
}

#[cfg(target_os = "windows")]
fn editor_ai_tools_from_processes(app_name: &str) -> Vec<String> {
    if !is_editor_app(app_name) {
        return Vec::new();
    }

    let mut tools = Vec::new();
    if app_name == "Cursor" {
        push_tool(&mut tools, "Cursor");
    }

    for executable in windows_process_executable_names() {
        let lower = executable.to_ascii_lowercase();
        if lower.contains("codex") || lower.contains("openai") {
            push_tool(&mut tools, "Codex");
        }
        if lower.contains("claude") {
            push_tool(&mut tools, "Claude Code");
        }
        if lower.contains("gemini") {
            push_tool(&mut tools, "Gemini");
        }
        if lower.contains("copilot") {
            push_tool(&mut tools, "Copilot");
        }
        if lower.contains("aider") {
            push_tool(&mut tools, "Aider");
        }
        if lower.contains("cline") {
            push_tool(&mut tools, "Cline");
        }
        if lower.contains("windsurf") {
            push_tool(&mut tools, "Windsurf");
        }
    }

    push_recent_editor_log_tools(&mut tools, app_name);
    tools
}

#[cfg(target_os = "windows")]
fn terminal_ai_tools_from_processes(app_name: &str, process_id: Option<u32>) -> Vec<String> {
    if !is_terminal_app(app_name) && !is_editor_app(app_name) {
        return Vec::new();
    }

    if let (Some(process_id), Some(output)) = (process_id, windows_process_command_output()) {
        let scoped_tools = terminal_ai_tools_from_ps_output(&output, process_id);
        if !scoped_tools.is_empty() {
            return scoped_tools;
        }
    }

    let mut tools = Vec::new();
    for executable in windows_process_executable_names() {
        push_cli_ai_tools_from_command(&mut tools, &executable);
    }
    tools
}

#[cfg(target_os = "windows")]
fn windows_process_command_output() -> Option<String> {
    let script = "Get-CimInstance Win32_Process | ForEach-Object { '{0} {1} {2}' -f $_.ProcessId,$_.ParentProcessId,(($_.CommandLine -as [string]) -replace '[\\r\\n]+',' ') }";
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(target_os = "windows")]
fn windows_process_executable_names() -> Vec<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }

        let mut entry = std::mem::zeroed::<PROCESSENTRY32W>();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut names = Vec::new();
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if let Some(name) = utf16_to_string_until_nul(&entry.szExeFile) {
                    names.push(name);
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        names
    }
}

#[cfg(target_os = "windows")]
fn utf16_to_string_until_nul(values: &[u16]) -> Option<String> {
    let len = values
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(values.len());
    utf16_to_string(&values[..len])
}

#[cfg(target_os = "windows")]
fn utf16_to_string(values: &[u16]) -> Option<String> {
    let value = OsString::from_wide(values)
        .to_string_lossy()
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_active_window() -> Option<ActiveWindowInfo> {
    let _ = Command::new("true");
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn editor_ai_tools_from_processes(_app_name: &str) -> Vec<String> {
    Vec::new()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn terminal_ai_tools_from_processes(_app_name: &str, _process_id: Option<u32>) -> Vec<String> {
    Vec::new()
}

fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn is_self_app(app_name: &str) -> bool {
    matches!(
        app_name,
        "DayTrail"
            | "daytrail"
            | "ai.daytrail.desktop"
            | "WorkTrace AI"
            | "worktrace-ai"
            | "ai.worktrace.desktop"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        display_app_name_from_executable, push_ai_tool_from_path, single_workspace_candidate,
        terminal_ai_tools_from_ps_output, workspace_from_candidates,
    };
    #[cfg(target_os = "macos")]
    use super::is_chromium_browser;
    use std::path::Path;

    #[test]
    fn matches_focused_vscode_title_to_the_right_project_candidate() {
        let candidates = vec![
            "/Users/alice/work/LMS-production".to_string(),
            "/Users/alice/work/CFM-main".to_string(),
        ];

        assert_eq!(
            workspace_from_candidates(
                Some("deploy-production.yml — CFM-main — Untracked"),
                &candidates,
            )
            .as_deref(),
            Some("/Users/alice/work/CFM-main")
        );
        assert_eq!(
            workspace_from_candidates(
                Some("app.tsx — LMS-production — Visual Studio Code"),
                &candidates,
            )
            .as_deref(),
            Some("/Users/alice/work/LMS-production")
        );
        assert_eq!(workspace_from_candidates(Some("Code"), &candidates), None);
    }

    #[test]
    fn only_uses_single_candidate_when_no_title_match_exists() {
        assert_eq!(
            single_workspace_candidate(&["/Users/alice/work/LMS-production".to_string()])
                .as_deref(),
            Some("/Users/alice/work/LMS-production")
        );
        assert_eq!(
            single_workspace_candidate(&[
                "/Users/alice/work/LMS-production".to_string(),
                "/Users/alice/work/CFM-main".to_string()
            ]),
            None
        );
    }

    #[test]
    fn detects_ai_tools_from_recent_editor_log_paths() {
        let mut tools = Vec::new();
        push_ai_tool_from_path(
            &mut tools,
            Path::new("/Users/alice/Library/Application Support/Code/logs/window1/exthost/Anthropic.claude-code/Claude VSCode.log"),
        );
        push_ai_tool_from_path(
            &mut tools,
            Path::new("/Users/alice/Library/Application Support/Code/logs/window1/exthost/openai.chatgpt/Codex.1.log"),
        );

        assert!(tools.iter().any(|tool| tool == "Claude Code"));
        assert!(tools.iter().any(|tool| tool == "Codex"));
    }

    #[test]
    fn detects_cli_ai_tools_from_terminal_descendant_processes() {
        let output = r#"
86535     1 /Applications/Warp.app/Contents/MacOS/stable
86537 86535 /Applications/Warp.app/Contents/MacOS/stable terminal-server --parent-pid=86535
 5001 86537 -zsh -g --no_rcs
 5204  5001 node /opt/homebrew/bin/gemini --yolo
 5216  5204 /opt/homebrew/Cellar/node/25.9.0_2/bin/node --max-old-space-size=8192 /opt/homebrew/bin/gemini --yolo
40417     1 /Users/alice/.vscode/extensions/openai.chatgpt/bin/macos-aarch64/codex app-server --analytics-default-enabled
"#;

        let tools = terminal_ai_tools_from_ps_output(output, 86535);

        assert_eq!(tools, vec!["Gemini".to_string()]);
    }

    #[test]
    fn detects_cli_ai_tools_from_editor_integrated_terminal_processes() {
        let output = r#"
10000     1 /Applications/Visual Studio Code.app/Contents/MacOS/Electron
10010 10000 /Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)
10020 10000 /Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin) /Users/alice/.vscode/extensions/openai.chatgpt/bin/macos-aarch64/codex app-server
10030 10010 -zsh
10040 10030 /opt/homebrew/bin/claude --dangerously-skip-permissions
"#;

        let tools = terminal_ai_tools_from_ps_output(output, 10000);

        assert_eq!(tools, vec!["Claude Code".to_string()]);
    }

    #[test]
    fn normalizes_common_windows_executable_names() {
        let cases = [
            (
                r"C:\Users\alice\AppData\Local\Programs\Microsoft VS Code\Code.exe",
                "VS Code",
            ),
            (
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                "Google Chrome",
            ),
            (
                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
                "Microsoft Edge",
            ),
            (
                r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
                "Brave Browser",
            ),
            (r"C:\Program Files\PowerShell\7\pwsh.exe", "PowerShell"),
            (r"C:\Windows\System32\cmd.exe", "Command Prompt"),
            (
                r"C:\Program Files\JetBrains\WebStorm\bin\webstorm64.exe",
                "WebStorm",
            ),
            (
                r"C:\Users\alice\AppData\Local\Programs\cursor\Cursor.exe",
                "Cursor",
            ),
        ];

        for (path, expected) in cases {
            assert_eq!(
                display_app_name_from_executable(path).as_deref(),
                Some(expected)
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn treats_chatgpt_atlas_as_chromium_browser_for_tab_urls() {
        assert!(is_chromium_browser("ChatGPT Atlas"));
    }
}
