//! Import scanning: find the most likely game executable in a folder, detect
//! installers and suggest a clean title. Read-only: never modifies files.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use vnl_core::AppError;

use crate::text::compact;
use crate::types::{ExeCandidate, ImportScan};

/// Maximum directory depth below the scan root (root = 0).
pub const MAX_DEPTH: usize = 4;
/// Stop walking after this many directory entries (protects against `/`).
pub const MAX_ENTRIES: usize = 50_000;
/// Bytes read from the start of an executable to look for installer markers.
const SIGNATURE_BYTES: u64 = 1024 * 1024;
const MAX_SIGNATURE_CHECKS: usize = 64;

/// Folder names too generic to be a title (`Game/bin/x64/Game.exe`).
const GENERIC_DIRS: &[&str] =
    &["bin", "bin32", "bin64", "x64", "x86", "win32", "win64", "game", "data", "app", "release", "windows", "exe"];

#[derive(Default)]
struct DirListing {
    files: Vec<String>,
    dirs: Vec<String>,
}

impl DirListing {
    fn has_file(&self, name: &str) -> bool {
        self.files.iter().any(|f| f == name)
    }
    fn has_dir(&self, name: &str) -> bool {
        self.dirs.iter().any(|d| d == name)
    }
    fn any_ext(&self, exts: &[&str]) -> bool {
        self.files.iter().any(|f| exts.iter().any(|e| f.ends_with(e)))
    }
}

/// Engines recognised from the files next to an executable.
fn detect_engines(dir: &DirListing) -> Vec<&'static str> {
    let mut engines = Vec::new();
    if dir.any_ext(&[".xp3"]) {
        engines.push("kirikiri");
    }
    if dir.files.iter().any(|f| f.starts_with("siglus")) || dir.has_file("scene.pck") || dir.has_file("gameexe.dat") {
        engines.push("siglus");
    }
    if dir.has_file("bgi.exe") || (dir.any_ext(&[".arc"]) && dir.has_file("bgi.gdb")) {
        engines.push("bgi");
    }
    if dir.has_file("gameexe.ini") || dir.has_file("reallive.exe") || dir.has_file("seen.txt") {
        engines.push("reallive");
    }
    if dir.has_file("unityplayer.dll") || dir.dirs.iter().any(|d| d.ends_with("_data")) {
        engines.push("unity");
    }
    if dir.any_ext(&[".rgssad", ".rgss2a", ".rgss3a"])
        || (dir.has_file("game.ini") && dir.files.iter().any(|f| f.starts_with("rgss") && f.ends_with(".dll")))
        || (dir.has_file("nw.dll") && (dir.has_dir("www") || dir.has_file("package.json")))
    {
        engines.push("rpgmaker");
    }
    if dir.has_dir("renpy") {
        engines.push("renpy");
    }
    if dir.any_ext(&[".pfs"]) {
        engines.push("artemis");
    }
    if dir.any_ext(&[".ypf"]) {
        engines.push("yuris");
    }
    if dir.has_file("nscript.dat") || dir.any_ext(&[".nsa", ".sar"]) {
        engines.push("nscripter");
    }
    if dir.has_file("cs2.exe") || dir.any_ext(&[".int"]) && dir.has_file("cs2conf.dll") {
        engines.push("catsystem2");
    }
    engines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Uninstaller,
    Redistributable,
    CrashReporter,
    TranslatorTool,
    Installer,
    ConfigTool,
    Helper,
}

impl Role {
    fn hint(self) -> &'static str {
        match self {
            Role::Uninstaller => "uninstaller",
            Role::Redistributable => "redistributable",
            Role::CrashReporter => "crashReporter",
            Role::TranslatorTool => "translatorTool",
            Role::Installer => "installer",
            Role::ConfigTool => "configTool",
            Role::Helper => "helperTool",
        }
    }
    fn penalty(self) -> i32 {
        match self {
            Role::Uninstaller | Role::Redistributable => -100,
            Role::CrashReporter | Role::TranslatorTool => -80,
            Role::Installer => -60,
            Role::ConfigTool => -40,
            Role::Helper => -30,
        }
    }
}

const REDIST_DIRS: &[&str] =
    &["redist", "_commonredist", "commonredist", "directx", "vcredist", "redistributables", "__installer", "dotnet"];

/// Classify a non-game helper executable by its (lowercased) stem and the
/// folders between the scan root and the file.
fn classify(stem: &str, rel_dirs: &[String]) -> Option<Role> {
    let has = |needles: &[&str]| needles.iter().any(|n| stem.contains(n));
    if stem.starts_with("unins") || has(&["uninstall", "アンインストール"]) {
        return Some(Role::Uninstaller);
    }
    if has(&[
        "vcredist",
        "vc_redist",
        "dxsetup",
        "dxwebsetup",
        "directx",
        "dotnetfx",
        "ndp4",
        "oalinst",
        "physx",
        "xnafx",
        "windowsdesktop-runtime",
    ]) || rel_dirs.iter().any(|d| REDIST_DIRS.contains(&d.as_str()))
    {
        return Some(Role::Redistributable);
    }
    if has(&["crash", "bugreport", "errorreport", "bugsplat"]) {
        return Some(Role::CrashReporter);
    }
    if has(&[
        "lunahook",
        "lunatranslator",
        "lunahost",
        "textractor",
        "ithvnr",
        "vnrhook",
        "visualnovelreader",
        "agth",
        "leproc",
        "ntleas",
        "localeemulator",
        "applocale",
        "mtool",
    ]) || stem == "vnr"
    {
        return Some(Role::TranslatorTool);
    }
    if has(&["setup", "install", "インストール"]) {
        return Some(Role::Installer);
    }
    if has(&["config", "setting", "設定", "configure", "option"]) {
        return Some(Role::ConfigTool);
    }
    if has(&["update", "notification_helper", "readme", "manual", "ffmpeg", "python", "subprocess", "7z", "unzip"]) {
        return Some(Role::Helper);
    }
    None
}

/// Look for installer-builder markers near the start of the file.
fn has_installer_signature(path: &Path) -> bool {
    const MARKERS: &[&[u8]] =
        &[b"Inno Setup", b"InnoSetupLdr", b"Nullsoft", b"NullsoftInst", b"InstallShield", b"7zSfx"];
    let Ok(file) = std::fs::File::open(path) else { return false };
    let mut buf = Vec::new();
    if file.take(SIGNATURE_BYTES).read_to_end(&mut buf).is_err() {
        return false;
    }
    MARKERS.iter().any(|m| buf.windows(m.len()).any(|w| w == *m))
}

fn lower_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn is_exe(name_lower: &str) -> bool {
    name_lower.ends_with(".exe")
}

fn is_msi(name_lower: &str) -> bool {
    name_lower.ends_with(".msi")
}

/// Folder whose name best describes the game (skips `bin`, `x64`, ...).
fn title_dir(root: &Path) -> &Path {
    let mut dir = root;
    for _ in 0..3 {
        let name = lower_name(dir);
        if !GENERIC_DIRS.contains(&name.as_str()) {
            break;
        }
        match dir.parent() {
            Some(parent) if parent.file_name().is_some() => dir = parent,
            _ => break,
        }
    }
    dir
}

/// Clean a folder name into a display title:
/// `"[KID] Ever17_the out of infinity (v1.02) [RJ123456]"` -> `"Ever17 the out of infinity"`.
pub fn suggest_title(raw: &str) -> String {
    let stripped = strip_brackets(raw);
    let base = if stripped.trim().is_empty() { raw.to_string() } else { stripped };
    let mut s = base.replace('_', " ");
    if !s.contains(' ') && s.matches('.').count() >= 2 {
        s = s.replace('.', " ");
    }
    let tokens: Vec<&str> = s.split_whitespace().filter(|t| !is_noise_token(t)).collect();
    let title = tokens.join(" ");
    let title = title.trim_matches(|c: char| c == '-' || c == '~' || c == '.' || c.is_whitespace()).to_string();
    if title.is_empty() { raw.trim().to_string() } else { title }
}

fn strip_brackets(s: &str) -> String {
    let pairs = [('[', ']'), ('(', ')'), ('【', '】'), ('（', '）'), ('{', '}')];
    let mut out = String::with_capacity(s.len());
    let mut stack: Vec<char> = Vec::new();
    for c in s.chars() {
        if let Some((_, close)) = pairs.iter().find(|(open, _)| *open == c) {
            stack.push(*close);
            continue;
        }
        if stack.last() == Some(&c) {
            stack.pop();
            out.push(' ');
            continue;
        }
        if stack.is_empty() {
            out.push(c);
        }
    }
    out
}

fn is_noise_token(t: &str) -> bool {
    let lower = t.to_lowercase();
    let version = lower.strip_prefix("ver.").or_else(|| lower.strip_prefix("ver")).or_else(|| lower.strip_prefix('v'));
    if let Some(rest) = version
        && rest.starts_with(|c: char| c.is_ascii_digit())
        && rest.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        return true;
    }
    // Release-date tokens such as 190524 / 20190524.
    (t.len() == 6 || t.len() == 8) && t.chars().all(|c| c.is_ascii_digit())
}

struct Found {
    path: PathBuf,
    rel: PathBuf,
    depth: usize,
    size: u64,
    msi: bool,
}

/// Scan an `.exe`/`.msi` file or a folder. `existing` maps an exe path to a
/// library game id (duplicate detection).
pub fn scan(path: &Path, existing: impl Fn(&Path) -> Option<vnl_core::GameId>) -> Result<ImportScan, AppError> {
    let meta = std::fs::metadata(path).map_err(|e| {
        let err: AppError = e.into();
        AppError::new(err.code, "Cannot read the selected path").with_details(format!("{}: {}", path.display(), err))
    })?;

    let (root, found, listings) = if meta.is_file() {
        let name = lower_name(path);
        if !is_exe(&name) && !is_msi(&name) {
            return Err(AppError::invalid("Select a Windows executable (.exe) or a game folder")
                .with_details(path.display().to_string()));
        }
        let root = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/"));
        let mut listings = HashMap::new();
        listings.insert(root.clone(), list_dir(&root));
        let found = vec![Found {
            path: path.to_path_buf(),
            rel: PathBuf::from(path.file_name().unwrap_or_default()),
            depth: 0,
            size: meta.len(),
            msi: is_msi(&name),
        }];
        (root, found, listings)
    } else {
        let (found, listings) = walk(path);
        (path.to_path_buf(), found, listings)
    };

    let title_source = title_dir(&root);
    let folder_name = title_source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let folder_key = compact(&folder_name);
    let root_key = compact(&root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());

    let mut candidates = Vec::with_capacity(found.len());
    let mut signature_checks = 0;
    for f in &found {
        let name = lower_name(&f.path);
        let stem = name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(name.clone());
        let rel_dirs: Vec<String> = f
            .rel
            .parent()
            .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect())
            .unwrap_or_default();
        let mut hints: Vec<String> = Vec::new();
        let mut score = 0i32;

        let role = if f.msi { Some(Role::Installer) } else { classify(&stem, &rel_dirs) };
        let mut is_installer = matches!(role, Some(Role::Installer));
        if f.msi {
            hints.push("msi".into());
        }
        if !f.msi && role.is_none() && signature_checks < MAX_SIGNATURE_CHECKS {
            signature_checks += 1;
            if has_installer_signature(&f.path) {
                is_installer = true;
                hints.push("installerSignature".into());
                score += Role::Installer.penalty();
            }
        }
        if let Some(role) = role {
            hints.push(role.hint().into());
            score += role.penalty();
        }

        let stem_key = compact(&stem);
        if !stem_key.is_empty() {
            let matches = |key: &str| {
                key.len() >= 3
                    && (stem_key == key || key.contains(&stem_key) && stem_key.len() >= 3 || stem_key.contains(key))
            };
            if matches(&folder_key) || matches(&root_key) {
                score += 30;
                hints.push("matchesFolderName".into());
            }
        }

        if let Some(dir) = f.path.parent().and_then(|d| listings.get(d)) {
            for engine in detect_engines(dir) {
                score += 15;
                hints.push(format!("engine:{engine}"));
            }
            let specific = match stem.as_str() {
                "siglusengine" | "bgi" | "reallive" => true,
                "game" => detect_engines(dir).contains(&"rpgmaker"),
                _ => {
                    dir.has_dir(&format!("{stem}_data"))
                        || (dir.has_dir("renpy") && dir.has_file(&format!("{stem}.py")))
                }
            };
            if specific && role.is_none() {
                score += 25;
                hints.push("engineMainExe".into());
            }
        }

        if f.size >= 1024 * 1024 {
            score += 5;
        } else if f.size < 64 * 1024 {
            score -= 10;
            hints.push("tiny".into());
        }
        if f.depth > 0 {
            score -= 8 * f.depth as i32;
            hints.push("nested".into());
        }

        candidates.push(ExeCandidate {
            path: f.path.clone(),
            relative_path: f.rel.clone(),
            size_bytes: f.size,
            score,
            hints,
            is_installer,
        });
    }

    // Largest plausible executable bonus.
    if let Some(largest) = candidates
        .iter_mut()
        .filter(|c| !c.is_installer && c.score > -30 && !c.hints.iter().any(|h| h == "msi"))
        .max_by_key(|c| c.size_bytes)
    {
        largest.score += 15;
        largest.hints.push("largest".into());
    }

    candidates.sort_by(|a, b| b.score.cmp(&a.score).then(b.size_bytes.cmp(&a.size_bytes)).then(a.path.cmp(&b.path)));

    let is_installer = if meta.is_file() {
        candidates.first().is_some_and(|c| c.is_installer)
    } else {
        !candidates.is_empty()
            && candidates.iter().all(|c| c.is_installer || c.score <= -80)
            && candidates.iter().any(|c| c.is_installer)
    };

    let existing_game_id = candidates.iter().find_map(|c| existing(&c.path));
    let suggested_title = if folder_name.is_empty() {
        candidates.first().map(|c| suggest_title(&lower_name(&c.path))).unwrap_or_default()
    } else {
        suggest_title(&folder_name)
    };

    Ok(ImportScan { root, candidates, suggested_title, is_installer, existing_game_id })
}

fn list_dir(dir: &Path) -> DirListing {
    let mut listing = DirListing::default();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten().take(MAX_ENTRIES) {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            match entry.file_type() {
                Ok(t) if t.is_dir() => listing.dirs.push(name),
                Ok(_) => listing.files.push(name),
                Err(_) => {}
            }
        }
    }
    listing
}

/// Breadth-first walk with depth and entry limits. Symlinked directories are
/// not followed (avoids loops and escaping the game folder).
fn walk(root: &Path) -> (Vec<Found>, HashMap<PathBuf, DirListing>) {
    let mut found = Vec::new();
    let mut listings = HashMap::new();
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut seen = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut listing = DirListing::default();
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                tracing::warn!(root = %root.display(), "scan entry limit reached, results may be incomplete");
                queue.clear();
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                listing.dirs.push(name);
                if depth < MAX_DEPTH {
                    queue.push_back((path, depth + 1));
                }
                continue;
            }
            listing.files.push(name.clone());
            if is_exe(&name) || is_msi(&name) {
                // Follows file symlinks; skips dangling ones.
                let Ok(meta) = std::fs::metadata(&path) else { continue };
                if !meta.is_file() {
                    continue;
                }
                let rel = path.strip_prefix(root).map(Path::to_path_buf).unwrap_or_else(|_| path.clone());
                found.push(Found { path, rel, depth, size: meta.len(), msi: is_msi(&name) });
            }
        }
        listings.insert(dir, listing);
    }
    (found, listings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, size: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; size]).unwrap();
    }

    #[test]
    fn title_cleanup() {
        assert_eq!(suggest_title("[KID] Ever17_the out of infinity (v1.02) [RJ123456]"), "Ever17 the out of infinity");
        assert_eq!(suggest_title("【同人】ひぐらしのなく頃に 祭 ver1.10"), "ひぐらしのなく頃に 祭");
        assert_eq!(suggest_title("[190524][Key] Summer Pockets"), "Summer Pockets");
        assert_eq!(suggest_title("Muv.Luv.Alternative"), "Muv Luv Alternative");
        assert_eq!(suggest_title("D.C. Da Capo"), "D.C. Da Capo");
        assert_eq!(suggest_title("[only brackets]"), "[only brackets]");
    }

    #[test]
    fn kirikiri_folder_with_spaces_and_japanese() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("[ゆずソフト] 千恋＊万花 v1.1");
        write(&root.join("千恋＊万花.exe"), 3 * 1024 * 1024);
        write(&root.join("data.xp3"), 100);
        write(&root.join("unins000.exe"), 1024 * 1024);
        write(&root.join("環境設定.exe"), 500 * 1024);
        write(&root.join("_CommonRedist/vcredist_x86.exe"), 6 * 1024 * 1024);
        write(&root.join("LunaHook/LunaHost32.exe"), 2 * 1024 * 1024);

        let scan = scan(&root, |_| None).unwrap();
        assert_eq!(scan.suggested_title, "千恋＊万花");
        assert!(!scan.is_installer);
        let best = &scan.candidates[0];
        assert_eq!(best.path, root.join("千恋＊万花.exe"), "{:#?}", scan.candidates);
        assert!(best.hints.contains(&"matchesFolderName".to_string()));
        assert!(best.hints.contains(&"engine:kirikiri".to_string()));
        let hint_of = |n: &str| scan.candidates.iter().find(|c| c.path.ends_with(n)).unwrap().hints.clone();
        assert!(hint_of("unins000.exe").contains(&"uninstaller".into()));
        assert!(hint_of("vcredist_x86.exe").contains(&"redistributable".into()));
        assert!(hint_of("LunaHost32.exe").contains(&"translatorTool".into()));
        assert!(hint_of("環境設定.exe").contains(&"configTool".into()));
        assert!(scan.candidates.iter().skip(1).all(|c| c.score < best.score));
    }

    #[test]
    fn unity_and_siglus_and_nested_bin() {
        let tmp = tempfile::tempdir().unwrap();
        let unity = tmp.path().join("My Unity VN");
        write(&unity.join("Launcher.exe"), 2 * 1024 * 1024);
        write(&unity.join("MyVN.exe"), 700 * 1024);
        write(&unity.join("UnityPlayer.dll"), 10);
        write(&unity.join("MyVN_Data/level0"), 10);
        write(&unity.join("UnityCrashHandler64.exe"), 1024 * 1024);
        let s = scan(&unity, |_| None).unwrap();
        assert_eq!(s.candidates[0].path, unity.join("MyVN.exe"), "{:#?}", s.candidates);

        let siglus = tmp.path().join("リトルバスターズ");
        write(&siglus.join("SiglusEngine.exe"), 2 * 1024 * 1024);
        write(&siglus.join("Scene.pck"), 10);
        write(&siglus.join("Config.exe"), 3 * 1024 * 1024);
        let s = scan(&siglus, |_| None).unwrap();
        assert_eq!(s.candidates[0].path, siglus.join("SiglusEngine.exe"));

        let nested = tmp.path().join("Some Game/bin");
        write(&nested.join("SomeGame.exe"), 2 * 1024 * 1024);
        let s = scan(&tmp.path().join("Some Game"), |_| None).unwrap();
        assert_eq!(s.candidates[0].relative_path, PathBuf::from("bin/SomeGame.exe"));
        // Exe given directly inside a generic folder: title from the parent.
        let s = scan(&nested.join("SomeGame.exe"), |_| None).unwrap();
        assert_eq!(s.suggested_title, "Some Game");
        assert_eq!(s.candidates.len(), 1);
    }

    #[test]
    fn installers_are_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("installer dl");
        write(&dir.join("setup.exe"), 1000);
        let s = scan(&dir.join("setup.exe"), |_| None).unwrap();
        assert!(s.is_installer);
        let s = scan(&dir, |_| None).unwrap();
        assert!(s.is_installer);

        // Inno/NSIS marker in an innocently named exe.
        let mut bytes = vec![0u8; 4096];
        bytes[1000..1000 + 23].copy_from_slice(b"Nullsoft Install System");
        let nsis = tmp.path().join("other/MyGame_1.0.exe");
        fs::create_dir_all(nsis.parent().unwrap()).unwrap();
        fs::write(&nsis, &bytes).unwrap();
        let s = scan(&nsis, |_| None).unwrap();
        assert!(s.is_installer);
        assert!(s.candidates[0].hints.contains(&"installerSignature".to_string()));

        let msi = tmp.path().join("msi/Game.msi");
        write(&msi, 10);
        assert!(scan(&msi, |_| None).unwrap().is_installer);
    }

    #[test]
    fn rejects_non_exe_and_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let txt = tmp.path().join("readme.txt");
        fs::write(&txt, "x").unwrap();
        assert_eq!(scan(&txt, |_| None).unwrap_err().code, vnl_core::ErrorCode::InvalidInput);
        assert_eq!(scan(&tmp.path().join("nope"), |_| None).unwrap_err().code, vnl_core::ErrorCode::NotFound);
    }

    #[test]
    fn depth_limit_is_respected() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("g/1/2/3/4/5/deep.exe");
        write(&deep, 10);
        let shallow = tmp.path().join("g/1/2/3/4/ok.exe");
        write(&shallow, 10);
        let s = scan(&tmp.path().join("g"), |_| None).unwrap();
        assert!(s.candidates.iter().any(|c| c.path == shallow));
        assert!(!s.candidates.iter().any(|c| c.path == deep));
    }
}
