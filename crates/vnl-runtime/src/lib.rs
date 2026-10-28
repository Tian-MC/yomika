//! Runtime subsystem: runner discovery, prefixes, command building and
//! process sessions. Owned by Agent 1.
//!
//! CONTRACT (used by src-tauri/src/commands/session.rs — keep signatures stable,
//! you may add methods/fields freely):
//! - `RuntimeService::new`
//! - `list_runners`, `resolve_runner`
//! - `list_prefixes`, `resolve_prefix`
//! - `build_game_command`, `build_prefix_command`
//! - `start_session`, `stop_session`, `start_aux`, `sessions`, `session`

use std::path::Path;
use std::sync::Arc;

use vnl_core::{
    AppError, AppPaths, AuxProgram, CommandSpec, EventSink, Game, GameId, PrefixChoice, PrefixInfo, Runner, RunnerRef,
    RuntimeConfig, SessionInfo,
};

/// Everything needed to start a game session. Built by the integration layer.
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub game_id: GameId,
    pub runner: Runner,
    pub prefix: PrefixInfo,
    pub config: RuntimeConfig,
    /// Final game command (possibly already wrapped by gamescope).
    pub command: CommandSpec,
    /// Native helper processes started after the game (e.g. upscaler).
    /// Stopped when the session ends.
    pub companions: Vec<CommandSpec>,
    /// Auxiliary programs with their pre-built commands.
    pub aux: Vec<(AuxProgram, CommandSpec)>,
}

pub struct RuntimeService {
    #[allow(dead_code)]
    paths: AppPaths,
    #[allow(dead_code)]
    events: Arc<dyn EventSink>,
}

impl RuntimeService {
    pub fn new(paths: &AppPaths, events: Arc<dyn EventSink>) -> Result<Self, AppError> {
        Ok(Self { paths: paths.clone(), events })
    }

    pub fn list_runners(&self) -> Vec<Runner> {
        Vec::new()
    }

    /// Pick `preferred`, else `fallback`, else the best available runner.
    pub fn resolve_runner(&self, preferred: Option<&RunnerRef>, fallback: Option<&RunnerRef>) -> Result<Runner, AppError> {
        let _ = (preferred, fallback);
        Err(AppError::not_implemented("runner resolution"))
    }

    pub fn list_prefixes(&self) -> Result<Vec<PrefixInfo>, AppError> {
        Ok(Vec::new())
    }

    /// Resolve (and register, for dedicated prefixes) the prefix for a game.
    /// Must not create/initialise anything destructive; Wine/Proton initialise
    /// an empty prefix on first run.
    pub fn resolve_prefix(&self, game: &Game, choice: &PrefixChoice, runner: &Runner) -> Result<PrefixInfo, AppError> {
        let _ = (game, choice, runner);
        Err(AppError::not_implemented("prefix resolution"))
    }

    pub fn build_game_command(
        &self,
        game: &Game,
        config: &RuntimeConfig,
        runner: &Runner,
        prefix: &PrefixInfo,
    ) -> Result<CommandSpec, AppError> {
        let _ = (game, config, runner, prefix);
        Err(AppError::not_implemented("game command"))
    }

    /// Command running another Windows exe in the same prefix/runner as the
    /// game, safe to start while the game is running (Proton: runinprefix).
    pub fn build_prefix_command(
        &self,
        exe: &Path,
        args: &[String],
        cwd: Option<&Path>,
        config: &RuntimeConfig,
        runner: &Runner,
        prefix: &PrefixInfo,
    ) -> Result<CommandSpec, AppError> {
        let _ = (exe, args, cwd, config, runner, prefix);
        Err(AppError::not_implemented("prefix command"))
    }

    pub async fn start_session(&self, request: LaunchRequest) -> Result<SessionInfo, AppError> {
        let _ = request;
        Err(AppError::not_implemented("session start"))
    }

    pub async fn stop_session(&self, session_id: &str, force: bool) -> Result<(), AppError> {
        let _ = (session_id, force);
        Err(AppError::not_implemented("session stop"))
    }

    pub async fn start_aux(&self, session_id: &str, aux_id: &str) -> Result<SessionInfo, AppError> {
        let _ = (session_id, aux_id);
        Err(AppError::not_implemented("aux start"))
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        Vec::new()
    }

    pub fn session(&self, session_id: &str) -> Option<SessionInfo> {
        let _ = session_id;
        None
    }
}
