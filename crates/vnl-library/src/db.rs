//! SQLite connection setup and versioned migrations (`PRAGMA user_version`).

use std::path::Path;

use rusqlite::Connection;
use vnl_core::{AppError, ErrorCode};

/// Migration `i` upgrades the schema from version `i` to `i + 1`.
/// Never edit a released migration; append a new one instead.
const MIGRATIONS: &[&str] = &[
    // v1: games, per-game settings, app settings, play sessions.
    r#"
    CREATE TABLE IF NOT EXISTS games (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        title            TEXT    NOT NULL,
        original_title   TEXT,
        exe_path         TEXT    NOT NULL,
        exe_key          TEXT    NOT NULL,
        install_dir      TEXT    NOT NULL,
        working_dir      TEXT,
        launch_args      TEXT    NOT NULL DEFAULT '[]',
        status           TEXT    NOT NULL DEFAULT 'unplayed',
        favorite         INTEGER NOT NULL DEFAULT 0,
        playtime_seconds INTEGER NOT NULL DEFAULT 0,
        last_played_at   INTEGER,
        added_at         INTEGER NOT NULL,
        metadata         TEXT    NOT NULL DEFAULT '{}',
        notes            TEXT
    );
    CREATE INDEX IF NOT EXISTS games_exe_key ON games(exe_key);

    CREATE TABLE IF NOT EXISTS game_settings (
        game_id    INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
        settings   TEXT    NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS app_settings (
        id         INTEGER PRIMARY KEY CHECK (id = 1),
        settings   TEXT    NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS play_sessions (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        game_id          INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        started_at       INTEGER NOT NULL,
        ended_at         INTEGER NOT NULL,
        duration_seconds INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS play_sessions_game ON play_sessions(game_id, started_at);
    "#,
    // v2: save locations + save backups index, metadata timestamp.
    r#"
    CREATE TABLE IF NOT EXISTS save_locations (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        path    TEXT    NOT NULL,
        source  TEXT    NOT NULL,
        reason  TEXT,
        UNIQUE (game_id, path, source)
    );

    CREATE TABLE IF NOT EXISTS save_backups (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        game_id      INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        created_at   INTEGER NOT NULL,
        label        TEXT,
        kind         TEXT    NOT NULL DEFAULT 'manual',
        size_bytes   INTEGER NOT NULL,
        file_count   INTEGER NOT NULL DEFAULT 0,
        archive_path TEXT    NOT NULL,
        source_paths TEXT    NOT NULL
    );
    CREATE INDEX IF NOT EXISTS save_backups_game ON save_backups(game_id, created_at);

    ALTER TABLE games ADD COLUMN updated_at INTEGER;
    "#,
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub(crate) fn db_err(e: rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::Database, "Library database error").with_details(e.to_string())
}

pub(crate) fn json_err(what: &str, e: serde_json::Error) -> AppError {
    AppError::new(ErrorCode::Database, format!("Stored {what} could not be read")).with_details(e.to_string())
}

pub fn open_file(path: &Path) -> Result<Connection, AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path).map_err(|e| {
        AppError::new(ErrorCode::Database, format!("Cannot open library database at {}", path.display()))
            .with_details(e.to_string())
    })?;
    // WAL is not available for in-memory DBs; ignore the returned mode.
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(())).map_err(db_err)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection, AppError> {
    let conn = Connection::open_in_memory().map_err(db_err)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;").map_err(db_err)?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(db_err)?;
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(db_err)
}

/// Apply migrations up to `target` (inclusive). Idempotent.
pub fn migrate_to(conn: &Connection, target: i64) -> Result<(), AppError> {
    let current = schema_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(AppError::new(
            ErrorCode::Database,
            "The library database was created by a newer version of Yomika",
        )
        .with_details(format!("database schema v{current}, supported up to v{SCHEMA_VERSION}")));
    }
    for version in current..target.min(SCHEMA_VERSION) {
        let sql = MIGRATIONS[version as usize];
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        tx.execute_batch(sql).map_err(|e| {
            AppError::new(ErrorCode::Database, format!("Database migration to v{} failed", version + 1))
                .with_details(e.to_string())
        })?;
        tx.pragma_update(None, "user_version", version + 1).map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        tracing::info!(version = version + 1, "library database migrated");
    }
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<(), AppError> {
    migrate_to(conn, SCHEMA_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_reaches_latest_version() {
        let conn = open_in_memory().unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn open_twice_is_idempotent_and_uses_wal() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sub dir/ライブラリ.sqlite3");
        {
            let conn = open_file(&path).unwrap();
            conn.execute(
                "INSERT INTO games (title, exe_path, exe_key, install_dir, added_at) VALUES ('a','/a.exe','/a.exe','/',1)",
                [],
            )
            .unwrap();
        }
        let conn = open_file(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        // Running migrate again is a no-op.
        migrate(&conn).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn upgrade_from_v1_keeps_data() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lib.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            migrate_to(&conn, 1).unwrap();
            assert_eq!(schema_version(&conn).unwrap(), 1);
            conn.execute(
                "INSERT INTO games (title, exe_path, exe_key, install_dir, added_at, playtime_seconds) \
                 VALUES ('Ever17','/g/e17.exe','/g/e17.exe','/g',5, 120)",
                [],
            )
            .unwrap();
            // save tables do not exist yet in v1
            assert!(conn.prepare("SELECT * FROM save_backups").is_err());
        }
        let conn = open_file(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        let (title, pt): (String, i64) =
            conn.query_row("SELECT title, playtime_seconds FROM games", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((title.as_str(), pt), ("Ever17", 120));
        conn.prepare("SELECT id, kind, source_paths FROM save_backups").unwrap();
        conn.prepare("SELECT updated_at FROM games").unwrap();
    }

    #[test]
    fn newer_schema_is_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5).unwrap();
        let err = migrate(&conn).unwrap_err();
        assert_eq!(err.code, ErrorCode::Database);
        assert!(err.message.contains("newer version"));
    }
}
