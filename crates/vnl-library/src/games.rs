//! Games CRUD, queries, per-game/app settings and playtime.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};
use vnl_core::{
    AppError, AppSettings, ErrorCode, Game, GameId, GameMetadata, GameQuery, GameSettings, GameSort, PlayStatus,
    PrefixChoice, TranslationConfig, now_secs,
};

use crate::Library;
use crate::db::{db_err, json_err};
use crate::text::{fold, path_key};
use crate::types::{ImportRequest, PlaySession};

const GAME_COLUMNS: &str = "id, title, original_title, exe_path, install_dir, working_dir, launch_args, status, \
     favorite, playtime_seconds, last_played_at, added_at, metadata, notes";

fn status_str(s: PlayStatus) -> &'static str {
    match s {
        PlayStatus::Unplayed => "unplayed",
        PlayStatus::Playing => "playing",
        PlayStatus::Completed => "completed",
        PlayStatus::OnHold => "onHold",
        PlayStatus::Dropped => "dropped",
        PlayStatus::Planned => "planned",
    }
}

fn parse_status(s: &str) -> PlayStatus {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or_default()
}

struct RawGame {
    game: Game,
    launch_args: String,
    metadata: String,
}

fn read_row(row: &Row<'_>) -> rusqlite::Result<RawGame> {
    let status: String = row.get(7)?;
    Ok(RawGame {
        game: Game {
            id: row.get(0)?,
            title: row.get(1)?,
            original_title: row.get(2)?,
            exe_path: PathBuf::from(row.get::<_, String>(3)?),
            install_dir: PathBuf::from(row.get::<_, String>(4)?),
            working_dir: row.get::<_, Option<String>>(5)?.map(PathBuf::from),
            launch_args: Vec::new(),
            status: parse_status(&status),
            favorite: row.get(8)?,
            playtime_seconds: row.get(9)?,
            last_played_at: row.get(10)?,
            added_at: row.get(11)?,
            metadata: GameMetadata::default(),
            notes: row.get(13)?,
        },
        launch_args: row.get(6)?,
        metadata: row.get(12)?,
    })
}

fn finish(raw: RawGame) -> Game {
    let mut game = raw.game;
    // Tolerate damaged JSON columns: a broken metadata blob must not hide the game.
    game.launch_args = serde_json::from_str(&raw.launch_args).unwrap_or_else(|e| {
        tracing::warn!(game = game.id, error = %e, "invalid launch_args JSON, ignoring");
        Vec::new()
    });
    game.metadata = serde_json::from_str(&raw.metadata).unwrap_or_else(|e| {
        tracing::warn!(game = game.id, error = %e, "invalid metadata JSON, ignoring");
        GameMetadata::default()
    });
    game
}

pub(crate) fn load_game(conn: &Connection, id: GameId) -> Result<Game, AppError> {
    conn.query_row(&format!("SELECT {GAME_COLUMNS} FROM games WHERE id = ?1"), [id], read_row)
        .optional()
        .map_err(db_err)?
        .map(finish)
        .ok_or_else(|| AppError::not_found(format!("Game {id} is not in the library")))
}

fn ensure_game_exists(conn: &Connection, id: GameId) -> Result<(), AppError> {
    let exists: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM games WHERE id = ?1)", [id], |r| r.get(0)).map_err(db_err)?;
    if exists { Ok(()) } else { Err(AppError::not_found(format!("Game {id} is not in the library"))) }
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String, AppError> {
    serde_json::to_string(v).map_err(|e| AppError::internal("serialization failed").with_details(e.to_string()))
}

fn require_absolute(p: &Path, what: &str) -> Result<(), AppError> {
    if p.as_os_str().is_empty() || !p.is_absolute() {
        return Err(AppError::invalid(format!("{what} must be an absolute path")).with_details(p.display().to_string()));
    }
    Ok(())
}

/// Default settings for a newly imported game.
pub fn default_game_settings(app: &AppSettings) -> GameSettings {
    let mut settings = GameSettings { graphics: app.default_graphics.clone(), ..Default::default() };
    settings.runtime.prefix = PrefixChoice::Dedicated;
    settings.translation = TranslationConfig { enabled: false, ..Default::default() };
    settings
}

fn matches_search(game: &Game, needle: &str) -> bool {
    let hay = |s: &str| fold(s).contains(needle);
    hay(&game.title)
        || game.original_title.as_deref().is_some_and(hay)
        || game.metadata.aliases.iter().any(|a| hay(a))
        || game.metadata.developers.iter().any(|d| hay(d))
}

fn compare(a: &Game, b: &Game, sort: GameSort, descending: bool) -> Ordering {
    // Missing values always sort last, regardless of direction.
    fn opt<T: Ord>(a: Option<T>, b: Option<T>, descending: bool) -> Ordering {
        match (a, b) {
            (Some(x), Some(y)) => {
                if descending {
                    y.cmp(&x)
                } else {
                    x.cmp(&y)
                }
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
    let primary = match sort {
        GameSort::Title => opt(Some(fold(&a.title)), Some(fold(&b.title)), descending),
        GameSort::LastPlayed => opt(a.last_played_at, b.last_played_at, descending),
        GameSort::Playtime => opt(Some(a.playtime_seconds), Some(b.playtime_seconds), descending),
        GameSort::AddedAt => opt(Some((a.added_at, a.id)), Some((b.added_at, b.id)), descending),
        GameSort::ReleaseDate => opt(
            a.metadata.release_date.as_deref().filter(|d| !d.is_empty()),
            b.metadata.release_date.as_deref().filter(|d| !d.is_empty()),
            descending,
        ),
    };
    primary.then_with(|| fold(&a.title).cmp(&fold(&b.title))).then_with(|| a.id.cmp(&b.id))
}

impl Library {
    pub fn list_games(&self, query: &GameQuery) -> Result<Vec<Game>, AppError> {
        let conn = self.conn();
        let mut sql = format!("SELECT {GAME_COLUMNS} FROM games WHERE 1=1");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(status) = query.status {
            sql.push_str(" AND status = ?");
            args.push(Box::new(status_str(status)));
        }
        if query.favorite_only {
            sql.push_str(" AND favorite = 1");
        }
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows =
            stmt.query_map(rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())), read_row).map_err(db_err)?;
        let mut games = Vec::new();
        for row in rows {
            games.push(finish(row.map_err(db_err)?));
        }
        drop(stmt);
        drop(conn);

        if let Some(needle) = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let needle = fold(needle);
            games.retain(|g| matches_search(g, &needle));
        }
        if let Some(tag) = query.tag.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let tag = fold(tag);
            games.retain(|g| g.metadata.tags.iter().any(|t| fold(t) == tag));
        }
        games.sort_by(|a, b| compare(a, b, query.sort, query.descending));
        Ok(games)
    }

    pub fn get_game(&self, id: GameId) -> Result<Game, AppError> {
        load_game(&self.conn(), id)
    }

    /// Game whose executable resolves to the same file as `exe`, if any.
    pub fn find_game_by_exe(&self, exe: &Path) -> Result<Option<GameId>, AppError> {
        let key = path_key(exe);
        self.conn()
            .query_row("SELECT id FROM games WHERE exe_key = ?1 ORDER BY id LIMIT 1", [key], |r| r.get(0))
            .optional()
            .map_err(db_err)
    }

    pub fn import_game(&self, request: ImportRequest) -> Result<Game, AppError> {
        let title = request.title.trim();
        if title.is_empty() {
            return Err(AppError::invalid("Title must not be empty"));
        }
        require_absolute(&request.exe_path, "Executable path")?;
        if !request.exe_path.is_file() {
            return Err(AppError::new(ErrorCode::NotFound, "The executable does not exist or is not a file")
                .with_details(request.exe_path.display().to_string()));
        }
        let install_dir = match request.install_dir.filter(|p| !p.as_os_str().is_empty()) {
            Some(dir) => dir,
            None => request.exe_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/")),
        };
        require_absolute(&install_dir, "Install folder")?;
        let working_dir = request.working_dir.filter(|p| !p.as_os_str().is_empty());
        if let Some(dir) = &working_dir {
            require_absolute(dir, "Working directory")?;
        }
        if let Some(existing) = self.find_game_by_exe(&request.exe_path)? {
            return Err(AppError::invalid("This executable is already in the library")
                .with_details(format!("existingGameId={existing}")));
        }

        let settings = match request.settings {
            Some(s) => s,
            None => default_game_settings(&self.get_app_settings()?),
        };
        let metadata = GameMetadata {
            vndb_id: request.vndb_id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            ..Default::default()
        };
        let now = now_secs();

        let mut conn = self.conn();
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute(
            "INSERT INTO games (title, exe_path, exe_key, install_dir, working_dir, launch_args, status, favorite, \
             playtime_seconds, added_at, metadata, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'unplayed', 0, 0, ?7, ?8, ?7)",
            params![
                title,
                request.exe_path.to_string_lossy(),
                path_key(&request.exe_path),
                install_dir.to_string_lossy(),
                working_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
                to_json(&request.launch_args)?,
                now,
                to_json(&metadata)?,
            ],
        )
        .map_err(db_err)?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO game_settings (game_id, settings, updated_at) VALUES (?1, ?2, ?3)",
            params![id, to_json(&settings)?, now],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        load_game(&conn, id)
    }

    /// Replace the user-editable fields. `id`, `addedAt`, `playtimeSeconds`
    /// and `lastPlayedAt` are never changed by this call.
    pub fn update_game(&self, game: &Game) -> Result<Game, AppError> {
        let title = game.title.trim();
        if title.is_empty() {
            return Err(AppError::invalid("Title must not be empty"));
        }
        require_absolute(&game.exe_path, "Executable path")?;
        require_absolute(&game.install_dir, "Install folder")?;
        if let Some(dir) = &game.working_dir {
            require_absolute(dir, "Working directory")?;
        }
        let key = path_key(&game.exe_path);
        let conn = self.conn();
        ensure_game_exists(&conn, game.id)?;
        let dup: Option<GameId> = conn
            .query_row("SELECT id FROM games WHERE exe_key = ?1 AND id != ?2 LIMIT 1", params![key, game.id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db_err)?;
        if let Some(other) = dup {
            return Err(AppError::invalid("Another library entry already uses this executable")
                .with_details(format!("existingGameId={other}")));
        }
        conn.execute(
            "UPDATE games SET title = ?2, original_title = ?3, exe_path = ?4, exe_key = ?5, install_dir = ?6, \
             working_dir = ?7, launch_args = ?8, status = ?9, favorite = ?10, metadata = ?11, notes = ?12, \
             updated_at = ?13 WHERE id = ?1",
            params![
                game.id,
                title,
                game.original_title.as_deref().map(str::trim).filter(|s| !s.is_empty()),
                game.exe_path.to_string_lossy(),
                key,
                game.install_dir.to_string_lossy(),
                game.working_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
                to_json(&game.launch_args)?,
                status_str(game.status),
                game.favorite,
                to_json(&game.metadata)?,
                game.notes.as_deref().filter(|s| !s.trim().is_empty()),
                now_secs(),
            ],
        )
        .map_err(db_err)?;
        load_game(&conn, game.id)
    }

    /// Store VNDB-derived fields without touching anything else (playtime,
    /// status, paths may have changed concurrently).
    pub(crate) fn store_metadata(
        &self,
        id: GameId,
        title: Option<&str>,
        original_title: Option<&str>,
        metadata: &GameMetadata,
    ) -> Result<Game, AppError> {
        let conn = self.conn();
        ensure_game_exists(&conn, id)?;
        conn.execute(
            "UPDATE games SET title = COALESCE(?2, title), original_title = COALESCE(?3, original_title), \
             metadata = ?4, updated_at = ?5 WHERE id = ?1",
            params![id, title, original_title, to_json(metadata)?, now_secs()],
        )
        .map_err(db_err)?;
        load_game(&conn, id)
    }

    /// Remove the library entry (and its settings, sessions, save index rows).
    /// Never touches game files, prefixes or backup archives on disk.
    pub fn remove_game(&self, id: GameId) -> Result<(), AppError> {
        let n = self.conn().execute("DELETE FROM games WHERE id = ?1", [id]).map_err(db_err)?;
        if n == 0 {
            return Err(AppError::not_found(format!("Game {id} is not in the library")));
        }
        Ok(())
    }

    // ------------------------------------------------------------ settings

    pub fn get_settings(&self, game_id: GameId) -> Result<GameSettings, AppError> {
        let stored: Option<String> = {
            let conn = self.conn();
            ensure_game_exists(&conn, game_id)?;
            conn.query_row("SELECT settings FROM game_settings WHERE game_id = ?1", [game_id], |r| r.get(0))
                .optional()
                .map_err(db_err)?
        };
        match stored {
            Some(json) => serde_json::from_str(&json).map_err(|e| json_err("game settings", e)),
            None => Ok(default_game_settings(&self.get_app_settings()?)),
        }
    }

    pub fn save_settings(&self, game_id: GameId, settings: &GameSettings) -> Result<GameSettings, AppError> {
        let conn = self.conn();
        ensure_game_exists(&conn, game_id)?;
        conn.execute(
            "INSERT INTO game_settings (game_id, settings, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(game_id) DO UPDATE SET settings = excluded.settings, updated_at = excluded.updated_at",
            params![game_id, to_json(settings)?, now_secs()],
        )
        .map_err(db_err)?;
        Ok(settings.clone())
    }

    pub fn get_app_settings(&self) -> Result<AppSettings, AppError> {
        let stored: Option<String> = self
            .conn()
            .query_row("SELECT settings FROM app_settings WHERE id = 1", [], |r| r.get(0))
            .optional()
            .map_err(db_err)?;
        match stored {
            Some(json) => serde_json::from_str(&json).map_err(|e| json_err("application settings", e)),
            None => Ok(AppSettings::default()),
        }
    }

    pub fn save_app_settings(&self, settings: &AppSettings) -> Result<AppSettings, AppError> {
        self.conn()
            .execute(
                "INSERT INTO app_settings (id, settings, updated_at) VALUES (1, ?1, ?2) \
                 ON CONFLICT(id) DO UPDATE SET settings = excluded.settings, updated_at = excluded.updated_at",
                params![to_json(settings)?, now_secs()],
            )
            .map_err(db_err)?;
        Ok(settings.clone())
    }

    // ------------------------------------------------------------ playtime

    /// Persist a finished play session and update the game's playtime and
    /// `last_played_at`. Sessions with a non-positive duration (clock skew,
    /// immediate crash) are ignored. An `Unplayed` game becomes `Playing`.
    pub fn record_play_session(&self, game_id: GameId, started_at: i64, ended_at: i64) -> Result<(), AppError> {
        let duration = ended_at.saturating_sub(started_at);
        let mut conn = self.conn();
        ensure_game_exists(&conn, game_id)?;
        if duration <= 0 {
            tracing::debug!(game_id, started_at, ended_at, "ignoring play session with non-positive duration");
            return Ok(());
        }
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute(
            "INSERT INTO play_sessions (game_id, started_at, ended_at, duration_seconds) VALUES (?1, ?2, ?3, ?4)",
            params![game_id, started_at, ended_at, duration],
        )
        .map_err(db_err)?;
        tx.execute(
            "UPDATE games SET playtime_seconds = playtime_seconds + ?2, \
             last_played_at = MAX(COALESCE(last_played_at, 0), ?3), \
             status = CASE status WHEN 'unplayed' THEN 'playing' ELSE status END WHERE id = ?1",
            params![game_id, duration, ended_at],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)
    }

    /// Play sessions of a game, most recent first.
    pub fn play_sessions(&self, game_id: GameId) -> Result<Vec<PlaySession>, AppError> {
        let conn = self.conn();
        ensure_game_exists(&conn, game_id)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, game_id, started_at, ended_at, duration_seconds FROM play_sessions \
                 WHERE game_id = ?1 ORDER BY started_at DESC, id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([game_id], |r| {
                Ok(PlaySession {
                    id: r.get(0)?,
                    game_id: r.get(1)?,
                    started_at: r.get(2)?,
                    ended_at: r.get(3)?,
                    duration_seconds: r.get(4)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }
}
