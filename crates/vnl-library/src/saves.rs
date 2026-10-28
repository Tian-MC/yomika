//! Save locations, backups and restore.
//!
//! Detection is a chain of [`SaveLocator`] strategies:
//! - [`InstallDirLocator`]: save-like folders inside the install dir;
//! - [`PrefixLocator`]: `drive_c/users/*/{AppData/..., Documents, Saved Games}`
//!   folders matching the game's title, original title, aliases or developer;
//! - manual locations configured by the user (stored in `save_locations`).
//!
//! Backups are `.tar.gz` archives under `backups_dir/<game_id>/` containing a
//! `manifest.json` and one top-level folder per source (`0/`, `1/`, ...).
//! Restoring requires `confirm == true`, always takes a safety backup first,
//! writes only below the recorded source paths and rejects unsafe entries.
//! Restore overwrites files contained in the backup; files created after the
//! backup are left in place (never deletes user files).

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Component, Path, PathBuf};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use vnl_core::{AppError, ErrorCode, Game, GameId, now_ms, now_secs};

use crate::Library;
use crate::db::db_err;
use crate::games::load_game;
use crate::text::{clean_path, compact};
use crate::types::{RestoreResult, SaveBackup, SaveBackupKind, SaveLocation, SaveLocationSource};

// ------------------------------------------------------------------ locators

/// Everything a locator may use to find saves.
pub struct SaveContext<'a> {
    pub game: &'a Game,
    /// `drive_c` of the game's Wine prefix, when known and initialised.
    pub drive_c: Option<&'a Path>,
}

pub trait SaveLocator: Send + Sync {
    fn name(&self) -> &'static str;
    fn locate(&self, ctx: &SaveContext<'_>) -> Vec<SaveLocation>;
}

fn detected(path: PathBuf, reason: String) -> SaveLocation {
    SaveLocation { exists: path.exists(), path, source: SaveLocationSource::Detected, reason: Some(reason) }
}

const SAVE_DIR_NAMES: &[&str] = &[
    "save",
    "saves",
    "savedata",
    "save_data",
    "save data",
    "savegame",
    "savegames",
    "saved",
    "savefile",
    "savefiles",
    "userdata",
    "sav",
    "セーブ",
    "セーブデータ",
];

/// Save-like folders inside the install dir (depth <= 3).
pub struct InstallDirLocator;

impl SaveLocator for InstallDirLocator {
    fn name(&self) -> &'static str {
        "installDir"
    }

    fn locate(&self, ctx: &SaveContext<'_>) -> Vec<SaveLocation> {
        let root = &ctx.game.install_dir;
        let mut out = Vec::new();
        let mut stack = vec![(root.clone(), 0usize)];
        let mut budget = 20_000usize;
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                budget = budget.saturating_sub(1);
                if budget == 0 {
                    return out;
                }
                let Ok(ft) = entry.file_type() else { continue };
                if !ft.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if SAVE_DIR_NAMES.contains(&name.as_str()) {
                    let rel = entry.path().strip_prefix(root).map(Path::to_path_buf).unwrap_or_default();
                    out.push(detected(entry.path(), format!("Folder \"{}\" in the install folder", rel.display())));
                } else if depth < 3 {
                    stack.push((entry.path(), depth + 1));
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }
}

/// Per-user folders inside a Wine prefix where Windows games keep saves.
const PREFIX_ROOTS: &[&str] = &[
    "AppData/Roaming",
    "AppData/Local",
    "AppData/LocalLow",
    "Documents",
    "My Documents",
    "Saved Games",
    "Application Data",
    "Local Settings/Application Data",
];

/// Folders that never contain a specific game's saves.
const NOISE_DIRS: &[&str] = &[
    "microsoft",
    "temp",
    "packages",
    "wine",
    "history",
    "cache",
    "crashdumps",
    "programs",
    "d3dscache",
    "nvidia",
    "nvidiacorporation",
    "mozilla",
    "google",
    "unity",
    "mypictures",
    "mymusic",
    "myvideos",
    "pictures",
    "music",
    "videos",
    "downloads",
    "desktop",
    "templates",
    "steam",
    "dxvk",
    "vkd3d",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    Title,
    Developer,
}

fn keys_for(game: &Game) -> Vec<(String, KeyKind)> {
    let mut keys: Vec<(String, KeyKind)> = Vec::new();
    let mut push = |s: &str, kind| {
        let k = compact(s);
        if k.chars().count() >= 2 && !keys.iter().any(|(x, _)| *x == k) {
            keys.push((k, kind));
        }
    };
    push(&game.title, KeyKind::Title);
    if let Some(o) = &game.original_title {
        push(o, KeyKind::Title);
    }
    for a in &game.metadata.aliases {
        push(a, KeyKind::Title);
    }
    if let Some(stem) = game.exe_path.file_stem() {
        let stem = stem.to_string_lossy();
        let k = compact(&stem);
        if k.chars().count() >= 4 && !["game", "start", "launcher", "play"].contains(&k.as_str()) {
            push(&stem, KeyKind::Title);
        }
    }
    for d in &game.metadata.developers {
        push(d, KeyKind::Developer);
    }
    keys
}

/// Loose folder-name match: equality, or containment of a long-enough key.
fn name_matches(dir_key: &str, key: &str) -> bool {
    if dir_key.is_empty() || key.is_empty() {
        return false;
    }
    let (dl, kl) = (dir_key.chars().count(), key.chars().count());
    dir_key == key
        || (kl >= 4 && dir_key.contains(key))
        || (dl >= 4 && (key.starts_with(dir_key) || (dl * 2 >= kl && key.contains(dir_key))))
}

fn best_match(dir_key: &str, keys: &[(String, KeyKind)]) -> Option<KeyKind> {
    if keys.iter().any(|(k, kind)| *kind == KeyKind::Title && name_matches(dir_key, k)) {
        Some(KeyKind::Title)
    } else if keys.iter().any(|(k, kind)| *kind == KeyKind::Developer && name_matches(dir_key, k)) {
        Some(KeyKind::Developer)
    } else {
        None
    }
}

fn child_dirs(dir: &Path) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(PathBuf, String)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (e.path(), e.file_name().to_string_lossy().into_owned()))
        .collect();
    out.sort();
    out
}

pub struct PrefixLocator;

impl SaveLocator for PrefixLocator {
    fn name(&self) -> &'static str {
        "prefix"
    }

    fn locate(&self, ctx: &SaveContext<'_>) -> Vec<SaveLocation> {
        let Some(drive_c) = ctx.drive_c else { return Vec::new() };
        let keys = keys_for(ctx.game);
        let mut out = Vec::new();
        for (user_dir, user) in child_dirs(&drive_c.join("users")) {
            for root_rel in PREFIX_ROOTS {
                let root = user_dir.join(root_rel);
                for (lvl1, name1) in child_dirs(&root) {
                    let key1 = compact(&name1);
                    if NOISE_DIRS.contains(&key1.as_str()) {
                        continue;
                    }
                    let label = format!("{root_rel}/{name1}");
                    match best_match(&key1, &keys) {
                        Some(KeyKind::Title) => {
                            out.push(detected(lvl1, format!("{label} (title match, user {user})")));
                        }
                        kind => {
                            // Publisher/Game layout, e.g. AppData/LocalLow/<Company>/<Product>.
                            let mut found_child = false;
                            for (lvl2, name2) in child_dirs(&lvl1) {
                                if best_match(&compact(&name2), &keys) == Some(KeyKind::Title) {
                                    found_child = true;
                                    out.push(detected(lvl2, format!("{label}/{name2} (title match, user {user})")));
                                }
                            }
                            if kind == Some(KeyKind::Developer) && !found_child {
                                out.push(detected(lvl1, format!("{label} (developer match, user {user})")));
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

pub fn default_locators() -> Vec<Box<dyn SaveLocator>> {
    vec![Box::new(InstallDirLocator), Box::new(PrefixLocator)]
}

/// Run all locators, de-duplicating by path (first reason wins).
pub fn detect_with(locators: &[Box<dyn SaveLocator>], ctx: &SaveContext<'_>) -> Vec<SaveLocation> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for locator in locators {
        for loc in locator.locate(ctx) {
            let key = std::fs::canonicalize(&loc.path).unwrap_or_else(|_| clean_path(&loc.path));
            if seen.insert(key) {
                out.push(loc);
            }
        }
    }
    out
}

// ------------------------------------------------------------------- archive

const MANIFEST: &str = "manifest.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: u32,
    game_id: GameId,
    created_at: i64,
    sources: Vec<ManifestSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestSource {
    index: usize,
    path: PathBuf,
    is_file: bool,
}

struct ArchiveStats {
    size_bytes: u64,
    file_count: u64,
}

fn io_err(context: &str, e: io::Error) -> AppError {
    AppError::new(ErrorCode::Io, context.to_string()).with_details(e.to_string())
}

/// Recursively add `dir` under `name` (symlinks are skipped, never followed).
fn append_dir<W: Write>(
    builder: &mut tar::Builder<W>,
    dir: &Path,
    name: &Path,
    stats: &mut ArchiveStats,
    depth: usize,
) -> io::Result<()> {
    builder.append_dir(name, dir)?;
    if depth > 32 {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let ft = entry.file_type()?;
        let child_name = name.join(entry.file_name());
        if ft.is_symlink() {
            tracing::debug!(path = %entry.path().display(), "skipping symlink in save folder");
        } else if ft.is_dir() {
            append_dir(builder, &entry.path(), &child_name, stats, depth + 1)?;
        } else if ft.is_file() {
            let mut f = File::open(entry.path())?;
            builder.append_file(&child_name, &mut f)?;
            stats.size_bytes += entry.metadata()?.len();
            stats.file_count += 1;
        }
    }
    Ok(())
}

fn write_archive(archive: &Path, manifest: &Manifest) -> Result<ArchiveStats, AppError> {
    let parent = archive.parent().ok_or_else(|| AppError::internal("archive path has no parent"))?;
    std::fs::create_dir_all(parent).map_err(|e| io_err("Cannot create the backup folder", e))?;
    let tmp = parent.join(format!(".{}.partial", archive.file_name().unwrap_or_default().to_string_lossy()));
    let mut stats = ArchiveStats { size_bytes: 0, file_count: 0 };
    let result = (|| -> io::Result<()> {
        let file = File::create(&tmp)?;
        let gz = GzEncoder::new(BufWriter::new(file), Compression::default());
        let mut builder = tar::Builder::new(gz);
        builder.follow_symlinks(false);
        let manifest_json = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest_json.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(manifest.created_at.max(0) as u64);
        header.set_cksum();
        builder.append_data(&mut header, MANIFEST, manifest_json.as_slice())?;
        for src in &manifest.sources {
            let base = PathBuf::from(src.index.to_string());
            if src.is_file {
                let mut f = File::open(&src.path)?;
                let name = base.join(src.path.file_name().unwrap_or_default());
                builder.append_file(name, &mut f)?;
                stats.size_bytes += f.metadata()?.len();
                stats.file_count += 1;
            } else {
                append_dir(&mut builder, &src.path, &base, &mut stats, 0)?;
            }
        }
        let gz = builder.into_inner()?;
        let mut writer = gz.finish()?;
        writer.flush()?;
        writer.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        std::fs::rename(&tmp, archive)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_err("Creating the save backup failed", e));
    }
    Ok(stats)
}

/// Validate an archive entry path: `<index>/<normal components...>`.
fn split_entry(path: &Path) -> Option<(usize, PathBuf)> {
    let mut comps = path.components();
    let index = match comps.next()? {
        Component::Normal(s) => s.to_str()?.parse::<usize>().ok()?,
        _ => return None,
    };
    let mut rel = PathBuf::new();
    for c in comps {
        match c {
            Component::Normal(s) => rel.push(s),
            Component::CurDir => {}
            _ => return None, // `..`, absolute, prefix
        }
    }
    Some((index, rel))
}

/// True if any existing component between `base` (exclusive) and `target` is a symlink.
fn crosses_symlink(base: &Path, rel: &Path) -> bool {
    let mut cur = base.to_path_buf();
    let comps: Vec<_> = rel.components().collect();
    for (i, c) in comps.iter().enumerate() {
        cur.push(c.as_os_str());
        let is_last = i + 1 == comps.len();
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() && !is_last => return true,
            Ok(_) => {}
            Err(_) => return false, // does not exist yet: nothing below can be a symlink
        }
    }
    false
}

/// Extract into `staging`, validating every entry. Returns (source index, relative path, is_dir) list.
fn extract_to_staging(
    archive: &Path,
    staging: &Path,
    n_sources: usize,
) -> Result<Vec<(usize, PathBuf, bool)>, AppError> {
    let file = File::open(archive).map_err(|e| {
        AppError::new(ErrorCode::NotFound, "The backup archive is missing")
            .with_details(format!("{}: {e}", archive.display()))
    })?;
    let mut ar = tar::Archive::new(GzDecoder::new(BufReader::new(file)));
    let corrupt =
        |e: io::Error| AppError::new(ErrorCode::Io, "The backup archive is damaged").with_details(e.to_string());
    let mut items = Vec::new();
    for entry in ar.entries().map_err(corrupt)? {
        let mut entry = entry.map_err(corrupt)?;
        let raw_path = entry.path().map_err(corrupt)?.into_owned();
        if raw_path == Path::new(MANIFEST) {
            continue;
        }
        let kind = entry.header().entry_type();
        let Some((index, rel)) = split_entry(&raw_path).filter(|(i, _)| *i < n_sources) else {
            return Err(AppError::new(ErrorCode::Io, "The backup archive contains an unsafe path")
                .with_details(raw_path.display().to_string()));
        };
        let dest = staging.join(index.to_string()).join(&rel);
        if kind.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| io_err("Cannot prepare restore", e))?;
            items.push((index, rel, true));
        } else if kind.is_file() {
            if rel.as_os_str().is_empty() {
                return Err(AppError::new(ErrorCode::Io, "The backup archive contains an invalid entry"));
            }
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p).map_err(|e| io_err("Cannot prepare restore", e))?;
            }
            let mut out = File::create(&dest).map_err(|e| io_err("Cannot prepare restore", e))?;
            io::copy(&mut entry, &mut out).map_err(corrupt)?;
            items.push((index, rel, false));
        } else {
            tracing::warn!(entry = %raw_path.display(), "skipping non-regular entry in save backup");
        }
    }
    Ok(items)
}

fn copy_atomic(src: &Path, dest: &Path) -> io::Result<()> {
    let dir = dest.parent().ok_or_else(|| io::Error::other("destination has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.vnl-restore", dest.file_name().unwrap_or_default().to_string_lossy()));
    let result = (|| {
        std::fs::copy(src, &tmp)?;
        File::open(&tmp)?.sync_all()?;
        std::fs::rename(&tmp, dest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn dir_has_files(p: &Path) -> bool {
    if p.is_file() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(p) else { return false };
    entries.flatten().any(|e| match e.file_type() {
        Ok(t) if t.is_file() => true,
        Ok(t) if t.is_dir() => dir_has_files(&e.path()),
        _ => false,
    })
}

// ------------------------------------------------------- Library integration

fn read_backup(row: &rusqlite::Row<'_>) -> rusqlite::Result<(SaveBackup, String)> {
    let kind: String = row.get(4)?;
    Ok((
        SaveBackup {
            id: row.get(0)?,
            game_id: row.get(1)?,
            created_at: row.get(2)?,
            label: row.get(3)?,
            kind: SaveBackupKind::parse(&kind),
            size_bytes: row.get::<_, i64>(5)?.max(0) as u64,
            file_count: row.get::<_, i64>(6)?.max(0) as u64,
            archive_path: PathBuf::from(row.get::<_, String>(7)?),
            source_paths: Vec::new(),
        },
        row.get(8)?,
    ))
}

fn finish_backup((mut b, sources): (SaveBackup, String)) -> SaveBackup {
    b.source_paths = serde_json::from_str(&sources).unwrap_or_default();
    b
}

const BACKUP_COLUMNS: &str = "id, game_id, created_at, label, kind, size_bytes, file_count, archive_path, source_paths";

impl Library {
    fn manual_locations(&self, game_id: GameId) -> Result<Vec<PathBuf>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT path FROM save_locations WHERE game_id = ?1 AND source = 'manual' ORDER BY id")
            .map_err(db_err)?;
        let rows = stmt.query_map([game_id], |r| r.get::<_, String>(0)).map_err(db_err)?;
        rows.map(|r| r.map(PathBuf::from)).collect::<Result<_, _>>().map_err(db_err)
    }

    /// Stored locations (manual + last detection) without scanning.
    pub fn save_locations(&self, game_id: GameId) -> Result<Vec<SaveLocation>, AppError> {
        let conn = self.conn();
        load_game(&conn, game_id)?;
        let mut stmt = conn
            .prepare(
                "SELECT path, source, reason FROM save_locations WHERE game_id = ?1 \
                 ORDER BY CASE source WHEN 'manual' THEN 0 ELSE 1 END, id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([game_id], |r| {
                let path = PathBuf::from(r.get::<_, String>(0)?);
                Ok(SaveLocation {
                    exists: path.exists(),
                    path,
                    source: SaveLocationSource::parse(&r.get::<_, String>(1)?),
                    reason: r.get(2)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    /// Detect save folders (install dir + optional prefix `drive_c`), store
    /// the detected ones and return manual + detected locations.
    pub fn detect_saves(&self, game_id: GameId, drive_c: Option<&Path>) -> Result<Vec<SaveLocation>, AppError> {
        self.detect_saves_with(game_id, drive_c, &default_locators())
    }

    pub fn detect_saves_with(
        &self,
        game_id: GameId,
        drive_c: Option<&Path>,
        locators: &[Box<dyn SaveLocator>],
    ) -> Result<Vec<SaveLocation>, AppError> {
        let game = self.get_game(game_id)?;
        let manual = self.manual_locations(game_id)?;
        // Filesystem scan happens without holding the DB lock.
        let found = detect_with(locators, &SaveContext { game: &game, drive_c });
        let found: Vec<SaveLocation> = found.into_iter().filter(|l| !manual.contains(&l.path)).collect();
        {
            let mut conn = self.conn();
            let tx = conn.transaction().map_err(db_err)?;
            tx.execute("DELETE FROM save_locations WHERE game_id = ?1 AND source = 'detected'", [game_id])
                .map_err(db_err)?;
            for loc in &found {
                tx.execute(
                    "INSERT OR IGNORE INTO save_locations (game_id, path, source, reason) VALUES (?1, ?2, 'detected', ?3)",
                    params![game_id, loc.path.to_string_lossy(), loc.reason],
                )
                .map_err(db_err)?;
            }
            tx.commit().map_err(db_err)?;
        }
        let mut all: Vec<SaveLocation> = manual
            .into_iter()
            .map(|p| SaveLocation { exists: p.exists(), path: p, source: SaveLocationSource::Manual, reason: None })
            .collect();
        all.extend(found);
        Ok(all)
    }

    /// Replace the user-configured save locations. Paths must be absolute;
    /// they do not need to exist yet.
    pub fn set_save_locations(&self, game_id: GameId, paths: &[PathBuf]) -> Result<Vec<SaveLocation>, AppError> {
        let mut cleaned: Vec<PathBuf> = Vec::new();
        for p in paths {
            if p.as_os_str().is_empty() {
                continue;
            }
            if !p.is_absolute() {
                return Err(
                    AppError::invalid("Save locations must be absolute paths").with_details(p.display().to_string())
                );
            }
            let p = clean_path(p);
            if p.parent().is_none() {
                return Err(AppError::invalid("The filesystem root cannot be a save location"));
            }
            if !cleaned.contains(&p) {
                cleaned.push(p);
            }
        }
        let mut conn = self.conn();
        load_game(&conn, game_id)?;
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute("DELETE FROM save_locations WHERE game_id = ?1 AND source = 'manual'", [game_id]).map_err(db_err)?;
        for p in &cleaned {
            tx.execute(
                "DELETE FROM save_locations WHERE game_id = ?1 AND source = 'detected' AND path = ?2",
                params![game_id, p.to_string_lossy()],
            )
            .map_err(db_err)?;
            tx.execute(
                "INSERT INTO save_locations (game_id, path, source) VALUES (?1, ?2, 'manual')",
                params![game_id, p.to_string_lossy()],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        drop(conn);
        self.save_locations(game_id)
    }

    pub fn list_backups(&self, game_id: GameId) -> Result<Vec<SaveBackup>, AppError> {
        let conn = self.conn();
        load_game(&conn, game_id)?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {BACKUP_COLUMNS} FROM save_backups WHERE game_id = ?1 ORDER BY created_at DESC, id DESC"
            ))
            .map_err(db_err)?;
        let rows = stmt.query_map([game_id], read_backup).map_err(db_err)?;
        rows.map(|r| r.map(finish_backup)).collect::<Result<_, _>>().map_err(db_err)
    }

    pub fn get_backup(&self, backup_id: i64) -> Result<SaveBackup, AppError> {
        self.conn()
            .query_row(&format!("SELECT {BACKUP_COLUMNS} FROM save_backups WHERE id = ?1"), [backup_id], read_backup)
            .optional()
            .map_err(db_err)?
            .map(finish_backup)
            .ok_or_else(|| AppError::not_found(format!("Save backup {backup_id} does not exist")))
    }

    /// Back up the game's saves. Uses the manual locations when configured,
    /// otherwise runs detection (`drive_c` enables prefix-based detection).
    pub fn create_backup(
        &self,
        game_id: GameId,
        label: Option<String>,
        drive_c: Option<&Path>,
    ) -> Result<SaveBackup, AppError> {
        let manual = self.manual_locations(game_id)?;
        let sources: Vec<PathBuf> = if manual.is_empty() {
            self.detect_saves(game_id, drive_c)?.into_iter().map(|l| l.path).collect()
        } else {
            self.get_game(game_id)?;
            manual
        };
        let sources: Vec<PathBuf> = sources.into_iter().filter(|p| p.exists()).collect();
        if sources.is_empty() {
            return Err(AppError::not_found(
                "No save folders were found for this game. Add the save location manually in the Saves tab.",
            ));
        }
        self.backup_paths(game_id, &sources, label, SaveBackupKind::Manual)
    }

    fn backup_paths(
        &self,
        game_id: GameId,
        sources: &[PathBuf],
        label: Option<String>,
        kind: SaveBackupKind,
    ) -> Result<SaveBackup, AppError> {
        let created_at = now_secs();
        let manifest = Manifest {
            version: 1,
            game_id,
            created_at,
            sources: sources
                .iter()
                .enumerate()
                .map(|(index, p)| ManifestSource { index, path: p.clone(), is_file: p.is_file() })
                .collect(),
        };
        let dir = self.paths.backups_dir().join(game_id.to_string());
        let mut archive = dir.join(format!("{}-{}.tar.gz", now_ms(), kind.as_str()));
        let mut n = 1;
        while archive.exists() {
            archive = dir.join(format!("{}-{}-{n}.tar.gz", now_ms(), kind.as_str()));
            n += 1;
        }
        let stats = write_archive(&archive, &manifest)?;
        let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
        let source_json = serde_json::to_string(sources).map_err(|e| AppError::internal(e.to_string()))?;
        let id = {
            let conn = self.conn();
            let res = conn.execute(
                "INSERT INTO save_backups (game_id, created_at, label, kind, size_bytes, file_count, archive_path, source_paths) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    game_id,
                    created_at,
                    label,
                    kind.as_str(),
                    stats.size_bytes as i64,
                    stats.file_count as i64,
                    archive.to_string_lossy(),
                    source_json
                ],
            );
            if let Err(e) = res {
                let _ = std::fs::remove_file(&archive);
                return Err(db_err(e));
            }
            conn.last_insert_rowid()
        };
        self.get_backup(id)
    }

    /// Restore a backup into its recorded source paths. Requires
    /// `confirm == true`; always takes a safety backup of the current saves first.
    pub fn restore_backup(&self, backup_id: i64, confirm: bool) -> Result<RestoreResult, AppError> {
        if !confirm {
            return Err(AppError::new(
                ErrorCode::ConfirmationRequired,
                "Restoring will overwrite the current save files. Confirm to continue (a safety backup is made first).",
            ));
        }
        let backup = self.get_backup(backup_id)?;
        let sources = backup.source_paths.clone();
        if sources.is_empty() || sources.iter().any(|p| !p.is_absolute()) {
            return Err(AppError::new(ErrorCode::Io, "The backup has no valid source paths recorded"));
        }

        // 1. Extract + validate the whole archive before touching any save.
        let staging = self.paths.backups_dir().join(backup.game_id.to_string()).join(format!(".restore-{}", now_ms()));
        let cleanup = |p: &Path| {
            let _ = std::fs::remove_dir_all(p);
        };
        let items = match extract_to_staging(&backup.archive_path, &staging, sources.len()) {
            Ok(items) => items,
            Err(e) => {
                cleanup(&staging);
                return Err(e);
            }
        };
        let source_is_file: Vec<bool> = {
            // A single-file source is archived as `<index>/<file name>`.
            (0..sources.len())
                .map(|i| {
                    let file_entries: Vec<_> = items.iter().filter(|(idx, _, d)| *idx == i && !d).collect();
                    let dir_entries = items.iter().any(|(idx, rel, d)| *idx == i && *d && rel.as_os_str().is_empty());
                    !dir_entries
                        && file_entries.len() == 1
                        && file_entries[0].1.as_os_str() == sources[i].file_name().unwrap_or_default()
                })
                .collect()
        };

        // 2. Safety backup of whatever is there now.
        let existing: Vec<PathBuf> = sources.iter().filter(|p| dir_has_files(p)).cloned().collect();
        let safety_backup = if existing.is_empty() {
            None
        } else {
            match self.backup_paths(
                backup.game_id,
                &existing,
                Some(format!("Before restoring backup #{}", backup.id)),
                SaveBackupKind::Safety,
            ) {
                Ok(b) => Some(b),
                Err(e) => {
                    cleanup(&staging);
                    return Err(AppError::new(e.code, "Could not back up the current saves; nothing was restored")
                        .with_details(e.details.unwrap_or(e.message)));
                }
            }
        };

        // 3. Copy staged files into place.
        let mut restored = 0u64;
        let mut failures: Vec<String> = Vec::new();
        for (index, rel, is_dir) in &items {
            let source = &sources[*index];
            let (base, rel_target): (PathBuf, PathBuf) = if source_is_file[*index] {
                (source.parent().map(Path::to_path_buf).unwrap_or_default(), rel.clone())
            } else {
                (source.clone(), rel.clone())
            };
            let target = base.join(&rel_target);
            if !clean_path(&target).starts_with(clean_path(&base)) || crosses_symlink(&base, &rel_target) {
                failures.push(format!("skipped unsafe target {}", target.display()));
                continue;
            }
            let staged = staging.join(index.to_string()).join(rel);
            let res = if *is_dir { std::fs::create_dir_all(&target) } else { copy_atomic(&staged, &target) };
            match res {
                Ok(()) if !is_dir => restored += 1,
                Ok(()) => {}
                Err(e) => failures.push(format!("{}: {e}", target.display())),
            }
        }
        cleanup(&staging);
        if !failures.is_empty() {
            return Err(AppError::new(
                ErrorCode::Io,
                format!(
                    "Restore finished with {} error(s). Your previous saves are in the safety backup.",
                    failures.len()
                ),
            )
            .with_details(failures.join("\n")));
        }
        Ok(RestoreResult { restored: backup, safety_backup, files_restored: restored })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_paths_are_validated() {
        assert_eq!(split_entry(Path::new("0/a/b.sav")), Some((0, PathBuf::from("a/b.sav"))));
        assert_eq!(split_entry(Path::new("3")), Some((3, PathBuf::new())));
        assert_eq!(split_entry(Path::new("0/../../etc/passwd")), None);
        assert_eq!(split_entry(Path::new("/etc/passwd")), None);
        assert_eq!(split_entry(Path::new("x/a")), None);
    }

    #[test]
    fn name_matching() {
        assert!(name_matches(&compact("Ever17"), &compact("Ever17 -the out of infinity-")));
        assert!(name_matches(&compact("Ever17 Save"), &compact("Ever17")));
        assert!(name_matches(&compact("千恋＊万花"), &compact("千恋＊万花")));
        assert!(!name_matches("land", &compact("Temptation Island")));
        assert!(!name_matches("ab", "abc"));
    }
}
