//! Tauri shell and cross-subsystem integration layer. Owned by the orchestrator.
//!
//! Subsystem agents may only edit:
//! - their own `commands/<area>.rs`
//! - the lines inside their area's block in `generate_handler!` below.

mod commands;
mod sink;
mod state;

use tauri::Manager;

pub use state::AppState;

pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::init(app.handle().clone())?;
            // Covers live in the cache dir, which can be relocated with VNL_HOME.
            let _ = app.asset_protocol_scope().allow_directory(state.paths.covers_dir(), true);
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // --- session (orchestrator)
            commands::session::session_launch,
            commands::session::session_stop,
            commands::session::session_list,
            commands::session::session_start_aux,
            commands::session::session_preview,
            commands::session::logs_recent,
            commands::session::app_info,
            // --- library (agent 2)
            commands::library::library_list_games,
            commands::library::library_get_game,
            commands::library::library_scan_path,
            commands::library::library_import_game,
            commands::library::library_update_game,
            commands::library::library_remove_game,
            commands::library::library_get_settings,
            commands::library::library_save_settings,
            commands::library::library_play_sessions,
            commands::library::settings_get_app,
            commands::library::settings_save_app,
            commands::library::vndb_search,
            commands::library::vndb_apply,
            commands::library::saves_detect,
            commands::library::saves_set_locations,
            commands::library::saves_list_backups,
            commands::library::saves_create_backup,
            commands::library::saves_restore_backup,
            // --- runtime (agent 1)
            commands::runtime::runtime_list_runners,
            commands::runtime::runtime_refresh_runners,
            commands::runtime::runtime_add_custom_runner,
            commands::runtime::runtime_list_prefixes,
            commands::runtime::runtime_create_prefix,
            commands::runtime::runtime_register_prefix,
            commands::runtime::runtime_prefix_tool,
            commands::runtime::runtime_run_installer,
            // --- graphics + diagnostics (agent 5)
            commands::graphics::graphics_capabilities,
            commands::graphics::graphics_preview,
            commands::graphics::graphics_upscaler_info,
            commands::graphics::diagnostics_dependencies,
            commands::graphics::diagnostics_system,
            commands::graphics::diagnostics_prefix_components,
            // --- translation (agent 4)
            commands::translation::translation_providers,
            commands::translation::translation_configure_provider,
            commands::translation::translation_test_provider,
            commands::translation::translation_translate,
            commands::translation::translation_languages,
            commands::translation::translation_glossary_list,
            commands::translation::translation_glossary_upsert,
            commands::translation::translation_glossary_delete,
            commands::translation::translation_history,
            commands::translation::translation_history_clear,
            commands::translation::translation_ocr_backends,
            commands::translation::translation_ocr_capture,
            commands::translation::translation_hook_backends,
            commands::translation::translation_session_status,
            commands::translation::overlay_show,
            commands::translation::overlay_hide,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VN Launcher");
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("VNL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
