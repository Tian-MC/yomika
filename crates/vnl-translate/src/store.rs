//! SQLite storage for glossary entries and translation history
//! (`<translation_dir>/translation.sqlite3`).

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use vnl_core::{AppError, ErrorCode, GameId, TextOrigin, TranslationLine};

use crate::types::{GlossaryEntry, GlossaryKind};

pub struct TranslationStore {
    conn: Mutex<Connection>,
}

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::Database, format!("translation database error: {e}"))
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS glossary (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id    INTEGER,
    kind       TEXT NOT NULL DEFAULT 'term',
    source     TEXT NOT NULL,
    target     TEXT NOT NULL,
    note       TEXT,
    enabled    INTEGER NOT NULL DEFAULT 1,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS glossary_game ON glossary(game_id);
CREATE TABLE IF NOT EXISTS history (
    id              TEXT PRIMARY KEY,
    session_id      TEXT,
    game_id         INTEGER,
    origin          TEXT NOT NULL,
    source_text     TEXT NOT NULL,
    translated_text TEXT,
    source_language TEXT,
    target_language TEXT,
    provider_id     TEXT,
    error           TEXT,
    ts_ms           INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS history_game_ts ON history(game_id, ts_ms);
PRAGMA user_version = 1;
"#;

fn origin_str(o: TextOrigin) -> &'static str {
    match o {
        TextOrigin::Hook => "hook",
        TextOrigin::Ocr => "ocr",
        TextOrigin::Manual => "manual",
        TextOrigin::Clipboard => "clipboard",
    }
}

fn parse_origin(s: &str) -> TextOrigin {
    match s {
        "hook" => TextOrigin::Hook,
        "ocr" => TextOrigin::Ocr,
        "clipboard" => TextOrigin::Clipboard,
        _ => TextOrigin::Manual,
    }
}

impl TranslationStore {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, AppError> {
        Self::init(Connection::open_in_memory().map_err(db_err)?)
    }

    fn init(conn: Connection) -> Result<Self, AppError> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(db_err)?;
        conn.execute_batch(SCHEMA).map_err(db_err)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ------------------------------------------------------------ glossary

    /// `None` = every entry (global and per-game); `Some(game)` = global + that game's.
    pub fn glossary_list(&self, game_id: Option<GameId>) -> Result<Vec<GlossaryEntry>, AppError> {
        let conn = self.conn();
        let sql = "SELECT id, game_id, kind, source, target, note, enabled FROM glossary \
                   WHERE (?1 IS NULL OR game_id IS NULL OR game_id = ?1) ORDER BY game_id IS NOT NULL, kind, source";
        let mut stmt = conn.prepare(sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params![game_id], |r| {
                Ok(GlossaryEntry {
                    id: r.get(0)?,
                    game_id: r.get(1)?,
                    kind: GlossaryKind::parse(&r.get::<_, String>(2)?),
                    source: r.get(3)?,
                    target: r.get(4)?,
                    note: r.get(5)?,
                    enabled: r.get::<_, i64>(6)? != 0,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Enabled entries applying to a game; a game entry overrides a global one with the same source.
    pub fn effective_glossary(&self, game_id: Option<GameId>) -> Result<Vec<GlossaryEntry>, AppError> {
        let all = match game_id {
            Some(g) => self.glossary_list(Some(g))?,
            None => self.glossary_list(None)?.into_iter().filter(|e| e.game_id.is_none()).collect(),
        };
        let mut out: Vec<GlossaryEntry> = Vec::new();
        // Global entries come first in the ordering, so game entries replace them.
        for e in all.into_iter().filter(|e| e.enabled && !e.source.trim().is_empty()) {
            if let Some(existing) = out.iter_mut().find(|x| x.source == e.source) {
                *existing = e;
            } else {
                out.push(e);
            }
        }
        Ok(out)
    }

    pub fn glossary_upsert(&self, entry: &GlossaryEntry) -> Result<GlossaryEntry, AppError> {
        if entry.source.trim().is_empty() {
            return Err(AppError::invalid("Glossary source text must not be empty"));
        }
        let conn = self.conn();
        let now = vnl_core::now_secs();
        let id = if entry.id > 0 {
            let n = conn
                .execute(
                    "UPDATE glossary SET game_id=?2, kind=?3, source=?4, target=?5, note=?6, enabled=?7, updated_at=?8 WHERE id=?1",
                    params![
                        entry.id,
                        entry.game_id,
                        entry.kind.as_str(),
                        entry.source,
                        entry.target,
                        entry.note,
                        entry.enabled as i64,
                        now
                    ],
                )
                .map_err(db_err)?;
            if n == 0 {
                return Err(AppError::not_found(format!("glossary entry {} not found", entry.id)));
            }
            entry.id
        } else {
            conn.execute(
                "INSERT INTO glossary (game_id, kind, source, target, note, enabled, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![entry.game_id, entry.kind.as_str(), entry.source, entry.target, entry.note, entry.enabled as i64, now],
            )
            .map_err(db_err)?;
            conn.last_insert_rowid()
        };
        Ok(GlossaryEntry { id, ..entry.clone() })
    }

    pub fn glossary_delete(&self, id: i64) -> Result<(), AppError> {
        let n = self.conn().execute("DELETE FROM glossary WHERE id=?1", params![id]).map_err(db_err)?;
        if n == 0 { Err(AppError::not_found(format!("glossary entry {id} not found"))) } else { Ok(()) }
    }

    // ------------------------------------------------------------- history

    pub fn history_put(&self, line: &TranslationLine) -> Result<(), AppError> {
        self.conn()
            .execute(
                "INSERT INTO history (id, session_id, game_id, origin, source_text, translated_text, source_language, \
                 target_language, provider_id, error, ts_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) \
                 ON CONFLICT(id) DO UPDATE SET translated_text=excluded.translated_text, error=excluded.error, \
                 provider_id=excluded.provider_id, target_language=excluded.target_language, source_language=excluded.source_language",
                params![
                    line.id,
                    line.session_id,
                    line.game_id,
                    origin_str(line.origin),
                    line.source_text,
                    line.translated_text,
                    line.source_language,
                    line.target_language,
                    line.provider_id,
                    line.error,
                    line.ts_ms
                ],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Latest `limit` lines (oldest first). `None` = all games.
    pub fn history_list(&self, game_id: Option<GameId>, limit: u32) -> Result<Vec<TranslationLine>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, game_id, origin, source_text, translated_text, source_language, target_language, \
                 provider_id, error, ts_ms FROM history WHERE (?1 IS NULL OR game_id = ?1) \
                 ORDER BY ts_ms DESC, rowid DESC LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![game_id, limit.max(1)], |r| {
                Ok(TranslationLine {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    game_id: r.get(2)?,
                    origin: parse_origin(&r.get::<_, String>(3)?),
                    source_text: r.get(4)?,
                    translated_text: r.get(5)?,
                    source_language: r.get(6)?,
                    target_language: r.get(7)?,
                    provider_id: r.get(8)?,
                    error: r.get(9)?,
                    ts_ms: r.get(10)?,
                })
            })
            .map_err(db_err)?;
        let mut lines = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        lines.reverse();
        Ok(lines)
    }

    pub fn history_get(&self, id: &str) -> Result<Option<String>, AppError> {
        self.conn()
            .query_row("SELECT translated_text FROM history WHERE id=?1", params![id], |r| r.get(0))
            .optional()
            .map_err(db_err)
    }

    /// `None` = clear everything.
    pub fn history_clear(&self, game_id: Option<GameId>) -> Result<usize, AppError> {
        let conn = self.conn();
        let n = match game_id {
            Some(g) => conn.execute("DELETE FROM history WHERE game_id=?1", params![g]),
            None => conn.execute("DELETE FROM history", []),
        }
        .map_err(db_err)?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(game: Option<GameId>, src: &str, tgt: &str) -> GlossaryEntry {
        GlossaryEntry { id: 0, game_id: game, kind: GlossaryKind::Term, source: src.into(), target: tgt.into(), note: None, enabled: true }
    }

    fn line(id: &str, game: Option<GameId>, ts: i64) -> TranslationLine {
        TranslationLine {
            id: id.into(),
            session_id: None,
            game_id: game,
            origin: TextOrigin::Hook,
            source_text: format!("src {id}"),
            translated_text: None,
            source_language: Some("ja".into()),
            target_language: Some("en".into()),
            provider_id: None,
            error: None,
            ts_ms: ts,
        }
    }

    #[test]
    fn glossary_crud_and_scoping() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TranslationStore::open(&tmp.path().join("t.sqlite3")).unwrap();
        let g = store.glossary_upsert(&entry(None, "先輩", "senpai")).unwrap();
        assert!(g.id > 0);
        store.glossary_upsert(&entry(Some(1), "先輩", "Senpai-san")).unwrap();
        store.glossary_upsert(&entry(Some(2), "部長", "Club President")).unwrap();
        let mut disabled = entry(Some(1), "会長", "Prez");
        disabled.enabled = false;
        store.glossary_upsert(&disabled).unwrap();

        assert_eq!(store.glossary_list(None).unwrap().len(), 4);
        assert_eq!(store.glossary_list(Some(1)).unwrap().len(), 3);
        let eff = store.effective_glossary(Some(1)).unwrap();
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].target, "Senpai-san");
        assert_eq!(store.effective_glossary(None).unwrap()[0].target, "senpai");

        let mut updated = g.clone();
        updated.target = "upperclassman".into();
        updated.kind = GlossaryKind::CharacterName;
        store.glossary_upsert(&updated).unwrap();
        let reread = store.glossary_list(None).unwrap().into_iter().find(|e| e.id == g.id).unwrap();
        assert_eq!(reread.kind, GlossaryKind::CharacterName);
        assert_eq!(reread.target, "upperclassman");

        store.glossary_delete(g.id).unwrap();
        assert_eq!(store.glossary_delete(g.id).unwrap_err().code, ErrorCode::NotFound);
        assert!(store.glossary_upsert(&entry(None, " ", "x")).is_err());
    }

    #[test]
    fn history_roundtrip_limit_clear() {
        let store = TranslationStore::open_in_memory().unwrap();
        for i in 0..5 {
            store.history_put(&line(&format!("a{i}"), Some(1), 100 + i)).unwrap();
        }
        store.history_put(&line("b0", Some(2), 50)).unwrap();
        // Upsert keeps one row and updates the translation.
        let mut l = line("a4", Some(1), 104);
        l.translated_text = Some("done".into());
        store.history_put(&l).unwrap();

        let h = store.history_list(Some(1), 3).unwrap();
        assert_eq!(h.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), vec!["a2", "a3", "a4"]);
        assert_eq!(h[2].translated_text.as_deref(), Some("done"));
        assert_eq!(store.history_list(None, 100).unwrap().len(), 6);
        assert_eq!(store.history_clear(Some(1)).unwrap(), 5);
        assert_eq!(store.history_list(None, 100).unwrap().len(), 1);
        store.history_clear(None).unwrap();
        assert!(store.history_list(None, 100).unwrap().is_empty());
    }
}
