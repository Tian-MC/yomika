//! Discovery and selection of gamescope binaries.
//!
//! Several binaries can coexist: the distro's `gamescope`, a user-chosen
//! custom one, and the private build of the gamescope-anime4k fork that the
//! launcher can compile on request ([`crate::gamescope_build`]). Only the fork
//! understands `-F anime4k`, so each binary is inspected (`--help`) for the
//! upscaler filters it really supports and [`select`] picks one per game.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vnl_core::{AppError, AppPaths, ErrorCode, GamescopeFilter};

use crate::gamescope::filter_value;
use crate::probe::{Probe, ToolInfo, extract_version, is_executable};

/// Directory (below `AppPaths::tools_dir`) holding the managed fork build.
pub const MANAGED_DIR_NAME: &str = "gamescope-anime4k";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GamescopeSource {
    /// Per-game custom path (`GamescopeConfig::binary_path`).
    Game,
    /// Application-wide custom path (`AppSettings::gamescope_path`).
    Custom,
    /// `PATH` or the Flatpak extension.
    System,
    /// Built by the launcher into its private directory.
    Managed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamescopeBinary {
    pub path: PathBuf,
    pub version: Option<String>,
    pub source: GamescopeSource,
    /// Values accepted by `-F`, parsed from `--help`. Empty when unknown.
    pub filters: Vec<String>,
    pub supports_anime4k: bool,
}

impl GamescopeBinary {
    pub fn supports_filter(&self, filter: GamescopeFilter) -> bool {
        // Unknown list (help could not be parsed): do not reject.
        self.filters.is_empty() || self.filters.iter().any(|f| f == filter_value(filter))
    }

    pub fn tool_info(&self) -> ToolInfo {
        ToolInfo { path: self.path.clone(), version: self.version.clone() }
    }
}

/// Parse the filter names from the `-F, --filter` line of `gamescope --help`:
/// `-F, --filter   upscaler filter (linear, nearest, fsr, nis, pixel, anime4k)`.
pub fn parse_filters(help: &str) -> Vec<String> {
    let Some(line) = help.lines().find(|l| l.contains("--filter")) else { return Vec::new() };
    let (Some(open), Some(close)) = (line.find('('), line.rfind(')')) else { return Vec::new() };
    if close <= open {
        return Vec::new();
    }
    line[open + 1..close]
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()))
        .collect()
}

/// `gamescope version 3.17.0 (gcc 16)` -> `3.17.0`; a build from git prints a
/// commit (`gamescope version 863ff3b (gcc 16)`), which is returned as is.
pub fn parse_version(text: &str) -> Option<String> {
    const MARK: &str = "gamescope version ";
    if let Some(i) = text.find(MARK) {
        let token = text[i + MARK.len()..].split_whitespace().next()?;
        if !token.starts_with('(') {
            return Some(token.to_string());
        }
    }
    extract_version(text)
}

/// Inspect one candidate binary. `None` when it is missing, not executable or
/// does not look like gamescope.
pub fn inspect(probe: &Probe, path: &Path, source: GamescopeSource) -> Option<GamescopeBinary> {
    if !is_executable(path) {
        return None;
    }
    if !probe.run_commands {
        // Cannot execute helpers (fixture probes): accept it, capabilities unknown.
        return Some(GamescopeBinary {
            path: path.to_path_buf(),
            version: None,
            source,
            filters: Vec::new(),
            supports_anime4k: false,
        });
    }
    let help = probe.run(path, &["--help"])?.combined();
    if !help.to_ascii_lowercase().contains("gamescope") {
        return None;
    }
    let filters = parse_filters(&help);
    let version = probe.run(path, &["--version"]).and_then(|o| parse_version(&o.combined()));
    let supports_anime4k = filters.iter().any(|f| f == "anime4k");
    Some(GamescopeBinary { path: path.to_path_buf(), version, source, filters, supports_anime4k })
}

pub fn managed_dir(paths: &AppPaths) -> PathBuf {
    paths.tools_dir().join(MANAGED_DIR_NAME)
}

pub fn managed_binary(paths: &AppPaths) -> PathBuf {
    managed_dir(paths).join("bin/gamescope")
}

/// Every usable gamescope binary, in precedence order: per-game custom,
/// application custom, system, managed. Duplicates (same real file) are dropped.
pub fn discover(
    probe: &Probe,
    paths: &AppPaths,
    game_path: Option<&Path>,
    app_path: Option<&Path>,
) -> Vec<GamescopeBinary> {
    let mut out: Vec<GamescopeBinary> = Vec::new();
    let mut push = |candidate: Option<GamescopeBinary>| {
        let Some(bin) = candidate else { return };
        let real = std::fs::canonicalize(&bin.path).unwrap_or_else(|_| bin.path.clone());
        if !out.iter().any(|b| std::fs::canonicalize(&b.path).unwrap_or_else(|_| b.path.clone()) == real) {
            out.push(bin);
        }
    };
    if let Some(p) = game_path {
        push(inspect(probe, p, GamescopeSource::Game));
    }
    if let Some(p) = app_path {
        push(inspect(probe, p, GamescopeSource::Custom));
    }
    if let Some(p) = crate::display::find_gamescope(probe) {
        push(inspect(probe, &p, GamescopeSource::System));
    }
    push(inspect(probe, &managed_binary(paths), GamescopeSource::Managed));
    out
}

pub const BUILD_HINT: &str = "Build the Anime4K-enabled gamescope fork from Settings > Gamescope, or choose a custom gamescope binary that supports `-F anime4k`.";

/// Pick the binary for `filter`. A binary that does not list the filter is
/// skipped. For `anime4k` there is no fallback: using a standard gamescope
/// would make it exit with an error, so a clear one is returned instead.
pub fn select(
    binaries: &[GamescopeBinary],
    filter: GamescopeFilter,
) -> Result<(&GamescopeBinary, Vec<String>), AppError> {
    let mut warnings = Vec::new();
    // The managed fork is meant for Anime4K, so it ranks above the system
    // gamescope only when that filter is requested.
    let ordered: Vec<&GamescopeBinary> = if filter == GamescopeFilter::Anime4k {
        let mut v: Vec<&GamescopeBinary> = binaries.iter().collect();
        v.sort_by_key(|b| match b.source {
            GamescopeSource::Game => 0,
            GamescopeSource::Custom => 1,
            GamescopeSource::Managed => 2,
            GamescopeSource::System => 3,
        });
        v
    } else {
        binaries.iter().collect()
    };
    if let Some(bin) = ordered.iter().find(|b| b.supports_filter(filter)) {
        return Ok((bin, warnings));
    }
    if filter == GamescopeFilter::Anime4k {
        let have = binaries.iter().map(|b| b.path.display().to_string()).collect::<Vec<_>>().join(", ");
        return Err(AppError::new(
            ErrorCode::MissingDependency,
            "The Anime4K filter needs a gamescope build with `-F anime4k`; none was found",
        )
        .with_details(if have.is_empty() {
            BUILD_HINT.to_string()
        } else {
            format!("{BUILD_HINT} Checked: {have}")
        }));
    }
    match binaries.first() {
        Some(first) => {
            warnings.push(format!(
                "None of the available gamescope binaries lists the '{}' filter; using {} anyway.",
                filter_value(filter),
                first.path.display()
            ));
            Ok((first, warnings))
        }
        None => Err(AppError::new(ErrorCode::MissingDependency, "gamescope is enabled but not installed")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fake_gamescope;
    use std::os::unix::fs::PermissionsExt;

    fn bin(source: GamescopeSource, filters: &[&str], path: &str) -> GamescopeBinary {
        GamescopeBinary {
            path: path.into(),
            version: None,
            source,
            filters: filters.iter().map(|s| s.to_string()).collect(),
            supports_anime4k: filters.contains(&"anime4k"),
        }
    }

    #[test]
    fn parses_filter_list_from_help() {
        let help = "  -F, --filter                   upscaler filter (linear, nearest, fsr, nis, pixel, anime4k)\n     fsr => AMD";
        assert_eq!(parse_filters(help), ["linear", "nearest", "fsr", "nis", "pixel", "anime4k"]);
        assert!(parse_filters("no such line").is_empty());
        assert_eq!(parse_filters("  -F, --filter  upscaler filter (linear, sgsr)")[1], "sgsr");
    }

    #[test]
    fn parses_release_and_git_versions() {
        assert_eq!(
            parse_version("[gamescope] [\u{1b}[0;34mInfo\u{1b}[0m]  console: gamescope version 3.17.0 (gcc 16.2.1)")
                .as_deref(),
            Some("3.17.0")
        );
        assert_eq!(parse_version("gamescope version 863ff3b (gcc 16.2.1)").as_deref(), Some("863ff3b"));
        assert_eq!(parse_version("something 1.2.3").as_deref(), Some("1.2.3"));
    }

    #[test]
    fn inspects_real_executables_only() {
        let tmp = tempfile::tempdir().unwrap();
        let probe = Probe::fixture(tmp.path(), Default::default());
        let fork = fake_gamescope(tmp.path(), "gs-fork", "linear, fsr, anime4k");
        let b = inspect(&probe, &fork, GamescopeSource::Custom).unwrap();
        assert!(b.supports_anime4k);
        assert_eq!(b.version.as_deref(), Some("3.17.0"));
        assert!(inspect(&probe, &tmp.path().join("missing"), GamescopeSource::Custom).is_none());
        let not_gs = tmp.path().join("other");
        std::fs::write(&not_gs, "#!/bin/sh\necho hello\n").unwrap();
        std::fs::set_permissions(&not_gs, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(inspect(&probe, &not_gs, GamescopeSource::Custom).is_none());
    }

    #[test]
    fn anime4k_prefers_fork_and_never_falls_back() {
        let sys = bin(GamescopeSource::System, &["linear", "fsr", "sgsr"], "/usr/bin/gamescope");
        let managed = bin(GamescopeSource::Managed, &["linear", "fsr", "anime4k"], "/m/gamescope");
        let all = [sys.clone(), managed.clone()];
        assert_eq!(select(&all, GamescopeFilter::Anime4k).unwrap().0.source, GamescopeSource::Managed);
        // Standard filters keep using the system binary.
        assert_eq!(select(&all, GamescopeFilter::Fsr).unwrap().0.source, GamescopeSource::System);
        let err = select(&[sys], GamescopeFilter::Anime4k).unwrap_err();
        assert_eq!(err.code, ErrorCode::MissingDependency);
        assert!(err.details.unwrap().contains("Settings > Gamescope"));
    }

    #[test]
    fn standard_filters_skip_binaries_lacking_them_and_custom_wins() {
        let fork = bin(GamescopeSource::Custom, &["linear", "fsr", "anime4k"], "/c/gs");
        let sys = bin(GamescopeSource::System, &["linear", "fsr", "sgsr"], "/usr/bin/gamescope");
        let all = [fork.clone(), sys.clone()];
        assert_eq!(select(&all, GamescopeFilter::Fsr).unwrap().0.source, GamescopeSource::Custom);
        // sgsr only exists in the system build.
        assert_eq!(select(&all, GamescopeFilter::Sgsr).unwrap().0.source, GamescopeSource::System);
        // Nothing lists it: warn but keep going.
        let only = [fork];
        let (b, w) = select(&only, GamescopeFilter::Sgsr).unwrap();
        assert_eq!(b.source, GamescopeSource::Custom);
        assert_eq!(w.len(), 1);
        assert!(select(&[], GamescopeFilter::Linear).is_err());
    }

    #[test]
    fn discover_orders_and_dedupes() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(tmp.path());
        paths.ensure().unwrap();
        let managed = managed_binary(&paths);
        std::fs::create_dir_all(managed.parent().unwrap()).unwrap();
        std::fs::copy(fake_gamescope(tmp.path(), "tmp-gs", "linear, anime4k"), &managed).unwrap();
        let custom = fake_gamescope(tmp.path(), "custom-gs", "linear, fsr");
        let probe = Probe::fixture(tmp.path(), Default::default());
        let found = discover(&probe, &paths, None, Some(&custom));
        let sources: Vec<_> = found.iter().map(|b| b.source).collect();
        assert_eq!(sources, [GamescopeSource::Custom, GamescopeSource::Managed]);
        // Same file as game and app custom path: listed once.
        let dup = discover(&probe, &paths, Some(&custom), Some(&custom));
        assert_eq!(dup.iter().filter(|b| b.path == custom).count(), 1);
        assert_eq!(dup[0].source, GamescopeSource::Game);
    }
}
