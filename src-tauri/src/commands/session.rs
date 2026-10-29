//! Cross-subsystem session orchestration: library -> runtime -> graphics ->
//! runtime spawn -> (optional) translation. Owned by the orchestrator.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::State;
use vnl_core::{
    AppError, CommandSpec, EventSink, ExtractionMode, GameId, LogEntry, LogLevel, PrefixCommandBuilder, PrefixInfo,
    Runner, RuntimeConfig, SessionInfo,
};
use vnl_runtime::RuntimeService;

use crate::launch::{ResolvedLaunch, resolve_launch};
use crate::sink::TauriSink;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPreview {
    pub runner: Runner,
    pub prefix: PrefixInfo,
    pub command: CommandSpec,
    pub preview: String,
    pub companions: Vec<CommandSpec>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub db_path: PathBuf,
}

fn resolve(state: &AppState, game_id: GameId) -> Result<ResolvedLaunch, AppError> {
    resolve_launch(&state.library, &state.runtime, &state.graphics, game_id)
}

/// Lets the translation subsystem run Windows helpers (hook hosts) inside the
/// game's prefix without depending on `vnl-runtime`.
struct SessionPrefixBuilder {
    runtime: Arc<RuntimeService>,
    config: RuntimeConfig,
    runner: Runner,
    prefix: PrefixInfo,
}

impl PrefixCommandBuilder for SessionPrefixBuilder {
    fn build(&self, exe: &Path, args: &[String], cwd: Option<&Path>) -> Result<CommandSpec, AppError> {
        self.runtime.build_prefix_command(exe, args, cwd, &self.config, &self.runner, &self.prefix)
    }

    fn prefix_path(&self) -> PathBuf {
        self.prefix.path.clone()
    }
}

#[tauri::command]
pub async fn session_launch(state: State<'_, AppState>, game_id: GameId) -> Result<SessionInfo, AppError> {
    if let Some(running) = state.runtime.sessions().into_iter().find(|s| s.game_id == game_id && !s.state.is_finished())
    {
        return Err(AppError::invalid("This game is already running").with_details(format!("session {}", running.id)));
    }

    let resolved = resolve(&state, game_id)?;
    for warning in &resolved.warnings {
        state.sink.log(LogEntry::new(LogLevel::Warn, "graphics", warning.clone()));
    }

    let session = state.runtime.start_session(resolved.to_request()).await?;

    let gamescope = &resolved.settings.graphics.gamescope;
    if gamescope.enabled && gamescope.auto_fullscreen {
        spawn_fullscreen_assist(state.runtime.clone(), state.sink.clone(), session.id.clone());
    }

    let translation = vnl_translate::resolve_config(&resolved.settings.translation, &resolved.app_settings.translation);
    if translation.enabled && translation.extraction != ExtractionMode::Disabled {
        let ctx = vnl_translate::SessionContext {
            session_id: session.id.clone(),
            game_id,
            config: translation,
            game_exe: resolved.game.exe_path.clone(),
            prefix: Arc::new(SessionPrefixBuilder {
                runtime: state.runtime.clone(),
                config: resolved.settings.runtime.clone(),
                runner: resolved.runner,
                prefix: resolved.prefix,
            }),
        };
        let service = state.translation.clone();
        let sink = state.sink.clone();
        let session_id = session.id.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = service.start_session(ctx).await {
                // Text extraction is optional: never abort the game because of it.
                sink.log(
                    LogEntry::new(LogLevel::Warn, "translation", format!("text extraction unavailable: {e}"))
                        .session(session_id),
                );
            }
        });
    }

    Ok(session)
}

/// Waits for gamescope's nested Xwayland display (from the session log) and
/// switches a windowed game to fullscreen. See `vnl_graphics::fullscreen`.
fn spawn_fullscreen_assist(runtime: Arc<RuntimeService>, sink: Arc<TauriSink>, session_id: String) {
    use vnl_graphics::fullscreen::{FullscreenOutcome, ensure_game_fullscreen, parse_xwayland_display};

    tauri::async_runtime::spawn_blocking(move || {
        let log =
            |level, message: String| sink.log(LogEntry::new(level, "graphics", message).session(session_id.clone()));
        let started = Instant::now();
        let display = loop {
            if runtime.session(&session_id).is_none_or(|s| s.state.is_finished()) {
                return;
            }
            let found =
                sink.recent_logs(2000, Some(&session_id)).iter().rev().find_map(|l| parse_xwayland_display(&l.message));
            if let Some(display) = found {
                break display;
            }
            if started.elapsed() > Duration::from_secs(30) {
                log(LogLevel::Warn, "fullscreen assist: gamescope's Xwayland display not found in its log".into());
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        };
        // Generous timeout: the first launch of a new prefix can take a while.
        match ensure_game_fullscreen(&display, Duration::from_secs(180)) {
            Ok(FullscreenOutcome::Switched) => {
                log(LogLevel::Info, format!("fullscreen assist: switched the game to fullscreen (display {display})"))
            }
            Ok(FullscreenOutcome::AlreadyFullscreen) => {
                log(LogLevel::Info, "fullscreen assist: game already fullscreen, nothing sent".into())
            }
            Ok(FullscreenOutcome::Unchanged(g)) => log(
                LogLevel::Warn,
                format!(
                    "fullscreen assist: sent Alt+Enter but the window stayed {}x{}; enable fullscreen in the game's own settings or disable this option",
                    g.width, g.height
                ),
            ),
            Ok(FullscreenOutcome::NoWindow) => log(LogLevel::Warn, "fullscreen assist: no game window appeared".into()),
            Err(e) => log(LogLevel::Warn, format!("fullscreen assist failed: {e}")),
        }
    });
}

#[tauri::command]
pub async fn session_stop(state: State<'_, AppState>, session_id: String, force: bool) -> Result<(), AppError> {
    state.runtime.stop_session(&session_id, force).await
}

#[tauri::command]
pub async fn session_list(state: State<'_, AppState>) -> Result<Vec<SessionInfo>, AppError> {
    Ok(state.runtime.sessions())
}

#[tauri::command]
pub async fn session_start_aux(
    state: State<'_, AppState>,
    session_id: String,
    aux_id: String,
) -> Result<SessionInfo, AppError> {
    state.runtime.start_aux(&session_id, &aux_id).await
}

#[tauri::command]
pub async fn session_preview(state: State<'_, AppState>, game_id: GameId) -> Result<LaunchPreview, AppError> {
    let resolved = resolve(&state, game_id)?;
    Ok(LaunchPreview {
        preview: resolved.command.preview(),
        runner: resolved.runner,
        prefix: resolved.prefix,
        command: resolved.command,
        companions: resolved.companions,
        warnings: resolved.warnings,
    })
}

#[tauri::command]
pub async fn logs_recent(
    state: State<'_, AppState>,
    limit: Option<usize>,
    session_id: Option<String>,
) -> Result<Vec<LogEntry>, AppError> {
    Ok(state.sink.recent_logs(limit.unwrap_or(500), session_id.as_deref()))
}

#[tauri::command]
pub async fn app_info(state: State<'_, AppState>) -> Result<AppInfo, AppError> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: state.paths.data_dir.clone(),
        config_dir: state.paths.config_dir.clone(),
        cache_dir: state.paths.cache_dir.clone(),
        db_path: state.paths.db_path(),
    })
}
