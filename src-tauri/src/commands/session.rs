//! Cross-subsystem session orchestration: library -> runtime -> graphics ->
//! runtime spawn -> (optional) translation. Owned by the orchestrator.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use vnl_core::{
    AppError, AuxProgram, CommandSpec, EventSink, ExtractionMode, Game, GameId, LogEntry, LogLevel, PrefixCommandBuilder,
    PrefixInfo, Runner, RuntimeConfig, SessionInfo,
};
use vnl_runtime::{LaunchRequest, RuntimeService};

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

struct ResolvedLaunch {
    game: Game,
    settings: vnl_core::GameSettings,
    app_settings: vnl_core::AppSettings,
    runner: Runner,
    prefix: PrefixInfo,
    command: CommandSpec,
    companions: Vec<CommandSpec>,
    aux: Vec<(AuxProgram, CommandSpec)>,
    warnings: Vec<String>,
}

fn resolve_launch(state: &AppState, game_id: GameId) -> Result<ResolvedLaunch, AppError> {
    let game = state.library.get_game(game_id)?;
    let settings = state.library.get_settings(game_id)?;
    let app_settings = state.library.get_app_settings()?;

    let runner = state.runtime.resolve_runner(settings.runtime.runner.as_ref(), app_settings.default_runner.as_ref())?;
    let prefix = state.runtime.resolve_prefix(&game, &settings.runtime.prefix, &runner)?;
    let base = state.runtime.build_game_command(&game, &settings.runtime, &runner, &prefix)?;
    let plan = state.graphics.apply(&settings.graphics, base, &runner)?;

    let mut aux = Vec::with_capacity(settings.runtime.auxiliary.len());
    for program in &settings.runtime.auxiliary {
        let spec = build_aux_command(&state.runtime, program, &settings.runtime, &runner, &prefix)?;
        aux.push((program.clone(), spec));
    }

    Ok(ResolvedLaunch {
        game,
        settings,
        app_settings,
        runner,
        prefix,
        command: plan.command,
        companions: plan.companions,
        aux,
        warnings: plan.warnings,
    })
}

fn build_aux_command(
    runtime: &RuntimeService,
    program: &AuxProgram,
    config: &RuntimeConfig,
    runner: &Runner,
    prefix: &PrefixInfo,
) -> Result<CommandSpec, AppError> {
    let mut spec = if program.windows {
        runtime.build_prefix_command(
            &program.exe_path,
            &program.args,
            program.working_dir.as_deref(),
            config,
            runner,
            prefix,
        )?
    } else {
        let mut spec = CommandSpec::new(&program.exe_path).args(program.args.clone());
        spec.cwd = program.working_dir.clone().or_else(|| program.exe_path.parent().map(Path::to_path_buf));
        spec
    };
    spec.env.extend(program.env.clone());
    Ok(spec)
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
    if let Some(running) =
        state.runtime.sessions().into_iter().find(|s| s.game_id == game_id && !s.state.is_finished())
    {
        return Err(AppError::invalid("This game is already running").with_details(format!("session {}", running.id)));
    }

    let resolved = resolve_launch(&state, game_id)?;
    for warning in &resolved.warnings {
        state.sink.log(LogEntry::new(LogLevel::Warn, "graphics", warning.clone()));
    }

    let request = LaunchRequest {
        game_id,
        runner: resolved.runner.clone(),
        prefix: resolved.prefix.clone(),
        config: resolved.settings.runtime.clone(),
        command: resolved.command,
        companions: resolved.companions,
        aux: resolved.aux,
    };
    let session = state.runtime.start_session(request).await?;

    let translation =
        vnl_translate::resolve_config(&resolved.settings.translation, &resolved.app_settings.translation);
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
    let resolved = resolve_launch(&state, game_id)?;
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
