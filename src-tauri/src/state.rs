use std::sync::Arc;

use tauri::AppHandle;
use vnl_core::{AppError, AppPaths, EventSink, LogEntry, LogLevel};
use vnl_graphics::GraphicsService;
use vnl_library::Library;
use vnl_runtime::RuntimeService;
use vnl_translate::TranslationService;

use crate::sink::TauriSink;

/// Shared application state, managed by Tauri. Commands receive it as
/// `tauri::State<'_, AppState>`.
pub struct AppState {
    pub app: AppHandle,
    pub paths: AppPaths,
    pub sink: Arc<TauriSink>,
    pub library: Arc<Library>,
    pub runtime: Arc<RuntimeService>,
    pub graphics: Arc<GraphicsService>,
    pub translation: Arc<TranslationService>,
}

impl AppState {
    pub fn init(app: AppHandle) -> Result<Self, AppError> {
        let paths = AppPaths::from_env()?;
        paths.ensure()?;
        tracing::info!(data = %paths.data_dir.display(), "starting VN Launcher");

        let sink = Arc::new(TauriSink::new(app.clone()));
        let events: Arc<dyn EventSink> = sink.clone();

        let library = Arc::new(Library::open(&paths)?);
        let runtime = Arc::new(RuntimeService::new(&paths, events.clone())?);
        let graphics = Arc::new(GraphicsService::new(&paths));
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

        Ok(Self { app, paths, sink, library, runtime, graphics, translation })
    }
}
