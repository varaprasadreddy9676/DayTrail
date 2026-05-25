use std::{
    env, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result};
use serde_json::Value;
use url::Url;

use crate::models::{ProjectContext, TerminalBridgeMetadata};

#[derive(Debug, Clone, Default)]
pub struct ProjectDetectionSources {
    pub workspace_storage_roots: Vec<PathBuf>,
    pub terminal_bridge_metadata_paths: Vec<PathBuf>,
}

pub fn default_project_sources() -> ProjectDetectionSources {
    let mut sources = ProjectDetectionSources::default();

    if let Some(home) = dirs::home_dir() {
        sources.workspace_storage_roots.extend([
            home.join("Library/Application Support/Code/User/workspaceStorage"),
            home.join("Library/Application Support/Cursor/User/workspaceStorage"),
            home.join(".config/Code/User/workspaceStorage"),
            home.join(".config/Cursor/User/workspaceStorage"),
            home.join("AppData/Roaming/Code/User/workspaceStorage"),
            home.join("AppData/Roaming/Cursor/User/workspaceStorage"),
        ]);
        sources
            .terminal_bridge_metadata_paths
            .push(home.join(".daytrail/terminal-bridge.json"));
        sources
            .terminal_bridge_metadata_paths
            .push(home.join(".worktrace/terminal-bridge.json"));
    }

    if let Ok(path) = env::var("DAYTRAIL_TERMINAL_BRIDGE") {
        sources
            .terminal_bridge_metadata_paths
            .insert(0, PathBuf::from(path));
    }

    if let Ok(path) = env::var("WORKTRACE_TERMINAL_BRIDGE") {
        sources
            .terminal_bridge_metadata_paths
            .insert(0, PathBuf::from(path));
    }

    let user = env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into());
    sources
        .terminal_bridge_metadata_paths
        .push(env::temp_dir().join(format!("daytrail-terminal-bridge-{user}.json")));
    sources
        .terminal_bridge_metadata_paths
        .push(env::temp_dir().join(format!("worktrace-terminal-bridge-{user}.json")));

    sources
}

pub fn detect_project_from_sources(sources: ProjectDetectionSources) -> Result<ProjectContext> {
    let contexts = detect_project_candidates_from_sources(sources)?;
    contexts
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no project context detected"))
}

pub fn detect_project_candidates_from_sources(
    sources: ProjectDetectionSources,
) -> Result<Vec<ProjectContext>> {
    let editor_hint = sources
        .workspace_storage_roots
        .iter()
        .find_map(|root| editor_hint_for_path(root));
    let mut contexts = Vec::new();

    for path in &sources.terminal_bridge_metadata_paths {
        if let Some(context) = read_terminal_bridge(path, editor_hint.clone())? {
            contexts.push(context);
        }
    }

    for root in &sources.workspace_storage_roots {
        contexts.extend(read_workspace_storage_candidates(root)?);
    }

    let mut seen = std::collections::HashSet::new();
    contexts.retain(|context| seen.insert(context.path.clone()));
    Ok(contexts)
}

fn read_terminal_bridge(
    path: &Path,
    editor_hint: Option<String>,
) -> Result<Option<ProjectContext>> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read terminal bridge {}", path.display()))?;
    let metadata: TerminalBridgeMetadata = serde_json::from_str(&contents)
        .with_context(|| format!("invalid terminal bridge JSON {}", path.display()))?;

    if metadata.cwd.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(ProjectContext {
        path: metadata.cwd,
        source: "terminal-bridge".into(),
        editor_hint,
    }))
}

fn read_workspace_storage_candidates(root: &Path) -> Result<Vec<ProjectContext>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let editor_hint = editor_hint_for_path(root);
    let mut entries = fs::read_dir(root)
        .with_context(|| format!("failed to read workspace storage {}", root.display()))?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| std::cmp::Reverse(workspace_activity_time(&entry.path())));

    let mut contexts = Vec::new();
    for entry in entries {
        let workspace_json = entry.path().join("workspace.json");
        if !workspace_json.exists() {
            continue;
        }

        let contents = fs::read_to_string(&workspace_json)
            .with_context(|| format!("failed to read {}", workspace_json.display()))?;
        let value: Value = serde_json::from_str(&contents)
            .with_context(|| format!("invalid workspace JSON {}", workspace_json.display()))?;

        if let Some(path) = extract_workspace_path(&value) {
            contexts.push(ProjectContext {
                path,
                source: "workspace-storage".into(),
                editor_hint: editor_hint.clone(),
            });
        }
    }

    Ok(contexts)
}

fn workspace_activity_time(path: &Path) -> SystemTime {
    [
        path.join("state.vscdb"),
        path.join("state.vscdb.backup"),
        path.join("workspace.json"),
    ]
    .into_iter()
    .filter_map(|path| path.metadata().ok()?.modified().ok())
    .max()
    .or_else(|| path.metadata().ok()?.modified().ok())
    .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn extract_workspace_path(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in ["folder", "workspace", "workspaceFolder", "path"] {
                if let Some(Value::String(raw)) = map.get(key) {
                    if let Some(path) = normalize_path(raw) {
                        return Some(path);
                    }
                }
            }

            for nested in map.values() {
                if let Some(path) = extract_workspace_path(nested) {
                    return Some(path);
                }
            }
            None
        }
        Value::Array(values) => values.iter().find_map(extract_workspace_path),
        Value::String(raw) => normalize_path(raw),
        _ => None,
    }
}

fn normalize_path(raw: &str) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }

    if let Ok(url) = Url::parse(raw) {
        if url.scheme() == "file" {
            return url
                .to_file_path()
                .ok()
                .map(|path| path.display().to_string());
        }
    }

    Some(raw.to_string())
}

fn editor_hint_for_path(path: &Path) -> Option<String> {
    let rendered = path.display().to_string();
    if rendered.contains("Cursor") {
        Some("Cursor".into())
    } else if rendered.contains("Code") {
        Some("Code".into())
    } else {
        None
    }
}
