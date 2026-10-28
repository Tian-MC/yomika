//! Prefix registry (`data_dir/prefixes.json`) and prefix layout helpers.
//!
//! ## Layout
//! Managed prefixes live in `prefixes_dir/<id>` and use the Steam
//! *compat-data* layout: `PrefixInfo.path` is the compat-data directory and
//! the real `WINEPREFIX` is `<path>/pfx`. This is exactly what Proton's
//! `CompatData` class expects (`STEAM_COMPAT_DATA_PATH=<path>`, prefix in
//! `<path>/pfx/`), and plain Wine is simply pointed at `WINEPREFIX=<path>/pfx`,
//! so one prefix works with both runner kinds.
//!
//! External/custom prefixes: if `<path>/pfx` exists they are treated as
//! compat-data dirs (Steam compatdata, umu prefixes whose `pfx` is a `.`
//! symlink), otherwise as a plain `WINEPREFIX`. A plain prefix is never
//! handed to the `proton` script (it would add `pfx/` and Proton's prefix
//! files to the user's directory); Proton runners use their bundled
//! `files/bin/wine` directly on it instead — see `command.rs`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use vnl_core::{AppError, GameId, PrefixArch, PrefixInfo, PrefixKind};

use crate::util::write_atomic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixLayout {
    /// `path` is a compat-data dir, WINEPREFIX = `path/pfx`.
    CompatData,
    /// `path` is the WINEPREFIX itself.
    Plain,
}

pub fn layout(prefix: &PrefixInfo) -> PrefixLayout {
    if prefix.managed || prefix.path.join("pfx").is_dir() { PrefixLayout::CompatData } else { PrefixLayout::Plain }
}

/// The directory to use as `WINEPREFIX`.
pub fn wine_prefix_dir(prefix: &PrefixInfo) -> PathBuf {
    match layout(prefix) {
        PrefixLayout::CompatData => prefix.path.join("pfx"),
        PrefixLayout::Plain => prefix.path.clone(),
    }
}

pub fn drive_c(prefix: &PrefixInfo) -> PathBuf {
    wine_prefix_dir(prefix).join("drive_c")
}

pub fn is_initialized(prefix: &PrefixInfo) -> bool {
    drive_c(prefix).is_dir()
}

/// Reads `#arch=win32|win64` from `system.reg` (written by wineboot).
pub fn detect_arch(wineprefix: &Path) -> Option<PrefixArch> {
    let content = std::fs::read(wineprefix.join("system.reg")).ok()?;
    let head = String::from_utf8_lossy(&content[..content.len().min(4096)]).into_owned();
    head.lines().find_map(|l| match l.trim() {
        "#arch=win32" => Some(PrefixArch::Win32),
        "#arch=win64" => Some(PrefixArch::Win64),
        _ => None,
    })
}

/// True when `dir` is missing or an empty directory.
pub fn is_missing_or_empty(dir: &Path) -> Result<bool, AppError> {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => Ok(entries.next().is_none()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RegistryFile {
    version: u32,
    prefixes: Vec<PrefixInfo>,
}

/// Persistent list of known prefixes. Registering never touches the prefix
/// directory itself.
pub struct PrefixRegistry {
    file: PathBuf,
    inner: Mutex<Vec<PrefixInfo>>,
}

impl PrefixRegistry {
    /// Load the registry. A corrupt file is moved aside (never deleted) and
    /// an empty registry is returned together with a warning.
    pub fn load(file: &Path) -> Result<(Self, Option<String>), AppError> {
        let mut warning = None;
        let prefixes = match std::fs::read_to_string(file) {
            Ok(s) => match serde_json::from_str::<RegistryFile>(&s) {
                Ok(f) => f.prefixes,
                Err(e) => {
                    let aside = file.with_extension(format!("json.corrupt-{}", vnl_core::now_secs()));
                    std::fs::rename(file, &aside)?;
                    warning = Some(format!(
                        "prefix registry {} was unreadable ({e}); moved to {}",
                        file.display(),
                        aside.display()
                    ));
                    Vec::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        Ok((Self { file: file.to_path_buf(), inner: Mutex::new(prefixes) }, warning))
    }

    fn refreshed(mut p: PrefixInfo) -> PrefixInfo {
        p.initialized = is_initialized(&p);
        p
    }

    pub fn list(&self) -> Vec<PrefixInfo> {
        self.inner.lock().unwrap().iter().cloned().map(Self::refreshed).collect()
    }

    pub fn get(&self, id: &str) -> Option<PrefixInfo> {
        self.inner.lock().unwrap().iter().find(|p| p.id == id).cloned().map(Self::refreshed)
    }

    pub fn find_dedicated(&self, game_id: GameId) -> Option<PrefixInfo> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .find(|p| matches!(p.kind, PrefixKind::Dedicated { game_id: g } if g == game_id))
            .cloned()
            .map(Self::refreshed)
    }

    pub fn find_by_path(&self, path: &Path) -> Option<PrefixInfo> {
        let want = canonical(path);
        self.inner.lock().unwrap().iter().find(|p| canonical(&p.path) == want).cloned().map(Self::refreshed)
    }

    pub fn contains_id(&self, id: &str) -> bool {
        self.inner.lock().unwrap().iter().any(|p| p.id == id)
    }

    /// Insert or replace (by id) and persist.
    pub fn upsert(&self, info: PrefixInfo) -> Result<PrefixInfo, AppError> {
        let mut list = self.inner.lock().unwrap();
        let mut stored = info.clone();
        stored.initialized = false; // derived on read
        match list.iter_mut().find(|p| p.id == info.id) {
            Some(existing) => *existing = stored,
            None => list.push(stored),
        }
        let json = serde_json::to_string_pretty(&RegistryFile { version: 1, prefixes: list.clone() })
            .map_err(|e| AppError::internal(e.to_string()))?;
        write_atomic(&self.file, json.as_bytes())?;
        Ok(Self::refreshed(info))
    }

    /// An id derived from `name` not used by any registered prefix nor by an
    /// existing directory under `prefixes_dir`.
    pub fn unique_id(&self, name: &str, prefixes_dir: &Path) -> String {
        let base = crate::util::slugify(name);
        let mut id = base.clone();
        let mut n = 2;
        while self.contains_id(&id) || prefixes_dir.join(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }
}

pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, path: &Path, managed: bool) -> PrefixInfo {
        PrefixInfo {
            id: id.into(),
            name: id.into(),
            path: path.to_path_buf(),
            arch: PrefixArch::Win64,
            kind: PrefixKind::Shared,
            runner_id: None,
            created_at: 1,
            managed,
            initialized: false,
        }
    }

    #[test]
    fn layouts() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = info("m", &tmp.path().join("m"), true);
        assert_eq!(wine_prefix_dir(&managed), tmp.path().join("m/pfx"));
        let plain_dir = tmp.path().join("plain wine ゲーム");
        std::fs::create_dir_all(plain_dir.join("drive_c")).unwrap();
        let plain = info("p", &plain_dir, false);
        assert_eq!(layout(&plain), PrefixLayout::Plain);
        assert_eq!(wine_prefix_dir(&plain), plain_dir);
        assert!(is_initialized(&plain));
        let compat_dir = tmp.path().join("compat");
        std::fs::create_dir_all(compat_dir.join("pfx")).unwrap();
        let compat = info("c", &compat_dir, false);
        assert_eq!(layout(&compat), PrefixLayout::CompatData);
        assert!(!is_initialized(&compat));
        std::fs::write(plain_dir.join("system.reg"), "WINE REGISTRY Version 2\n;; All keys\n\n#arch=win32\n").unwrap();
        assert_eq!(detect_arch(&plain_dir), Some(PrefixArch::Win32));
    }

    #[test]
    fn registry_persists_and_reloads() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("data/prefixes.json");
        let (reg, warn) = PrefixRegistry::load(&file).unwrap();
        assert!(warn.is_none());
        let dir = tmp.path().join("pfxes/a");
        std::fs::create_dir_all(dir.join("pfx/drive_c")).unwrap();
        let mut a = info("a", &dir, true);
        a.kind = PrefixKind::Dedicated { game_id: 7 };
        let stored = reg.upsert(a.clone()).unwrap();
        assert!(stored.initialized);
        reg.upsert(info("b", &tmp.path().join("b"), false)).unwrap();
        drop(reg);

        let (reg, _) = PrefixRegistry::load(&file).unwrap();
        assert_eq!(reg.list().len(), 2);
        assert_eq!(reg.find_dedicated(7).unwrap().id, "a");
        assert!(reg.find_dedicated(8).is_none());
        assert_eq!(reg.find_by_path(&dir).unwrap().id, "a");
        assert!(reg.get("a").unwrap().initialized);
        assert_eq!(reg.unique_id("A", &tmp.path().join("pfxes")), "a-2");
    }

    #[test]
    fn corrupt_registry_is_moved_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("prefixes.json");
        std::fs::write(&file, "{not json").unwrap();
        let (reg, warn) = PrefixRegistry::load(&file).unwrap();
        assert!(warn.is_some());
        assert!(reg.list().is_empty());
        assert!(!file.exists());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }
}
