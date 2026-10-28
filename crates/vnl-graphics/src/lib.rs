//! Graphics subsystem: gamescope wrapping, Wine FSR env, upscaler adapter,
//! display/dependency detection. Owned by Agent 5.
//!
//! CONTRACT (used by src-tauri/src/commands/session.rs — keep signatures stable,
//! you may add methods/fields freely).

use vnl_core::{AppError, AppPaths, CommandSpec, GraphicsConfig, Runner};

/// Result of applying a graphics config to a game command.
#[derive(Debug, Clone, Default)]
pub struct GraphicsPlan {
    /// Game command, wrapped by gamescope when enabled, with extra env.
    pub command: CommandSpec,
    /// Native helper processes to start alongside the game (e.g. upscaler).
    pub companions: Vec<CommandSpec>,
    pub warnings: Vec<String>,
}

pub struct GraphicsService {
    #[allow(dead_code)]
    paths: AppPaths,
}

impl GraphicsService {
    pub fn new(paths: &AppPaths) -> Self {
        Self { paths: paths.clone() }
    }

    /// Apply `config` to the fully-built runtime command. When every graphics
    /// feature is disabled this must return the command unchanged.
    pub fn apply(&self, config: &GraphicsConfig, command: CommandSpec, runner: &Runner) -> Result<GraphicsPlan, AppError> {
        let _ = (config, runner);
        Ok(GraphicsPlan { command, ..Default::default() })
    }
}
