//! Runner discovery (system Wine, Proton builds, managed and custom runners).
//!
//! Every discovery function takes explicit root paths so tests can point it at
//! fake directory trees. [`DiscoveryRoots::from_system`] builds the real roots.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vnl_core::{AppError, AppPaths, ErrorCode, Runner, RunnerKind, RunnerSource};

pub const NATIVE_RUNNER_ID: &str = "native";
pub const SYSTEM_WINE_ID: &str = "wine:system";

/// A user-added runner, persisted in `config_dir/runners.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomRunner {
    pub id: String,
    pub name: String,
    pub kind: RunnerKind,
    /// `wine` binary (Wine) or Proton root directory (Proton).
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct CustomRunnersFile {
    runners: Vec<CustomRunner>,
}

/// Where to look for runners.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryRoots {
    /// System `wine` binary (usually from `which wine`).
    pub system_wine: Option<PathBuf>,
    /// Steam `compatibilitytools.d` directories (each subdir may be a Proton build).
    pub compat_tool_dirs: Vec<PathBuf>,
    /// Steam installation roots; `steamapps/common/Proton*` and the libraries
    /// listed in `steamapps/libraryfolders.vdf` are scanned.
    pub steam_roots: Vec<PathBuf>,
    /// Runners managed by VN Launcher (`AppPaths::runners_dir`).
    pub managed_dir: Option<PathBuf>,
    /// `runners.json` with user-added runners.
    pub custom_file: Option<PathBuf>,
}

impl DiscoveryRoots {
    /// Real locations on this machine.
    pub fn from_system(paths: &AppPaths) -> Self {
        let mut compat = Vec::new();
        let mut steam = Vec::new();
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            compat.push(home.join(".local/share/Steam/compatibilitytools.d"));
            compat.push(home.join(".steam/root/compatibilitytools.d"));
            compat.push(home.join(".steam/steam/compatibilitytools.d"));
            compat.push(home.join(".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d"));
            steam.push(home.join(".local/share/Steam"));
            steam.push(home.join(".steam/root"));
            steam.push(home.join(".steam/steam"));
            steam.push(home.join(".var/app/com.valvesoftware.Steam/data/Steam"));
        }
        compat.push(PathBuf::from("/usr/share/steam/compatibilitytools.d"));
        Self {
            system_wine: which::which("wine").ok(),
            compat_tool_dirs: compat,
            steam_roots: steam,
            managed_dir: Some(paths.runners_dir()),
            custom_file: Some(custom_runners_file(paths)),
        }
    }
}

pub fn custom_runners_file(paths: &AppPaths) -> PathBuf {
    paths.config_dir.join("runners.json")
}

pub fn native_runner() -> Runner {
    Runner {
        id: NATIVE_RUNNER_ID.into(),
        name: "Native (Linux)".into(),
        kind: RunnerKind::Native,
        source: RunnerSource::System,
        path: PathBuf::new(),
        version: None,
    }
}

/// Discover every runner. Deduplicated by canonical path; the `native` runner is always last.
pub fn discover(roots: &DiscoveryRoots) -> Vec<Runner> {
    let mut found = Found::default();

    if let Some(wine) = &roots.system_wine
        && wine.is_file()
    {
        found.push(
            Runner {
                id: SYSTEM_WINE_ID.into(),
                name: "System Wine".into(),
                kind: RunnerKind::Wine,
                source: RunnerSource::System,
                path: wine.clone(),
                version: wine_version(wine),
            },
            wine,
        );
    }

    if let Some(managed) = &roots.managed_dir {
        for dir in sorted_subdirs(managed) {
            if let Some(runner) = proton_runner(&dir, RunnerSource::Managed) {
                found.push(runner, &dir);
            } else if dir.join("bin/wine").is_file() {
                let wine = dir.join("bin/wine");
                let name = dir_name(&dir);
                found.push(
                    Runner {
                        id: format!("wine:{name}"),
                        name: name.clone(),
                        kind: RunnerKind::Wine,
                        source: RunnerSource::Managed,
                        version: wine_version(&wine),
                        path: wine.clone(),
                    },
                    &wine,
                );
            }
        }
    }

    for tools in &roots.compat_tool_dirs {
        for dir in sorted_subdirs(tools) {
            if let Some(runner) = proton_runner(&dir, RunnerSource::Steam) {
                found.push(runner, &dir);
            }
        }
    }

    for common in steam_common_dirs(&roots.steam_roots) {
        for dir in sorted_subdirs(&common) {
            if !dir_name(&dir).starts_with("Proton") {
                continue;
            }
            if let Some(runner) = proton_runner(&dir, RunnerSource::Steam) {
                found.push(runner, &dir);
            }
        }
    }

    if let Some(file) = &roots.custom_file {
        for custom in load_custom_runners(file).unwrap_or_default() {
            let version = match custom.kind {
                RunnerKind::Proton => read_proton_version(&custom.path),
                RunnerKind::Wine => wine_version(&custom.path),
                RunnerKind::Native => None,
            };
            let path = custom.path.clone();
            found.push(
                Runner {
                    id: custom.id,
                    name: custom.name,
                    kind: custom.kind,
                    source: RunnerSource::Custom,
                    path: custom.path,
                    version,
                },
                &path,
            );
        }
    }

    let mut runners = found.runners;
    runners.push(native_runner());
    runners
}

#[derive(Default)]
struct Found {
    runners: Vec<Runner>,
    paths: HashSet<PathBuf>,
    ids: HashSet<String>,
}

impl Found {
    fn push(&mut self, mut runner: Runner, path: &Path) {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !self.paths.insert(canonical) {
            return;
        }
        if self.ids.contains(&runner.id) {
            let base = runner.id.clone();
            let mut n = 2;
            while self.ids.contains(&format!("{base}#{n}")) {
                n += 1;
            }
            runner.id = format!("{base}#{n}");
        }
        self.ids.insert(runner.id.clone());
        self.runners.push(runner);
    }
}

fn sorted_subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

fn dir_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `steamapps/common` of every Steam root and every library in `libraryfolders.vdf`.
fn steam_common_dirs(steam_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |p: PathBuf, out: &mut Vec<PathBuf>| {
        let c = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        if p.is_dir() && seen.insert(c) {
            out.push(p);
        }
    };
    for root in steam_roots {
        add(root.join("steamapps/common"), &mut out);
        if let Ok(vdf) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            for lib in parse_library_folders(&vdf) {
                add(lib.join("steamapps/common"), &mut out);
            }
        }
    }
    out
}

/// Extracts `"path" "..."` values from Steam's `libraryfolders.vdf`.
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let mut parts = vdf_quoted(line);
            match (parts.next(), parts.next()) {
                (Some(k), Some(v)) if k == "path" => Some(PathBuf::from(v.replace("\\\\", "\\"))),
                _ => None,
            }
        })
        .collect()
}

/// Quoted tokens of one VDF line (comments after `//` are ignored).
fn vdf_quoted(line: &str) -> impl Iterator<Item = String> + '_ {
    let line = line.trim();
    let mut chars = line.char_indices().peekable();
    std::iter::from_fn(move || {
        loop {
            let (_, c) = chars.next()?;
            if c == '/' && chars.peek().map(|(_, c)| *c) == Some('/') {
                return None;
            }
            if c == '"' {
                let mut s = String::new();
                let mut escaped = false;
                for (_, c) in chars.by_ref() {
                    if escaped {
                        s.push(c);
                        escaped = false;
                    } else if c == '\\' {
                        s.push(c);
                        escaped = true;
                    } else if c == '"' {
                        return Some(s);
                    } else {
                        s.push(c);
                    }
                }
                return Some(s);
            }
        }
    })
}

/// A Proton build is a directory containing the `proton` script.
pub fn proton_runner(dir: &Path, source: RunnerSource) -> Option<Runner> {
    if !dir.join("proton").is_file() {
        return None;
    }
    let dirname = dir_name(dir);
    let name = read_display_name(dir).unwrap_or_else(|| dirname.clone());
    Some(Runner {
        id: format!("proton:{dirname}"),
        name,
        kind: RunnerKind::Proton,
        source,
        path: dir.to_path_buf(),
        version: read_proton_version(dir),
    })
}

/// Proton's `version` file: `"<build timestamp> <version name>"`.
pub fn read_proton_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("version")).ok()?;
    let line = content.lines().next()?.trim();
    let mut parts = line.split_whitespace();
    let first = parts.next()?;
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() { Some(first.to_string()) } else { Some(rest.join(" ")) }
}

fn read_display_name(dir: &Path) -> Option<String> {
    let vdf = std::fs::read_to_string(dir.join("compatibilitytool.vdf")).ok()?;
    vdf.lines().find_map(|line| {
        let mut parts = vdf_quoted(line);
        match (parts.next(), parts.next()) {
            (Some(k), Some(v)) if k == "display_name" && !v.is_empty() => Some(v),
            _ => None,
        }
    })
}

/// Runs `<wine> --version` (does not start a wineserver).
pub fn wine_version(wine: &Path) -> Option<String> {
    let out = std::process::Command::new(wine)
        .arg("--version")
        .env("WINEDEBUG", "-all")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_wine_version(&String::from_utf8_lossy(&out.stdout))
}

/// `"wine-11.20 (Staging)"` -> `"11.20 (Staging)"`.
pub fn parse_wine_version(output: &str) -> Option<String> {
    let line = output.lines().map(str::trim).find(|l| !l.is_empty())?;
    let v = line.strip_prefix("wine-").unwrap_or(line).trim();
    if v.is_empty() { None } else { Some(v.to_string()) }
}

/// Wine binary used by a runner (Proton: its bundled `files/bin/wine`).
pub fn wine_binary(runner: &Runner) -> Option<PathBuf> {
    match runner.kind {
        RunnerKind::Wine => Some(runner.path.clone()),
        RunnerKind::Proton => {
            ["files/bin/wine", "dist/bin/wine"].iter().map(|p| runner.path.join(p)).find(|p| p.is_file())
        }
        RunnerKind::Native => None,
    }
}

/// `wineserver` matching a runner's Wine build.
pub fn wineserver_binary(runner: &Runner) -> Option<PathBuf> {
    match runner.kind {
        RunnerKind::Wine => {
            let sibling = runner.path.parent().map(|p| p.join("wineserver"));
            match sibling {
                Some(s) if s.is_file() => Some(s),
                _ => which::which("wineserver").ok(),
            }
        }
        RunnerKind::Proton => {
            ["files/bin/wineserver", "dist/bin/wineserver"].iter().map(|p| runner.path.join(p)).find(|p| p.is_file())
        }
        RunnerKind::Native => None,
    }
}

/// Numeric components of a version string, used for "newest" comparisons.
pub fn version_key(v: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in v.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(cur.parse().unwrap_or(0));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.parse().unwrap_or(0));
    }
    out
}

fn is_ge_proton(r: &Runner) -> bool {
    let hay = format!("{} {} {}", r.name, dir_name(&r.path), r.version.as_deref().unwrap_or("")).to_lowercase();
    r.kind == RunnerKind::Proton && (hay.contains("ge-proton") || hay.contains("proton-ge"))
}

fn newest<'a>(candidates: impl Iterator<Item = &'a Runner>) -> Option<&'a Runner> {
    let mut list: Vec<&Runner> = candidates.collect();
    // Highest version first; ties broken by id for determinism.
    list.sort_by(|a, b| {
        let ka = version_key(a.version.as_deref().unwrap_or(&a.name));
        let kb = version_key(b.version.as_deref().unwrap_or(&b.name));
        kb.cmp(&ka).then_with(|| a.id.cmp(&b.id))
    });
    list.into_iter().next()
}

/// Default choice: newest GE-Proton, then system Wine, then any Proton, then any Wine.
pub fn best_runner(runners: &[Runner]) -> Option<&Runner> {
    newest(runners.iter().filter(|r| is_ge_proton(r)))
        .or_else(|| runners.iter().find(|r| r.id == SYSTEM_WINE_ID))
        .or_else(|| newest(runners.iter().filter(|r| r.kind == RunnerKind::Proton)))
        .or_else(|| runners.iter().find(|r| r.kind == RunnerKind::Wine))
}

pub fn load_custom_runners(file: &Path) -> Result<Vec<CustomRunner>, AppError> {
    match std::fs::read_to_string(file) {
        Ok(s) => serde_json::from_str::<CustomRunnersFile>(&s)
            .map(|f| f.runners)
            .map_err(|e| AppError::new(ErrorCode::Io, format!("invalid {}", file.display())).with_details(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

pub fn save_custom_runners(file: &Path, runners: &[CustomRunner]) -> Result<(), AppError> {
    let json = serde_json::to_string_pretty(&CustomRunnersFile { runners: runners.to_vec() })
        .map_err(|e| AppError::internal(e.to_string()))?;
    crate::util::write_atomic(file, json.as_bytes())
}

/// Validate a user-supplied runner path and build its [`CustomRunner`] entry.
/// Accepts a Proton directory, a Wine build directory (`bin/wine`) or a `wine` binary.
pub fn inspect_custom_runner(path: &Path, name: Option<&str>, existing: &[CustomRunner]) -> Result<CustomRunner, AppError> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| AppError::not_found(format!("runner path not found: {}", path.display())).with_details(e.to_string()))?;
    let (kind, runner_path) = if canonical.is_dir() && canonical.join("proton").is_file() {
        (RunnerKind::Proton, canonical.clone())
    } else if canonical.is_dir() && canonical.join("bin/wine").is_file() {
        (RunnerKind::Wine, canonical.join("bin/wine"))
    } else if canonical.is_file() && is_executable(&canonical) {
        (RunnerKind::Wine, canonical.clone())
    } else {
        return Err(AppError::invalid(format!(
            "{} is not a Proton directory, a Wine build directory or a wine executable",
            path.display()
        )));
    };
    let default_name = if kind == RunnerKind::Proton || canonical.is_dir() {
        dir_name(&canonical)
    } else {
        canonical.parent().and_then(|p| p.parent()).map(dir_name).unwrap_or_else(|| dir_name(&canonical))
    };
    let name = name.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or(default_name);
    let base = format!("custom:{}", crate::util::slugify(&name));
    let mut id = base.clone();
    let mut n = 2;
    while existing.iter().any(|r| r.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    Ok(CustomRunner { id, name, kind, path: runner_path })
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_proton(root: &Path, name: &str, version: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("files/bin")).unwrap();
        std::fs::write(dir.join("proton"), "#!/usr/bin/env python3\n").unwrap();
        std::fs::write(dir.join("version"), format!("1792432585 {version}\n")).unwrap();
        std::fs::write(dir.join("files/bin/wine"), "").unwrap();
        std::fs::write(dir.join("files/bin/wineserver"), "").unwrap();
        dir
    }

    fn fake_wine(path: &Path, version: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\necho {version}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn discovers_fake_tree_and_dedupes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let wine = root.join("usr/bin/wine");
        fake_wine(&wine, "wine-11.20");
        let compat = root.join("steam/compatibilitytools.d");
        fake_proton(&compat, "GE-Proton9-27", "GE-Proton9-27");
        fake_proton(&compat, "GE-Proton11-11-x86_64", "GE-Proton11-11");
        std::fs::create_dir_all(compat.join("not-a-runner")).unwrap();
        // Same directory reachable through a symlink (like ~/.steam/root).
        std::os::unix::fs::symlink(root.join("steam"), root.join("steam-link")).unwrap();
        let steam_root = root.join("steamroot");
        fake_proton(&steam_root.join("steamapps/common"), "Proton - Experimental", "experimental-11.0");
        std::fs::create_dir_all(steam_root.join("steamapps/common/Some Game")).unwrap();
        let lib2 = root.join("lib2");
        fake_proton(&lib2.join("steamapps/common"), "Proton 9.0", "9.0-4");
        std::fs::write(
            steam_root.join("steamapps/libraryfolders.vdf"),
            format!("\"libraryfolders\"\n{{\n \"1\"\n {{\n  \"path\"\t\t\"{}\"\n }}\n}}\n", lib2.display()),
        )
        .unwrap();
        let managed = root.join("managed");
        fake_proton(&managed, "GE-Proton10-1", "GE-Proton10-1");

        let roots = DiscoveryRoots {
            system_wine: Some(wine.clone()),
            compat_tool_dirs: vec![compat.clone(), root.join("steam-link/compatibilitytools.d"), root.join("missing")],
            steam_roots: vec![steam_root],
            managed_dir: Some(managed),
            custom_file: Some(root.join("runners.json")),
        };
        let runners = discover(&roots);
        let ids: Vec<&str> = runners.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "wine:system",
                "proton:GE-Proton10-1",
                "proton:GE-Proton11-11-x86_64",
                "proton:GE-Proton9-27",
                "proton:Proton - Experimental",
                "proton:Proton 9.0",
                "native"
            ]
        );
        let sys = &runners[0];
        assert_eq!(sys.version.as_deref(), Some("11.20"));
        assert_eq!(sys.kind, RunnerKind::Wine);
        let ge = runners.iter().find(|r| r.id == "proton:GE-Proton11-11-x86_64").unwrap();
        assert_eq!(ge.version.as_deref(), Some("GE-Proton11-11"));
        assert_eq!(ge.source, RunnerSource::Steam);
        assert_eq!(runners[1].source, RunnerSource::Managed);
        assert_eq!(runners.last().unwrap().kind, RunnerKind::Native);

        assert_eq!(best_runner(&runners).unwrap().id, "proton:GE-Proton11-11-x86_64");
        let without_ge: Vec<Runner> = runners.iter().filter(|r| !is_ge_proton(r)).cloned().collect();
        assert_eq!(best_runner(&without_ge).unwrap().id, "wine:system");
        let only_native = vec![native_runner()];
        assert!(best_runner(&only_native).is_none());
    }

    #[test]
    fn custom_runner_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let proton = fake_proton(tmp.path(), "My Proton ゲーム", "custom-1");
        let wine_build = tmp.path().join("wine-tkg");
        fake_wine(&wine_build.join("bin/wine"), "wine-10.0");
        let file = tmp.path().join("cfg/runners.json");

        let a = inspect_custom_runner(&proton, None, &[]).unwrap();
        assert_eq!(a.kind, RunnerKind::Proton);
        assert_eq!(a.name, "My Proton ゲーム");
        let b = inspect_custom_runner(&wine_build, Some("TKG"), std::slice::from_ref(&a)).unwrap();
        assert_eq!(b.kind, RunnerKind::Wine);
        assert_eq!(b.path, std::fs::canonicalize(&wine_build).unwrap().join("bin/wine"));
        assert_eq!(b.id, "custom:tkg");
        let c = inspect_custom_runner(&wine_build.join("bin/wine"), Some("TKG"), &[a.clone(), b.clone()]).unwrap();
        assert_eq!(c.id, "custom:tkg-2");
        assert!(inspect_custom_runner(tmp.path(), None, &[]).is_err());

        save_custom_runners(&file, &[a.clone(), b.clone()]).unwrap();
        assert_eq!(load_custom_runners(&file).unwrap(), vec![a, b]);
        let runners = discover(&DiscoveryRoots { custom_file: Some(file), ..Default::default() });
        assert_eq!(runners.len(), 3);
        assert_eq!(runners[0].source, RunnerSource::Custom);
        assert_eq!(runners[0].version.as_deref(), Some("custom-1"));
        assert_eq!(runners[1].version.as_deref(), Some("10.0"));
    }

    #[test]
    fn parses_versions() {
        assert_eq!(parse_wine_version("wine-11.20 (Staging)\n").as_deref(), Some("11.20 (Staging)"));
        assert_eq!(parse_wine_version("").as_deref(), None);
        assert_eq!(version_key("GE-Proton11-11"), vec![11, 11]);
        assert!(version_key("GE-Proton11-11") > version_key("GE-Proton9-27"));
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("version"), "1793209673 TKG-proton.11.0\n").unwrap();
        assert_eq!(read_proton_version(tmp.path()).as_deref(), Some("TKG-proton.11.0"));
        std::fs::write(tmp.path().join("version"), "9.0-1\n").unwrap();
        assert_eq!(read_proton_version(tmp.path()).as_deref(), Some("9.0-1"));
    }

    #[test]
    fn reads_display_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = fake_proton(tmp.path(), "Proton-GE Latest", "GE-Proton11-11");
        std::fs::write(
            dir.join("compatibilitytool.vdf"),
            "\"compatibilitytools\"\n{\n  // \"display_name\" \"commented\"\n  \"display_name\" \"Proton-GE (latest)\" // x\n}\n",
        )
        .unwrap();
        assert_eq!(proton_runner(&dir, RunnerSource::Steam).unwrap().name, "Proton-GE (latest)");
    }
}
