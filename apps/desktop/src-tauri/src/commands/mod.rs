pub mod agents;
pub mod bridge;
pub mod commitments;
pub mod context;
pub mod export;
pub mod inbox;
pub mod offline;
pub mod outputs;
pub mod permissions;
pub mod planning;
pub mod privacy;
pub mod quick_note;
pub mod report;
pub mod search;
pub mod settings;
pub mod tasks;
pub mod today;
pub mod tracking;
pub mod work_memory;

pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        today::today,
        today::menu_bar_summary,
        today::record_loop_action,
        today::detect_project_context,
        agents::record_agent_run,
        agents::list_loop_risks,
        tasks::list_tasks,
        tasks::create_task,
        tasks::complete_task,
        commitments::create_commitment,
        commitments::list_open_commitments,
        inbox::upsert_email_thread,
        inbox::list_pending_replies,
        context::add_scratchpad_note,
        context::list_scratchpad_notes,
        context::create_state_snapshot,
        context::get_return_marker,
        outputs::record_ai_usage,
        outputs::list_ai_usage,
        outputs::record_work_output,
        outputs::list_work_outputs,
        offline::upsert_meeting,
        offline::list_meetings,
        offline::upsert_field_visit,
        offline::list_field_visits,
        offline::upsert_idle_block,
        offline::list_idle_blocks,
        privacy::clear_clipboard_history,
        privacy::delete_context_data,
        privacy::purge_captured_data,
        quick_note::add_quick_note,
        tracking::pause_tracking,
        tracking::resume_tracking,
        settings::get_settings,
        settings::update_settings,
        settings::set_ai_api_key,
        settings::install_terminal_bridge,
        settings::get_storage_locations,
        settings::export_settings_config,
        settings::import_settings_config,
        settings::backup_database,
        settings::restore_database,
        permissions::get_capture_permissions,
        permissions::open_capture_permission_settings,
        permissions::request_capture_permission,
        permissions::restart_app,
        export::export_data,
        export::export_data_range,
        export::analyze_export_range,
        report::generate_daily_report,
        report::generate_weekly_review,
        planning::generate_morning_plan,
        planning::generate_weekly_plan,
        search::search_work_memory,
        work_memory::materialize_work_memory,
        bridge::ingest_browser_event,
        quick_note::delete_quick_note
    ]
}
