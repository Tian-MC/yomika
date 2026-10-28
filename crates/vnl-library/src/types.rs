//! Library-specific API types. Mirrored in `src/contract/library.ts`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vnl_core::{GameId, GameSettings};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExeCandidate {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    /// Higher = more likely the main game executable.
    pub score: i32,
    /// Short machine-friendly hints, e.g. `["largest", "matchesFolderName"]`.
    pub hints: Vec<String>,
    pub is_installer: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportScan {
    pub root: PathBuf,
    /// Sorted by score, best first.
    pub candidates: Vec<ExeCandidate>,
    pub suggested_title: String,
    /// The scanned path itself looks like an installer (setup.exe, *.msi...).
    pub is_installer: bool,
    /// Set when a game with one of the candidate exes is already in the library.
    pub existing_game_id: Option<GameId>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportRequest {
    pub title: String,
    pub exe_path: PathBuf,
    pub install_dir: Option<PathBuf>,
    pub working_dir: Option<PathBuf>,
    pub launch_args: Vec<String>,
    pub vndb_id: Option<String>,
    /// Defaults derived from `AppSettings` when omitted.
    pub settings: Option<GameSettings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaySession {
    pub id: i64,
    pub game_id: GameId,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VndbSearchResult {
    pub id: String,
    pub title: String,
    pub alt_title: Option<String>,
    pub released: Option<String>,
    pub developers: Vec<String>,
    pub image_url: Option<String>,
    pub image_sexual: Option<f64>,
    pub rating: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveLocationSource {
    Detected,
    Manual,
}

impl SaveLocationSource {
    pub(crate) fn parse(s: &str) -> Self {
        if s == "manual" { Self::Manual } else { Self::Detected }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLocation {
    pub path: PathBuf,
    pub source: SaveLocationSource,
    pub exists: bool,
    /// Why this location was suggested, e.g. "AppData/Roaming match".
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveBackupKind {
    /// Requested by the user.
    Manual,
    /// Taken automatically right before a restore.
    Safety,
}

impl SaveBackupKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Safety => "safety",
        }
    }

    pub(crate) fn parse(s: &str) -> Self {
        if s == "safety" { Self::Safety } else { Self::Manual }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBackup {
    pub id: i64,
    pub game_id: GameId,
    pub created_at: i64,
    pub label: Option<String>,
    pub kind: SaveBackupKind,
    pub size_bytes: u64,
    pub file_count: u64,
    pub archive_path: PathBuf,
    pub source_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub restored: SaveBackup,
    /// Backup of the current saves taken automatically before restoring.
    /// `None` when none of the source paths currently contain files.
    pub safety_backup: Option<SaveBackup>,
    /// Number of files written back.
    pub files_restored: u64,
}
