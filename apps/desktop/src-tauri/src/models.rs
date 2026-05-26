use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    Done,
}

impl TaskStatus {
    pub fn as_db_value(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
        }
    }
}

impl TryFrom<&str> for TaskStatus {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "open" => Ok(Self::Open),
            "done" => Ok(Self::Done),
            other => anyhow::bail!("unknown task status: {other}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub status: TaskStatus,
    pub due_date: Option<String>,
    pub source: Option<String>,
    pub project_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub title: String,
    pub due_date: Option<String>,
    pub source: Option<String>,
    pub project_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNote {
    pub id: i64,
    pub body: String,
    pub source: Option<String>,
    pub project_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub idle_timeout_minutes: i64,
    pub export_format: String,
    pub launch_at_login: bool,
    pub browser_bridge_enabled: bool,
    pub terminal_bridge_path: Option<String>,
    pub excluded_apps: Vec<String>,
    pub excluded_domains: Vec<String>,
    pub excluded_projects: Vec<String>,
    pub ai_provider: String,
    pub ai_model: String,
    pub ai_endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_api_key_ref: Option<String>,
    pub ai_redact_secrets: bool,
    pub full_clipboard_history: bool,
    #[serde(default = "default_experience_mode")]
    pub experience_mode: String,
    #[serde(default)]
    pub show_system_apps: bool,
    #[serde(default)]
    pub show_raw_events: bool,
    #[serde(default)]
    pub show_capture_confidence: bool,
    #[serde(default = "default_show_ai_details")]
    pub show_ai_details: String,
}

fn default_experience_mode() -> String {
    "simple".into()
}

fn default_show_ai_details() -> String {
    "summary".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle_timeout_minutes: 10,
            export_format: "json".into(),
            launch_at_login: false,
            browser_bridge_enabled: true,
            terminal_bridge_path: None,
            excluded_apps: Vec::new(),
            excluded_domains: Vec::new(),
            excluded_projects: Vec::new(),
            ai_provider: "Ollama Local".into(),
            ai_model: "llama3.1".into(),
            ai_endpoint: "http://127.0.0.1:11434/v1/chat/completions".into(),
            ai_api_key_ref: None,
            ai_redact_secrets: true,
            full_clipboard_history: false,
            experience_mode: default_experience_mode(),
            show_system_apps: false,
            show_raw_events: false,
            show_capture_confidence: false,
            show_ai_details: default_show_ai_details(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub idle_timeout_minutes: Option<i64>,
    pub export_format: Option<String>,
    pub launch_at_login: Option<bool>,
    pub browser_bridge_enabled: Option<bool>,
    pub terminal_bridge_path: Option<String>,
    pub excluded_apps: Option<Vec<String>>,
    pub excluded_domains: Option<Vec<String>>,
    pub excluded_projects: Option<Vec<String>>,
    pub ai_provider: Option<String>,
    pub ai_model: Option<String>,
    pub ai_endpoint: Option<String>,
    pub ai_redact_secrets: Option<bool>,
    pub full_clipboard_history: Option<bool>,
    pub experience_mode: Option<String>,
    pub show_system_apps: Option<bool>,
    pub show_raw_events: Option<bool>,
    pub show_capture_confidence: Option<bool>,
    pub show_ai_details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsConfigPayload {
    pub schema_version: u16,
    pub exported_at: String,
    pub settings: Settings,
    pub secrets_exported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageLocationInfo {
    pub database_path: String,
    pub backup_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseTransferResult {
    pub path: String,
    pub bytes: u64,
    pub generated_at: String,
    pub pre_restore_backup_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePermissionSummary {
    pub platform: String,
    pub setup_required: bool,
    pub all_required_granted: bool,
    pub app_path: Option<String>,
    pub executable_path: Option<String>,
    pub restart_recommended: bool,
    pub diagnostics: Vec<String>,
    pub checks: Vec<CapturePermissionCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePermissionCheck {
    pub id: String,
    pub label: String,
    pub required: bool,
    pub status: String,
    pub detail: String,
    pub settings_label: Option<String>,
    pub settings_url: Option<String>,
    pub action_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseState {
    pub paused: bool,
    pub reason: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaySnapshot {
    pub local_date: String,
    pub tasks: Vec<Task>,
    pub quick_notes: Vec<QuickNote>,
    pub commitments: Vec<Commitment>,
    pub pending_replies: Vec<EmailThread>,
    pub ai_outputs: Vec<WorkOutput>,
    pub meetings: Vec<Meeting>,
    pub field_visits: Vec<FieldVisit>,
    pub idle_blocks: Vec<IdleBlock>,
    pub source_events: Vec<SourceEvent>,
    pub work_sessions: Vec<WorkSessionSummary>,
    pub parallel_streams: Vec<ParallelStreamSummary>,
    pub ai_usage_summary: AiUsageSummary,
    pub app_usage_summary: AppUsageSummary,
    pub automation_candidates: Vec<AutomationCandidate>,
    pub capture_health: CaptureHealthSummary,
    pub unclosed_loop_inbox: Vec<UnclosedLoopItem>,
    pub ai_output_ledger: Vec<AiOutputLedgerItem>,
    pub menu_bar_summary: MenuBarSummary,
    pub loop_risks: Vec<LoopRisk>,
    pub next_best_action: Option<NextBestAction>,
    pub pause_state: PauseState,
    pub settings: Settings,
    pub project_context: Option<ProjectContext>,
    pub active_work_context: Option<ActiveWorkContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageSummary {
    pub total_duration_ms: i64,
    pub tools: Vec<AiToolUsage>,
    pub contexts: Vec<AiContextUsage>,
    pub output_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolUsage {
    pub tool: String,
    pub duration_ms: i64,
    pub events: usize,
    pub contexts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiContextUsage {
    pub label: String,
    pub duration_ms: i64,
    pub events: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationCandidate {
    pub id: String,
    pub title: String,
    pub signal: String,
    pub reason: String,
    pub occurrences: usize,
    pub duration_ms: i64,
    pub example_sources: Vec<String>,
    pub suggested_steps: Vec<String>,
    pub related_ai_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageSummary {
    pub total_duration_ms: i64,
    pub apps: Vec<AppUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    pub app: String,
    pub category: String,
    pub duration_ms: i64,
    pub events: usize,
    pub projects: Vec<AppProjectUsage>,
    pub ai_tools: Vec<AiToolUsage>,
    /// Per-file (or per-tab) time breakdown, populated for editors and browsers.
    pub files: Vec<FileUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileUsage {
    /// Filename (for editors) or page title / domain (for browsers).
    pub name: String,
    /// Project folder, workspace name, or site domain.
    pub context: Option<String>,
    pub duration_ms: i64,
    pub events: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppProjectUsage {
    pub label: String,
    pub contexts: Vec<String>,
    pub duration_ms: i64,
    pub events: usize,
    pub ai_tools: Vec<AiToolUsage>,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureHealthSummary {
    pub status: String,
    pub headline: String,
    pub updated_at: i64,
    pub checks: Vec<CaptureHealthCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureHealthCheck {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: String,
    pub last_seen_at: Option<i64>,
    pub evidence_count: usize,
    pub action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnclosedLoopItem {
    pub id: String,
    pub category: String,
    pub title: String,
    pub detail: String,
    pub source: String,
    pub risk: String,
    pub status: String,
    pub primary_action: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopAction {
    pub id: String,
    pub action: String,
    pub snoozed_until: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopActionInput {
    pub id: String,
    pub action: String,
    pub snoozed_until: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiOutputLedgerItem {
    pub id: String,
    pub title: String,
    pub tool: String,
    pub source_context: String,
    pub destination: String,
    pub status: String,
    pub duration_ms: i64,
    pub evidence_ids: Vec<String>,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuBarSummary {
    pub current_work: String,
    pub detail: String,
    pub capture_state: String,
    pub ai_usage: String,
    pub open_loops: usize,
    pub next_action: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commitment {
    pub id: String,
    pub title: String,
    pub source: Option<String>,
    pub owner: Option<String>,
    pub due_at: Option<i64>,
    pub status: String,
    pub confidence: f64,
    pub evidence_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentInput {
    pub id: Option<String>,
    pub title: String,
    pub source: Option<String>,
    pub owner: Option<String>,
    pub due_at: Option<i64>,
    pub confidence: Option<f64>,
    pub evidence_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailThread {
    pub id: String,
    pub subject: String,
    pub latest_sender: Option<String>,
    pub latest_at: Option<i64>,
    pub pending_reply: bool,
    pub evidence_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailThreadInput {
    pub id: String,
    pub subject: String,
    pub latest_sender: Option<String>,
    pub latest_at: Option<i64>,
    pub pending_reply: bool,
    pub evidence_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPayload {
    pub generated_at: String,
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub timesheet_rows: Vec<TimesheetRow>,
    pub ai_contribution_rows: Vec<AiContributionRow>,
    pub tasks: Vec<Task>,
    pub quick_notes: Vec<QuickNote>,
    pub commitments: Vec<Commitment>,
    pub pending_replies: Vec<EmailThread>,
    pub outputs: Vec<WorkOutput>,
    pub source_events: Vec<SourceEvent>,
    pub work_sessions: Vec<WorkSessionSummary>,
    pub idle_blocks: Vec<IdleBlock>,
    pub ai_usage: Vec<AiUsage>,
    pub app_usage_summary: AppUsageSummary,
    pub ai_usage_summary: AiUsageSummary,
    pub automation_candidates: Vec<AutomationCandidate>,
    pub unclosed_loop_inbox: Vec<UnclosedLoopItem>,
    pub settings: Settings,
    pub pause_state: PauseState,
    pub project_context: Option<ProjectContext>,
    pub active_work_context: Option<ActiveWorkContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimesheetRow {
    pub id: String,
    pub local_date: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_ms: i64,
    pub title: String,
    pub category: String,
    pub app: String,
    pub project_or_client: String,
    pub ai_used: bool,
    pub ai_tools: Vec<String>,
    pub confidence_percent: i64,
    pub evidence_ids: Vec<String>,
    pub billing_status: String,
    pub billable: bool,
    pub client_label: Option<String>,
    pub project_label: Option<String>,
    pub ticket_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiContributionRow {
    pub id: String,
    pub tool: String,
    pub app: String,
    pub project_or_client: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_ms: i64,
    pub title: String,
    pub destination: String,
    pub status: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRangeInput {
    pub from_date: Option<String>,
    pub to_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportOutput {
    pub generated_at: String,
    pub report_type: String,
    pub title: String,
    pub body_markdown: String,
    pub used_ai: bool,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub snippet: String,
    pub source: Option<String>,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningOutput {
    pub generated_at: String,
    pub horizon: String,
    pub title: String,
    pub body_markdown: String,
    pub must_close: Vec<PlanningItem>,
    pub should_progress: Vec<PlanningItem>,
    pub can_defer: Vec<PlanningItem>,
    pub waiting: Vec<PlanningItem>,
    pub at_risk: Vec<PlanningItem>,
    pub capacity_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningItem {
    pub title: String,
    pub source: String,
    pub reason: String,
    pub due_at: Option<i64>,
    pub priority: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_ms: i64,
    pub ai_used: bool,
    pub confidence_percent: i64,
    pub summary: Option<String>,
    pub evidence_event_ids: Vec<String>,
    // billing / review fields
    pub billing_status: String,
    pub billable: bool,
    pub client_label: Option<String>,
    pub project_label: Option<String>,
    pub ticket_id: Option<String>,
    pub review_notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSessionInput {
    pub session_id: String,
    pub billing_status: Option<String>,
    pub billable: Option<bool>,
    pub client_label: Option<String>,
    pub project_label: Option<String>,
    pub ticket_id: Option<String>,
    pub review_notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelStreamSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub summary: Option<String>,
    pub event_ids: Vec<String>,
    pub next_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextBestAction {
    pub title: String,
    pub reason: String,
    pub source_type: String,
    pub source_id: String,
    pub priority: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopRisk {
    pub id: String,
    pub risk_type: String,
    pub title: String,
    pub source: String,
    pub reason: String,
    pub priority: i64,
    pub evidence_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: String,
    pub context_id: Option<String>,
    pub tool_name: Option<String>,
    pub command_label: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub status: Option<String>,
    pub exit_code: Option<i64>,
    pub summary: Option<String>,
    pub error_tail: Option<String>,
    pub notified: bool,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunInput {
    pub id: Option<String>,
    pub context_id: Option<String>,
    pub tool_name: Option<String>,
    pub command_label: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub status: Option<String>,
    pub exit_code: Option<i64>,
    pub summary: Option<String>,
    pub error_tail: Option<String>,
    pub notified: Option<bool>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkMemorySummary {
    pub source_events: usize,
    pub work_sessions: usize,
    pub parallel_streams: usize,
    pub graph_edges: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsage {
    pub id: String,
    pub provider: Option<String>,
    pub tool_name: Option<String>,
    pub thread_title: Option<String>,
    pub context_id: Option<String>,
    pub prompt_summary: Option<String>,
    pub output_summary: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub metadata_json: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageInput {
    pub id: Option<String>,
    pub provider: Option<String>,
    pub tool_name: Option<String>,
    pub thread_title: Option<String>,
    pub context_id: Option<String>,
    pub prompt_summary: Option<String>,
    pub output_summary: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiPromptInput {
    pub prompt_kind: String,
    pub instruction: String,
    pub context_markdown: String,
    pub max_input_chars: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiPromptOutput {
    pub provider: String,
    pub model: String,
    pub body_markdown: String,
    pub audit_id: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkOutput {
    pub id: String,
    pub output_type: String,
    pub title: String,
    pub source: Option<String>,
    pub ai_assisted: bool,
    pub status: String,
    pub evidence_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkOutputInput {
    pub id: Option<String>,
    pub output_type: String,
    pub title: String,
    pub source: Option<String>,
    pub ai_assisted: Option<bool>,
    pub status: Option<String>,
    pub evidence_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyDeleteSummary {
    pub deleted_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub starts_at: Option<i64>,
    pub ends_at: Option<i64>,
    pub attendees_json: Option<String>,
    pub summary: Option<String>,
    pub actions_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingInput {
    pub id: Option<String>,
    pub title: String,
    pub starts_at: Option<i64>,
    pub ends_at: Option<i64>,
    pub attendees_json: Option<String>,
    pub summary: Option<String>,
    pub actions_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldVisit {
    pub id: String,
    pub client_label: Option<String>,
    pub starts_at: i64,
    pub ends_at: Option<i64>,
    pub location_label: Option<String>,
    pub debrief: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldVisitInput {
    pub id: Option<String>,
    pub client_label: Option<String>,
    pub starts_at: Option<i64>,
    pub ends_at: Option<i64>,
    pub location_label: Option<String>,
    pub debrief: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdleBlock {
    pub id: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_ms: i64,
    pub category: Option<String>,
    pub classified: bool,
    pub evidence_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdleBlockInput {
    pub id: Option<String>,
    pub started_at: i64,
    pub ended_at: i64,
    pub category: Option<String>,
    pub classified: Option<bool>,
    pub evidence_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceContext {
    pub id: String,
    pub context_key: String,
    pub context_type: String,
    pub label: Option<String>,
    pub git_repo: Option<String>,
    pub git_branch: Option<String>,
    pub folder_path: Option<String>,
    pub domain: Option<String>,
    pub email_thread_id: Option<String>,
    pub project_id: Option<String>,
    pub last_seen_at: Option<i64>,
    pub metadata_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchpadNote {
    pub id: String,
    pub context_id: String,
    pub note: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchpadNoteInput {
    pub id: Option<String>,
    pub context_id: String,
    pub note: String,
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSnapshot {
    pub id: String,
    pub context_id: String,
    pub trigger_type: String,
    pub snapshot_type: String,
    pub summary: Option<String>,
    pub terminal_tail: Option<String>,
    pub git_diff_summary: Option<String>,
    pub active_file: Option<String>,
    pub cursor_position: Option<String>,
    pub ai_context_summary: Option<String>,
    pub metadata_json: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSnapshotInput {
    pub id: Option<String>,
    pub context_id: String,
    pub trigger_type: String,
    pub snapshot_type: String,
    pub summary: Option<String>,
    pub terminal_tail: Option<String>,
    pub git_diff_summary: Option<String>,
    pub active_file: Option<String>,
    pub cursor_position: Option<String>,
    pub ai_context_summary: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnMarker {
    pub context_id: String,
    pub context: Option<WorkspaceContext>,
    pub latest_snapshot: Option<StateSnapshot>,
    pub pinned_notes: Vec<ScratchpadNote>,
    pub recent_notes: Vec<ScratchpadNote>,
    pub recent_sessions: Vec<WorkSessionSummary>,
    pub suggested_next_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContext {
    pub path: String,
    pub source: String,
    pub editor_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalBridgeMetadata {
    pub cwd: String,
    pub shell: Option<String>,
    pub terminal: Option<String>,
    pub updated_at: Option<String>,
    pub event_type: Option<String>,
    pub last_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBridgeEvent {
    pub url: Option<String>,
    pub title: Option<String>,
    pub source: Option<String>,
    pub captured_at: Option<String>,
    pub tab_id: Option<i64>,
    pub window_id: Option<i64>,
    pub incognito: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEventInput {
    pub id: Option<String>,
    pub source: String,
    pub event_type: String,
    pub app: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub workspace_key: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub sensitivity: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEvent {
    pub id: String,
    pub source: String,
    pub event_type: String,
    pub app: Option<String>,
    pub title: Option<String>,
    pub domain: Option<String>,
    pub url_redacted: Option<String>,
    pub workspace_key: Option<String>,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_ms: i64,
    pub sensitivity: String,
    pub metadata_json: Option<String>,
    pub created_at: i64,
}

// ── Git context ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitContext {
    pub branch: Option<String>,
    pub repo_root: Option<String>,
    pub remote_origin: Option<String>,
    pub ticket_id: Option<String>,
}

// ── Active work context ───────────────────────────────────────────────────────

/// The user's explicitly-set current work context.  Stored as a singleton row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWorkContext {
    pub client: Option<String>,
    pub project: Option<String>,
    pub task: Option<String>,
    pub ticket_id: Option<String>,
    pub billable: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWorkContextInput {
    pub client: Option<String>,
    pub project: Option<String>,
    pub task: Option<String>,
    pub ticket_id: Option<String>,
    pub billable: Option<bool>,
}
