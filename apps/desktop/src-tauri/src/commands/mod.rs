pub mod agents;
pub mod bridge;
pub mod insights;
pub mod chat;
pub mod calendar;
pub mod commitments;
pub mod context;
pub mod export;
pub mod focus;
pub mod inbox;
pub mod links;
pub mod offline;
pub mod outputs;
pub mod permissions;
pub mod planning;
pub mod privacy;
pub mod quick_note;
pub mod recovery;
pub mod report;
pub mod review;
pub mod search;
pub mod settings;
pub mod tasks;
pub mod today;
pub mod tracking;
pub mod updates;
pub mod work_context;
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
        tasks::update_task,
        tasks::draft_tasks_from_text,
        tasks::complete_task,
        tasks::snooze_task,
        tasks::delete_task,
        links::link_activity_to_task,
        links::unlink_activity_from_task,
        links::search_recent_activities,
        links::list_task_activities,
        links::list_activity_tasks,
        links::list_task_rules,
        links::create_task_rule,
        links::update_task_rule,
        links::delete_task_rule,
        links::apply_task_rules,
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
        offline::delete_idle_block,
        calendar::upsert_calendar_event,
        calendar::list_calendar_events,
        focus::upsert_focus_session,
        focus::list_focus_sessions,
        privacy::clear_clipboard_history,
        privacy::delete_context_data,
        privacy::purge_captured_data,
        privacy::prune_captured_data,
        privacy::prune_completed_tasks,
        privacy::apply_retention_policy,
        quick_note::add_quick_note,
        tracking::pause_tracking,
        tracking::resume_tracking,
        settings::get_app_icon,
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
        permissions::trigger_browser_automation_prompt,
        permissions::reset_and_request_accessibility,
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
        quick_note::delete_quick_note,
        work_context::get_active_work_context,
        work_context::set_active_work_context,
        work_context::clear_active_work_context,
        review::review_session,
        review::list_sessions_for_review,
        review::export_timesheet_markdown,
        updates::check_for_updates,
        updates::app_version,
        updates::brew_upgrade_daytrail,
        focus::start_focus_session,
        focus::end_focus_session,
        focus::get_focus_session,
        focus::snooze_focus_session,
        recovery::record_recovery_event,
        recovery::get_recovery_summary,
        recovery::list_recovery_events,
        recovery::snooze_recovery,
        recovery::skip_recovery,
        recovery::take_recovery_break,
        chat::chat_query,
        insights::list_proactive_insights,
        insights::dismiss_insight,
        insights::mark_insights_seen,
        insights::count_unseen_insights
    ]
}
