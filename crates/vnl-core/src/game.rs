use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub type GameId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayStatus {
    #[default]
    Unplayed,
    Playing,
    Completed,
    OnHold,
    Dropped,
    Planned,
}

/// Metadata usually filled from VNDB, always editable by the user.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameMetadata {
    /// VNDB id such as `"v17"`.
    pub vndb_id: Option<String>,
    pub description: Option<String>,
    pub developers: Vec<String>,
    /// ISO date (`YYYY-MM-DD`, `YYYY-MM` or `YYYY`).
    pub release_date: Option<String>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    /// VNDB rating on a 10–100 scale.
    pub rating: Option<f64>,
    pub length_minutes: Option<i64>,
    /// Locally cached cover image (absolute path).
    pub cover_path: Option<PathBuf>,
    pub cover_url: Option<String>,
    /// VNDB image sexual rating (0–2). Used to blur covers when configured.
    pub cover_sexual: Option<f64>,
    /// Unix seconds of the last successful metadata refresh.
    pub fetched_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: GameId,
    pub title: String,
    pub original_title: Option<String>,
    /// Absolute path to the Windows (or native) executable.
    pub exe_path: PathBuf,
    /// Root folder of the installed game.
    pub install_dir: PathBuf,
    /// Working directory override; defaults to the executable's folder.
    pub working_dir: Option<PathBuf>,
    pub launch_args: Vec<String>,
    pub status: PlayStatus,
    pub favorite: bool,
    pub playtime_seconds: i64,
    pub last_played_at: Option<i64>,
    pub added_at: i64,
    pub metadata: GameMetadata,
    pub notes: Option<String>,
}

impl Game {
    /// Effective working directory: explicit override or the executable's parent.
    pub fn effective_working_dir(&self) -> PathBuf {
        self.working_dir
            .clone()
            .or_else(|| self.exe_path.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| self.install_dir.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GameSort {
    #[default]
    Title,
    LastPlayed,
    Playtime,
    AddedAt,
    ReleaseDate,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameQuery {
    /// Case-insensitive match on title, original title, aliases and developers.
    pub search: Option<String>,
    pub status: Option<PlayStatus>,
    pub favorite_only: bool,
    pub tag: Option<String>,
    pub sort: GameSort,
    pub descending: bool,
}
