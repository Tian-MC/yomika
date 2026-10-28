//! Library subsystem: SQLite schema, games, settings, playtime, VNDB, saves.
//! Owned by Agent 2.
//!
//! CONTRACT (used by src-tauri/src/commands/session.rs — keep signatures stable,
//! you may add methods freely). `Library` must be `Send + Sync`.

use vnl_core::{AppError, AppPaths, AppSettings, Game, GameId, GameSettings};

pub struct Library {
    #[allow(dead_code)]
    paths: AppPaths,
}

impl Library {
    /// Open (creating + migrating) the database at `paths.db_path()`.
    pub fn open(paths: &AppPaths) -> Result<Self, AppError> {
        Ok(Self { paths: paths.clone() })
    }

    pub fn get_game(&self, id: GameId) -> Result<Game, AppError> {
        Err(AppError::not_found(format!("game {id}")))
    }

    pub fn get_settings(&self, game_id: GameId) -> Result<GameSettings, AppError> {
        let _ = game_id;
        Ok(GameSettings::default())
    }

    pub fn get_app_settings(&self) -> Result<AppSettings, AppError> {
        Ok(AppSettings::default())
    }

    /// Persist a finished play session and update the game's playtime and
    /// `last_played_at`.
    pub fn record_play_session(&self, game_id: GameId, started_at: i64, ended_at: i64) -> Result<(), AppError> {
        let _ = (game_id, started_at, ended_at);
        Err(AppError::not_implemented("playtime recording"))
    }
}
