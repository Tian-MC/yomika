use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::game::GameId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunnerKind {
    /// A plain Wine build (`wine` binary).
    Wine,
    /// A Proton build (directory containing the `proton` script), e.g. GE-Proton.
    Proton,
    /// Run a Linux-native executable directly (no Wine).
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunnerSource {
    /// Found in `$PATH` (e.g. `/usr/bin/wine`).
    System,
    /// Steam `compatibilitytools.d` or Steam's own Proton installs.
    Steam,
    /// Downloaded/managed by VN Launcher.
    Managed,
    /// Added manually by the user.
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Runner {
    /// Stable id, e.g. `"wine:system"`, `"proton:GE-Proton11-11"`, `"native"`.
    pub id: String,
    pub name: String,
    pub kind: RunnerKind,
    pub source: RunnerSource,
    /// `wine` binary for Wine runners, Proton root directory for Proton runners.
    pub path: PathBuf,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrefixArch {
    #[default]
    Win64,
    Win32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PrefixKind {
    /// Created for exactly one game.
    #[serde(rename_all = "camelCase")]
    Dedicated { game_id: GameId },
    /// Named prefix that several games can share.
    Shared,
    /// Prefix that already existed and was registered by the user.
    /// VN Launcher must never delete or reset it.
    External,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixInfo {
    pub id: String,
    pub name: String,
    /// For Wine: the WINEPREFIX. For Proton: the compat data dir (contains `pfx/`).
    pub path: PathBuf,
    pub arch: PrefixArch,
    pub kind: PrefixKind,
    /// Runner last used to initialise this prefix.
    pub runner_id: Option<String>,
    pub created_at: i64,
    /// True when VN Launcher created the directory and may manage it.
    pub managed: bool,
    /// Whether the prefix has been initialised (drive_c exists).
    pub initialized: bool,
}

pub type SessionId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Starting,
    Running,
    Stopping,
    Exited,
    Failed,
}

impl SessionState {
    pub fn is_finished(self) -> bool {
        matches!(self, SessionState::Exited | SessionState::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxProcessInfo {
    pub aux_id: String,
    pub name: String,
    pub pid: Option<u32>,
    pub running: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: SessionId,
    pub game_id: GameId,
    pub state: SessionState,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// PID of the top-level launched process (may be gamescope/umu/proton).
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub runner_id: Option<String>,
    pub prefix_path: Option<PathBuf>,
    pub aux: Vec<AuxProcessInfo>,
    /// Human-readable command line for diagnostics (never executed via a shell).
    pub command_preview: String,
}
