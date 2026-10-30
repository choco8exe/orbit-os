mod core;
mod modules;
mod plugins;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init());

    builder = builder.setup(|app| {
        use tauri::Manager;
        if let Err(error) = modules::maintenance::cleanup_retired_atlas_data(&app.handle()) {
            eprintln!("ORBIT: retired Atlas key cleanup will retry next launch: {error}");
        }
        if let Err(error) = modules::maintenance::apply_staged_restore(&app.handle()) {
            // Maintenance quarantines a failed staged restore and exposes its status to the UI.
            // Keep the application available while preserving an explicit failure diagnostic.
            eprintln!("ORBIT: staged restore failed; continuing startup after quarantine: {error}");
        }
        app.manage(core::CoreState::default());
        if modules::echo::initialize(&app.handle()).is_err() {
            eprintln!("ORBIT: Echo could not initialize; clipboard services remain unavailable.");
        }
        if modules::pulsar::initialize(&app.handle()).is_err() {
            eprintln!("ORBIT: Pulsar could not initialize; automation services remain unavailable.");
        }
        if plugins::initialize(&app.handle()).is_err() {
            eprintln!("ORBIT: Plugin registry could not initialize; Plugin Center mutations remain unavailable.");
        }
        if modules::activity::initialize(&app.handle()).is_err() {
            eprintln!("ORBIT: Pulse could not initialize; activity tracking remains unavailable.");
        }
        if modules::creator_plugins::initialize(&app.handle()).is_err() {
            eprintln!("ORBIT: FL Studio notes could not initialize; project notes remain unavailable.");
        }
        modules::streamdeck_bridge::initialize(&app.handle());
        if modules::fl_studio::initialize(&app.handle()).is_err() {
            eprintln!("ORBIT: Project Shelf could not initialize; its local library remains unavailable.");
        }
        if modules::workspaces::initialize(&app.handle()).is_err() {
            eprintln!("ORBIT: Workspaces could not initialize; local session groups remain unavailable.");
        }
        if modules::downloads_organizer::initialize(&app.handle()).is_err() {
            eprintln!("ORBIT: Downloads Organizer could not initialize; local sorting remains unavailable.");
        }
        if modules::game_sessions::initialize(&app.handle()).is_err() {
            eprintln!("ORBIT: Game Sessions could not initialize; local tracking remains unavailable.");
        }
        Ok(())
    });

    builder
        .invoke_handler(tauri::generate_handler![
            core::get_system_snapshot,
            modules::echo::echo_snapshot,
            modules::echo::echo_get_entry,
            modules::echo::echo_search,
            modules::echo::echo_set_monitoring,
            modules::echo::echo_set_preferences,
            modules::echo::echo_copy_entry,
            modules::echo::echo_set_favorite,
            modules::echo::echo_set_pinned,
            modules::echo::echo_assign_collection,
            modules::echo::echo_create_collection,
            modules::echo::echo_delete_entry,
            modules::echo::echo_delete_entries,
            modules::echo::echo_find_duplicates,
            modules::echo::echo_remove_duplicates,
            modules::echo::echo_clear_history,
            modules::pulsar::pulsar_list,
            modules::pulsar::pulsar_save_scenario,
            modules::pulsar::pulsar_create_draft,
            modules::pulsar::pulsar_delete_scenario,
            modules::pulsar::pulsar_set_enabled,
            modules::pulsar::pulsar_simulate,
            modules::pulsar::pulsar_run,
            modules::pulsar::pulsar_logs,
            modules::pulsar::pulsar_fire_event,
            modules::pulsar::pulsar_global_pause_status,
            modules::pulsar::pulsar_global_pause_set,
            plugins::plugin_list_catalog,
            plugins::plugin_list_states,
            plugins::plugin_install,
            plugins::plugin_enable,
            plugins::plugin_disable,
            plugins::plugin_set_permissions,
            plugins::plugin_secret_status,
            plugins::plugin_set_secret,
            plugins::plugin_remove_secret,
            plugins::plugin_retired_credential_status,
            plugins::plugin_cleanup_retired_credentials,
            plugins::plugin_uninstall,
            modules::fl_studio::fl_studio_list_projects,
            modules::fl_studio::fl_studio_add_project,
            modules::fl_studio::fl_studio_set_pinned,
            modules::fl_studio::fl_studio_open_project,
            modules::fl_studio::fl_studio_remove_project,
            modules::fl_studio::fl_studio_add_tag,
            modules::fl_studio::fl_studio_remove_tag,
            modules::workspaces::workspaces_list,
            modules::workspaces::workspaces_create,
            modules::workspaces::workspaces_rename,
            modules::workspaces::workspaces_delete,
            modules::workspaces::workspaces_add_app,
            modules::workspaces::workspaces_remove_app,
            modules::workspaces::workspaces_launch,
            modules::activity::activity_pulse_snapshot,
            modules::activity::activity_pulse_set_monitoring,
            modules::activity::activity_pulse_set_preferences,
            modules::activity::activity_pulse_set_category,
            modules::activity::activity_pulse_set_app_goal,
            modules::activity::activity_pulse_clear_history,
            modules::activity::activity_pulse_export_csv,
            modules::creator_plugins::creator_plugins_studio_get_note,
            modules::creator_plugins::creator_plugins_studio_save_note,
            modules::creator_plugins::creator_plugins_studio_delete_note,
            modules::game_sessions::game_sessions_snapshot,
            modules::game_sessions::game_sessions_set_enabled,
            modules::game_sessions::game_sessions_add,
            modules::game_sessions::game_sessions_remove,
            modules::game_sessions::game_sessions_clear,
            modules::downloads_organizer::downloads_organizer_snapshot,
            modules::downloads_organizer::downloads_organizer_set_root,
            modules::downloads_organizer::downloads_organizer_set_enabled,
            modules::downloads_organizer::downloads_organizer_save_rule,
            modules::downloads_organizer::downloads_organizer_delete_rule,
            modules::downloads_organizer::downloads_organizer_preview,
            modules::downloads_organizer::downloads_organizer_run,
            modules::downloads_organizer::downloads_organizer_undo,
            modules::maintenance::maintenance_backup_create,
            modules::maintenance::maintenance_backup_preview,
            modules::maintenance::maintenance_backup_stage_restore,
            modules::maintenance::maintenance_take_restored_client_data,
            modules::maintenance::maintenance_pending_restore_status,
            modules::maintenance::maintenance_acknowledge_restored_client_data,
            modules::maintenance::maintenance_diagnostics,
            modules::maintenance::maintenance_diagnostics_export,
            modules::maintenance::maintenance_check_updates,
            modules::streamdeck_bridge::creator_streamdeck_pairing_start,
            modules::streamdeck_bridge::creator_streamdeck_pairing_status,
            modules::streamdeck_bridge::creator_streamdeck_revoke,
            modules::streamdeck_bridge::creator_streamdeck_install
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ORBIT OS");
}
