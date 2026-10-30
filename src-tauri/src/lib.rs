//! Tauri shell and cross-subsystem integration layer. Owned by the orchestrator.
//!
//! Subsystem agents may only edit:
//! - their own `commands/<area>.rs`
//! - the lines inside their area's block in `generate_handler!` below.

mod commands;
mod launch;
mod sink;
mod state;

use tauri::Manager;

pub use state::AppState;

pub fn run() {
    init_tracing();
    apply_webkit_workarounds();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::init(app.handle().clone())?;
            // Covers live in the cache dir, which can be relocated with VNL_HOME.
            let _ = app.asset_protocol_scope().allow_directory(state.paths.covers_dir(), true);
            app.manage(state);
            navigate_to_initial_route(app.handle());
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
            // --- gamescope management (orchestrator)
            commands::gamescope::gamescope_status,
            commands::gamescope::gamescope_inspect_path,
            commands::gamescope::gamescope_build_start,
            commands::gamescope::gamescope_build_cancel,
            commands::gamescope::gamescope_managed_remove,
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
            commands::translation::translation_hook_select_thread,
            commands::translation::translation_hook_select_threads,
            commands::translation::translation_hook_insert_code,
            commands::translation::translation_hook_remove_code,
            commands::translation::translation_hook_search,
            commands::translation::overlay_show,
            commands::translation::overlay_hide,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Yomika")
        .run(|app, event| match event {
            tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { api, .. }, .. }
                if label == "main" =>
            {
                // Closing the main window must also exit when an overlay exists.
                api.prevent_close();
                AppState::begin_shutdown(app);
            }
            tauri::RunEvent::ExitRequested { api, .. }
                if !app.state::<AppState>().shutdown_complete.load(std::sync::atomic::Ordering::SeqCst) =>
            {
                api.prevent_exit();
                AppState::begin_shutdown(app);
            }
            _ => {}
        });
}

/// WebKitGTK's DMABUF renderer shows a blank window on many NVIDIA setups
/// ("Failed to create GBM buffer"). Disable it there unless the user chose
/// explicitly. Must run before GTK/WebKit initialise (single-threaded here).
fn apply_webkit_workarounds() {
    const VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
    if std::env::var_os(VAR).is_none() && std::path::Path::new("/proc/driver/nvidia/version").exists() {
        tracing::info!("NVIDIA driver detected: setting {VAR}=1");
        // SAFETY: called at the start of `run`, before any other thread exists.
        unsafe { std::env::set_var(VAR, "1") };
    }
}

/// Debug/screenshot aid: `VNL_INITIAL_ROUTE=/diagnostics` opens that page.
fn navigate_to_initial_route(app: &tauri::AppHandle) {
    let Some(route) = std::env::var("VNL_INITIAL_ROUTE").ok().filter(|r| r.starts_with('/')) else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if let Some(window) = app.get_webview_window("main") {
            let hash = serde_json::to_string(&format!("#{route}")).unwrap_or_default();
            let _ = window.eval(format!("window.location.hash = {hash};"));
        }
    });
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("VNL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
