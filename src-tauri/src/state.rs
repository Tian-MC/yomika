use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;
use vnl_core::{AppError, AppPaths, EventSink, LogEntry, LogLevel};
use vnl_graphics::GraphicsService;
use vnl_graphics::gamescope_build::GamescopeBuilder;
use vnl_library::Library;
use vnl_runtime::RuntimeService;
use vnl_translate::TranslationService;

use crate::sink::TauriSink;

/// Shared application state, managed by Tauri. Commands receive it as
/// `tauri::State<'_, AppState>`.
pub struct AppState {
    pub app: AppHandle,
    pub shutting_down: AtomicBool,
    pub shutdown_complete: AtomicBool,
    pub paths: AppPaths,
    pub sink: Arc<TauriSink>,
    pub library: Arc<Library>,
    pub runtime: Arc<RuntimeService>,
    pub graphics: Arc<GraphicsService>,
    pub gamescope_builder: Arc<GamescopeBuilder>,
    pub translation: Arc<TranslationService>,
}

impl AppState {
    /// Called once from the main-window close or application exit handler.
    pub fn begin_shutdown(app: &AppHandle) {
        use tauri::Manager;
        let state = app.state::<Self>();
        if state.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }
        state.gamescope_builder.cancel();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<Self>();
            // Stop extraction first: hook hosts sharing the game's Wine prefix
            // must not keep wineserver/proton alive while game cleanup waits.
            state.translation.shutdown().await;
            state.runtime.shutdown().await;
            // Give the existing build controller time to escalate cancellation
            // and reap its command before the event loop/runtime goes away.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while state.gamescope_builder.status().running && std::time::Instant::now() < deadline {
                state.gamescope_builder.cancel();
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            state.shutdown_complete.store(true, Ordering::SeqCst);
            app.exit(0);
        });
    }

    pub fn init(app: AppHandle) -> Result<Self, AppError> {
        let paths = AppPaths::from_env()?;
        paths.ensure()?;
        tracing::info!(data = %paths.data_dir.display(), "starting Yomika");

        let sink = Arc::new(TauriSink::new(app.clone()));
        let events: Arc<dyn EventSink> = sink.clone();

        let library = Arc::new(Library::open(&paths)?);
        let runtime = Arc::new(RuntimeService::new(&paths, events.clone())?);
        let graphics = Arc::new(GraphicsService::new(&paths));
        let gamescope_builder = Arc::new(GamescopeBuilder::new(&paths, events.clone()));
        let translation = Arc::new(TranslationService::new(&paths, events.clone())?);

        {
            let library = library.clone();
            let translation = translation.clone();
            let sink_for_log = sink.clone();
            sink.set_session_end_handler(Box::new(move |info| {
                if let Err(e) = crate::launch::record_session_end(&library, info) {
                    sink_for_log.log(
                        LogEntry::new(LogLevel::Error, "library", format!("failed to record playtime: {e}"))
                            .session(info.id.clone()),
                    );
                }
                let translation = translation.clone();
                let session_id = info.id.clone();
                tauri::async_runtime::spawn(async move { translation.stop_session(&session_id).await });
            }));
        }

        Ok(Self {
            app,
            paths,
            sink,
            library,
            runtime,
            graphics,
            gamescope_builder,
            translation,
            shutting_down: AtomicBool::new(false),
            shutdown_complete: AtomicBool::new(false),
        })
    }
}
