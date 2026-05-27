use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    app_icons::app_icon_data_url,
    error::CommandError,
    models::{DatabaseTransferResult, Settings, SettingsPatch, StorageLocationInfo},
    store::WorktraceStore,
};

#[tauri::command]
pub fn get_app_icon(app_name: String) -> Option<String> {
    app_icon_data_url(&app_name)
}

const TERMINAL_BRIDGE_SCRIPT: &str =
    include_str!("../../../../../scripts/worktrace-terminal-bridge.sh");
const TERMINAL_BRIDGE_BLOCK_START: &str = "# >>> DayTrail terminal bridge >>>";
const TERMINAL_BRIDGE_BLOCK_END: &str = "# <<< DayTrail terminal bridge <<<";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalBridgeInstallResult {
    pub shell: String,
    pub profile_path: String,
    pub bridge_script_path: String,
    pub metadata_path: String,
    pub already_installed: bool,
    pub message: String,
}

#[tauri::command]
pub fn get_settings(store: State<'_, WorktraceStore>) -> Result<Settings, CommandError> {
    store.get_settings().map_err(Into::into)
}

#[tauri::command]
pub fn update_settings(
    store: State<'_, WorktraceStore>,
    patch: SettingsPatch,
) -> Result<Settings, CommandError> {
    store.update_settings(patch).map_err(Into::into)
}

#[tauri::command]
pub fn set_ai_api_key(
    store: State<'_, WorktraceStore>,
    provider: String,
    api_key: String,
) -> Result<Settings, CommandError> {
    store
        .set_ai_api_key(&provider, &api_key)
        .map_err(Into::into)
}

#[tauri::command]
pub fn install_terminal_bridge(
    app: AppHandle,
    store: State<'_, WorktraceStore>,
) -> Result<TerminalBridgeInstallResult, CommandError> {
    let result = install_terminal_bridge_inner(app, &store)?;
    Ok(result)
}

#[tauri::command]
pub fn get_storage_locations(
    store: State<'_, WorktraceStore>,
) -> Result<StorageLocationInfo, CommandError> {
    store.storage_locations().map_err(Into::into)
}

#[tauri::command]
pub fn export_settings_config(store: State<'_, WorktraceStore>) -> Result<String, CommandError> {
    store.export_settings_config_json().map_err(Into::into)
}

#[tauri::command]
pub fn import_settings_config(
    store: State<'_, WorktraceStore>,
    config_json: String,
) -> Result<Settings, CommandError> {
    store
        .import_settings_config_json(&config_json)
        .map_err(Into::into)
}

#[tauri::command]
pub fn backup_database(
    store: State<'_, WorktraceStore>,
) -> Result<DatabaseTransferResult, CommandError> {
    store.backup_database_to_default().map_err(Into::into)
}

#[tauri::command]
pub fn restore_database(
    store: State<'_, WorktraceStore>,
    path: String,
) -> Result<DatabaseTransferResult, CommandError> {
    store.restore_database_from_path(path).map_err(Into::into)
}

fn install_terminal_bridge_inner(
    app: AppHandle,
    store: &WorktraceStore,
) -> anyhow::Result<TerminalBridgeInstallResult> {
    let app_data_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&app_data_dir)?;

    let bridge_script_path = app_data_dir.join("terminal-bridge.sh");
    let metadata_path = app_data_dir.join("terminal-bridge.json");
    fs::write(&bridge_script_path, TERMINAL_BRIDGE_SCRIPT)?;
    make_executable(&bridge_script_path)?;

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let (profile_path, hook_kind) = shell_profile_for(&shell)?;
    if let Some(parent) = profile_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let existing = fs::read_to_string(&profile_path).unwrap_or_default();
    let block = terminal_bridge_profile_block(&bridge_script_path, &metadata_path, hook_kind);
    let already_installed = existing.contains(TERMINAL_BRIDGE_BLOCK_START)
        && existing.contains(&bridge_script_path.display().to_string())
        && existing.contains(&metadata_path.display().to_string());
    let next_profile = replace_marked_block(&existing, &block);
    if next_profile != existing {
        fs::write(&profile_path, next_profile)?;
    }

    store.update_settings(SettingsPatch {
        terminal_bridge_path: Some(metadata_path.display().to_string()),
        ..SettingsPatch::default()
    })?;

    Ok(TerminalBridgeInstallResult {
        shell,
        profile_path: profile_path.display().to_string(),
        bridge_script_path: bridge_script_path.display().to_string(),
        metadata_path: metadata_path.display().to_string(),
        already_installed,
        message: if already_installed {
            "Terminal bridge was already installed. Open a new terminal tab and run any command."
                .to_string()
        } else {
            "Terminal bridge installed. Open a new terminal tab and run any command.".to_string()
        },
    })
}

fn shell_profile_for(shell: &str) -> anyhow::Result<(PathBuf, &'static str)> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("failed to resolve home dir"))?;
    let shell_name = shell.rsplit('/').next().unwrap_or(shell);
    match shell_name {
        "zsh" => Ok((home.join(".zshrc"), "zsh")),
        "bash" => Ok((home.join(".bashrc"), "bash")),
        other => anyhow::bail!("unsupported shell for automatic terminal bridge install: {other}"),
    }
}

fn terminal_bridge_profile_block(
    bridge_script_path: &Path,
    metadata_path: &Path,
    hook_kind: &str,
) -> String {
    let hook_arg = if hook_kind == "bash" {
        "--print-bash-hook"
    } else {
        "--print-zsh-hook"
    };
    format!(
        "{TERMINAL_BRIDGE_BLOCK_START}\nexport DAYTRAIL_TERMINAL_BRIDGE={}\nif [ -f {} ]; then\n  source <({} {hook_arg})\nfi\n{TERMINAL_BRIDGE_BLOCK_END}\n",
        shell_quote(&metadata_path.display().to_string()),
        shell_quote(&bridge_script_path.display().to_string()),
        shell_quote(&bridge_script_path.display().to_string()),
    )
}

fn replace_marked_block(existing: &str, block: &str) -> String {
    if let Some(start) = existing.find(TERMINAL_BRIDGE_BLOCK_START) {
        if let Some(relative_end) = existing[start..].find(TERMINAL_BRIDGE_BLOCK_END) {
            let end = start + relative_end + TERMINAL_BRIDGE_BLOCK_END.len();
            let mut output = String::new();
            output.push_str(existing[..start].trim_end());
            output.push_str("\n\n");
            output.push_str(block.trim_end());
            output.push('\n');
            output.push_str(existing[end..].trim_start_matches(['\r', '\n']));
            return output;
        }
    }

    let mut output = existing.trim_end().to_string();
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    output.push_str(block.trim_end());
    output.push('\n');
    output
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn make_executable(path: &PathBuf) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &PathBuf) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{replace_marked_block, shell_quote, terminal_bridge_profile_block};
    use std::path::PathBuf;

    #[test]
    fn terminal_bridge_profile_block_is_idempotent_and_shell_quoted() {
        let block = terminal_bridge_profile_block(
            &PathBuf::from("/Users/alice/Application Support/terminal-bridge.sh"),
            &PathBuf::from("/Users/alice/Application Support/terminal-bridge.json"),
            "zsh",
        );
        assert!(block.contains("--print-zsh-hook"));
        assert!(block.contains("'/Users/alice/Application Support/terminal-bridge.sh'"));

        let first = replace_marked_block("export PATH=/opt/bin:$PATH\n", &block);
        let second = replace_marked_block(&first, &block);
        assert_eq!(first, second);
    }

    #[test]
    fn shell_quote_handles_single_quotes() {
        assert_eq!(shell_quote("/tmp/Day'Trail"), "'/tmp/Day'\\''Trail'");
    }
}
