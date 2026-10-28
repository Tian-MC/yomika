//! Library subsystem: SQLite schema, games, settings, playtime, VNDB, saves.
//! Owned by Agent 2.
//!
//! CONTRACT (used by src-tauri/src/commands/session.rs — keep signatures stable,
//! you may add methods freely). `Library` must be `Send + Sync`.
//!
//! Module map:
//! - [`db`]: connection setup + versioned migrations (`PRAGMA user_version`)
//! - `games`: games CRUD/query, per-game + app settings, playtime
//! - [`scan`]: import scanning (exe candidates, installer detection, title)
//! - [`vndb`]: VNDB Kana API client + metadata mapping + cover download
//! - [`saves`]: save location detection, backups and restore

use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;
use vnl_core::{AppError, AppPaths};

pub mod db;
mod games;
pub mod saves;
pub mod scan;
pub mod text;
pub mod types;
pub mod vndb;

pub use games::default_game_settings;
pub use saves::{SaveContext, SaveLocator};
pub use types::*;
pub use vndb::VndbClient;

pub struct Library {
    paths: AppPaths,
    conn: Mutex<Connection>,
    vndb: VndbClient,
}

impl Library {
    /// Open (creating + migrating) the database at `paths.db_path()`.
    pub fn open(paths: &AppPaths) -> Result<Self, AppError> {
        let conn = db::open_file(&paths.db_path())?;
        Ok(Self::from_connection(paths, conn))
    }

    /// In-memory database (tests). Files (covers, backups) still go to `paths`.
    pub fn open_in_memory(paths: &AppPaths) -> Result<Self, AppError> {
        Ok(Self::from_connection(paths, db::open_in_memory()?))
    }

    fn from_connection(paths: &AppPaths, conn: Connection) -> Self {
        Self { paths: paths.clone(), conn: Mutex::new(conn), vndb: VndbClient::from_env() }
    }

    /// Replace the VNDB client (e.g. to point at a mock server in tests).
    pub fn with_vndb_client(mut self, client: VndbClient) -> Self {
        self.vndb = client;
        self
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn vndb_client(&self) -> &VndbClient {
        &self.vndb
    }

    /// Inspect an `.exe`/`.msi` or a folder before importing (read-only).
    /// Blocking filesystem I/O: call from `spawn_blocking` in async contexts.
    pub fn scan_path(&self, path: &std::path::Path) -> Result<ImportScan, AppError> {
        scan::scan(path, |p| self.find_game_by_exe(p).ok().flatten())
    }

    /// The connection lock. A panic while holding it cannot leave SQLite in a
    /// half-written state (transactions roll back on drop), so poisoning is
    /// recovered from instead of propagating.
    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests;
