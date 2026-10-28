use std::path::{Path, PathBuf};

/// All on-disk locations used by VN Launcher.
///
/// Resolution order:
/// 1. `VNL_HOME` env var: everything lives under `$VNL_HOME/{data,config,cache}`
///    (used by tests, portable installs and `make dev`).
/// 2. XDG base directories via `directories` (`~/.local/share/vn-launcher`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppPaths {
    pub const APP_NAME: &'static str = "vn-launcher";

    pub fn from_env() -> std::io::Result<Self> {
        if let Some(home) = std::env::var_os("VNL_HOME").filter(|v| !v.is_empty()) {
            return Ok(Self::under(Path::new(&home)));
        }
        let dirs = directories::ProjectDirs::from("", "", Self::APP_NAME).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "cannot determine home directory")
        })?;
        Ok(Self {
            data_dir: dirs.data_dir().to_path_buf(),
            config_dir: dirs.config_dir().to_path_buf(),
            cache_dir: dirs.cache_dir().to_path_buf(),
        })
    }

    /// Self-contained layout under `root` (tests, portable mode).
    pub fn under(root: &Path) -> Self {
        Self { data_dir: root.join("data"), config_dir: root.join("config"), cache_dir: root.join("cache") }
    }

    /// Create every directory that must exist.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            self.data_dir.clone(),
            self.config_dir.clone(),
            self.cache_dir.clone(),
            self.prefixes_dir(),
            self.covers_dir(),
            self.backups_dir(),
            self.logs_dir(),
            self.translation_dir(),
            self.runners_dir(),
            self.tools_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Main SQLite database (library, settings, playtime, save backups index).
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("library.sqlite3")
    }

    /// Managed Wine/Proton prefixes.
    pub fn prefixes_dir(&self) -> PathBuf {
        self.data_dir.join("prefixes")
    }

    /// Managed (downloaded) runners such as GE-Proton builds.
    pub fn runners_dir(&self) -> PathBuf {
        self.data_dir.join("runners")
    }

    /// Downloaded helper tools (LunaHost, upscaler models, ...).
    pub fn tools_dir(&self) -> PathBuf {
        self.data_dir.join("tools")
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.cache_dir.join("covers")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join("save-backups")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Translation subsystem storage (glossaries, history, provider config).
    pub fn translation_dir(&self) -> PathBuf {
        self.data_dir.join("translation")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_creates_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(tmp.path());
        paths.ensure().unwrap();
        assert!(paths.prefixes_dir().is_dir());
        assert!(paths.covers_dir().starts_with(tmp.path()));
        assert_eq!(paths.db_path().parent().unwrap(), paths.data_dir);
    }
}
