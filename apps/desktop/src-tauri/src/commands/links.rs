use tauri::State;

use crate::{
    error::CommandError,
    models::{
        ActivityTaskLink, ApplyRulesSummary, LinkedActivity, PrivacyDeleteSummary, SourceEvent,
        Task, TaskMatchRule, TaskMatchRuleInput,
    },
    store::WorktraceStore,
};

#[tauri::command]
pub fn search_recent_activities(
    store: State<'_, WorktraceStore>,
    query: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<SourceEvent>, CommandError> {
    store
        .search_recent_activities(query, limit.unwrap_or(25))
        .map_err(Into::into)
}

#[tauri::command]
pub fn link_activity_to_task(
    store: State<'_, WorktraceStore>,
    source_event_id: String,
    task_id: i64,
) -> Result<ActivityTaskLink, CommandError> {
    store
        .link_activity_to_task(&source_event_id, task_id)
        .map_err(Into::into)
}

#[tauri::command]
pub fn unlink_activity_from_task(
    store: State<'_, WorktraceStore>,
    source_event_id: String,
    task_id: i64,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store
        .unlink_activity_from_task(&source_event_id, task_id)
        .map_err(Into::into)
}

#[tauri::command]
pub fn list_task_activities(
    store: State<'_, WorktraceStore>,
    task_id: i64,
) -> Result<Vec<LinkedActivity>, CommandError> {
    store.list_task_activities(task_id).map_err(Into::into)
}

#[tauri::command]
pub fn list_activity_tasks(
    store: State<'_, WorktraceStore>,
    source_event_id: String,
) -> Result<Vec<Task>, CommandError> {
    store
        .list_activity_tasks(&source_event_id)
        .map_err(Into::into)
}

#[tauri::command]
pub fn list_task_rules(
    store: State<'_, WorktraceStore>,
    task_id: i64,
) -> Result<Vec<TaskMatchRule>, CommandError> {
    store.list_task_rules(task_id).map_err(Into::into)
}

#[tauri::command]
pub fn create_task_rule(
    store: State<'_, WorktraceStore>,
    task_id: i64,
    input: TaskMatchRuleInput,
) -> Result<TaskMatchRule, CommandError> {
    store.create_task_rule(task_id, input).map_err(Into::into)
}

#[tauri::command]
pub fn update_task_rule(
    store: State<'_, WorktraceStore>,
    rule_id: i64,
    input: TaskMatchRuleInput,
) -> Result<TaskMatchRule, CommandError> {
    store.update_task_rule(rule_id, input).map_err(Into::into)
}

#[tauri::command]
pub fn delete_task_rule(
    store: State<'_, WorktraceStore>,
    rule_id: i64,
) -> Result<PrivacyDeleteSummary, CommandError> {
    store.delete_task_rule(rule_id).map_err(Into::into)
}

/// Apply task rules retroactively to already-recorded activities. Pass a
/// `task_id` to run only that task's rules, or omit it to run every rule.
#[tauri::command]
pub fn apply_task_rules(
    store: State<'_, WorktraceStore>,
    task_id: Option<i64>,
) -> Result<ApplyRulesSummary, CommandError> {
    store.apply_task_rules(task_id).map_err(Into::into)
}
