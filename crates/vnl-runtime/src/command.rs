//! Pure command building for Wine, Proton (direct and umu-run) and native runners.
//!
//! Environment precedence (later wins): runner base env -> locale -> `TZ` ->
//! `WINEDLLOVERRIDES` -> Proton option flags -> user env.
//!
//! Notes verified against GE-Proton11-11's `proton` script:
//! - verbs `run`, `waitforexitandrun` (waits for `wineserver -w` first) and
//!   `runinprefix` (plain `wine <exe>` with Proton's env, no prefix setup —
//!   safe while the game is running);
//! - the prefix lives in `$STEAM_COMPAT_DATA_PATH/pfx/`;
//! - `STEAM_COMPAT_CLIENT_INSTALL_PATH` is read unconditionally during prefix setup;
//! - `LC_ALL` is dropped unless `HOST_LC_ALL` is set, so we set both.
//!
//! umu-run (verified from umu-launcher's docs and `umu_run.py`): it sets
//! `STEAM_COMPAT_DATA_PATH=$WINEPREFIX` and only creates a `pfx -> .`
//! symlink when `$WINEPREFIX/pfx` is not a directory. We therefore pass the
//! compat-data dir as `WINEPREFIX` (after making sure `pfx/` exists) so umu and
//! direct Proton use the exact same on-disk layout. `PROTON_VERB` selects the
//! verb (`runinprefix` for aux programs), `GAMEID` defaults to `umu-default`.

use std::path::{Path, PathBuf};

use vnl_core::{
    AppError, CommandSpec, ErrorCode, Game, PrefixArch, PrefixInfo, ProtonLauncher, Runner, RunnerKind, RuntimeConfig,
};

use crate::prefixes::{self, PrefixLayout};
use crate::runners;

/// Host facts the builder depends on. Injected so tests are deterministic.
#[derive(Debug, Clone, Default)]
pub struct HostContext {
    pub umu_run: Option<PathBuf>,
    /// Real Steam installation (for `STEAM_COMPAT_CLIENT_INSTALL_PATH`).
    pub steam_root: Option<PathBuf>,
    /// Used when Steam is not installed (created before spawning).
    pub fallback_steam_dir: PathBuf,
    /// `PROTON_LOG_DIR` when Proton logging is enabled.
    pub logs_dir: PathBuf,
    pub winetricks: Option<PathBuf>,
    /// Output of `locale -a`; `None` = unknown (no warning emitted).
    pub locales: Option<Vec<String>>,
}

impl HostContext {
    pub fn detect(paths: &vnl_core::AppPaths) -> Self {
        let steam_root = std::env::var_os("HOME").map(PathBuf::from).and_then(|home| {
            [".local/share/Steam", ".steam/root", ".steam/steam"]
                .iter()
                .map(|p| home.join(p))
                .find(|p| p.join("steamapps").is_dir() || p.join("ubuntu12_32").is_dir())
                .map(|p| prefixes::canonical(&p))
        });
        let locales = std::process::Command::new("locale")
            .arg("-a")
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).collect());
        Self {
            umu_run: which::which("umu-run").ok(),
            steam_root,
            fallback_steam_dir: paths.data_dir.join("steam-compat-client"),
            logs_dir: paths.logs_dir(),
            winetricks: which::which("winetricks").ok(),
            locales,
        }
    }

    pub fn steam_client_dir(&self) -> PathBuf {
        self.steam_root.clone().unwrap_or_else(|| self.fallback_steam_dir.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtonVerb {
    Run,
    WaitForExitAndRun,
    RunInPrefix,
}

impl ProtonVerb {
    pub fn as_str(self) -> &'static str {
        match self {
            ProtonVerb::Run => "run",
            ProtonVerb::WaitForExitAndRun => "waitforexitandrun",
            ProtonVerb::RunInPrefix => "runinprefix",
        }
    }
}

/// How a Proton runner is invoked for a given prefix/config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtonMode {
    Direct,
    Umu(PathBuf),
    /// Plain (non compat-data) prefix: Proton's bundled wine is used directly so
    /// the prefix is never restructured by the `proton` script.
    BundledWine,
}

pub fn proton_mode(ctx: &HostContext, config: &RuntimeConfig, prefix: &PrefixInfo) -> Result<ProtonMode, AppError> {
    if prefixes::layout(prefix) == PrefixLayout::Plain {
        return Ok(ProtonMode::BundledWine);
    }
    match config.proton.launcher {
        ProtonLauncher::Direct => Ok(ProtonMode::Direct),
        ProtonLauncher::Auto => Ok(ctx.umu_run.clone().map(ProtonMode::Umu).unwrap_or(ProtonMode::Direct)),
        ProtonLauncher::Umu => ctx.umu_run.clone().map(ProtonMode::Umu).ok_or_else(|| {
            AppError::new(ErrorCode::MissingDependency, "umu-run is not installed")
                .with_details("Install umu-launcher or set the Proton launcher to \"Auto\" or \"Direct\".")
        }),
    }
}

/// A Windows program to run with a runner/prefix.
#[derive(Debug, Clone)]
pub struct Invocation<'a> {
    pub exe: &'a Path,
    pub args: &'a [String],
    pub cwd: Option<PathBuf>,
    pub verb: ProtonVerb,
    /// Game install dir (`STEAM_COMPAT_INSTALL_PATH`), game commands only.
    pub install_dir: Option<&'a Path>,
}

#[derive(Debug, Clone)]
pub struct Built {
    pub spec: CommandSpec,
    pub warnings: Vec<String>,
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Steam app id derived from the umu game id (`"umu-1234"` / `"1234"`), else `"0"`.
fn steam_app_id(umu_game_id: Option<&str>) -> String {
    umu_game_id
        .map(|g| g.trim().trim_start_matches("umu-"))
        .filter(|g| !g.is_empty() && g.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or("0")
        .to_string()
}

fn normalize_locale(l: &str) -> String {
    l.to_ascii_lowercase().replace("utf-8", "utf8")
}

fn wine_env(spec: &mut CommandSpec, config: &RuntimeConfig, prefix: &PrefixInfo, wineserver: Option<PathBuf>) {
    spec.env.insert("WINEPREFIX".into(), path_str(&prefixes::wine_prefix_dir(prefix)));
    if !config.proton.enable_log {
        spec.env.insert("WINEDEBUG".into(), "-all".into());
    }
    if let Some(ws) = wineserver {
        spec.env.insert("WINESERVER".into(), path_str(&ws));
    }
}

/// Build the full command for a Windows program (or a native one with the native runner).
pub fn build(
    ctx: &HostContext,
    inv: &Invocation<'_>,
    config: &RuntimeConfig,
    runner: &Runner,
    prefix: &PrefixInfo,
) -> Result<Built, AppError> {
    let mut warnings = Vec::new();
    let exe = path_str(inv.exe);
    let mut uses_proton_script = false;

    // 1. runner base command + env
    let mut spec = match runner.kind {
        RunnerKind::Native => CommandSpec::new(inv.exe).args(inv.args.iter().cloned()),
        RunnerKind::Wine => {
            let mut spec = CommandSpec::new(&runner.path).arg(exe).args(inv.args.iter().cloned());
            let wineserver = if runner.source != vnl_core::RunnerSource::System {
                runner.path.parent().map(|p| p.join("wineserver")).filter(|p| p.is_file())
            } else {
                None
            };
            wine_env(&mut spec, config, prefix, wineserver);
            if prefix.arch == PrefixArch::Win32 && !prefixes::is_initialized(prefix) {
                spec.env.insert("WINEARCH".into(), "win32".into());
            }
            spec
        }
        RunnerKind::Proton => match proton_mode(ctx, config, prefix)? {
            ProtonMode::BundledWine => {
                let wine = runners::wine_binary(runner).ok_or_else(|| {
                    AppError::new(
                        ErrorCode::MissingDependency,
                        format!("{} has no bundled wine binary (files/bin/wine)", runner.name),
                    )
                })?;
                warnings.push(format!(
                    "{} is a plain WINEPREFIX (no pfx/ directory): running {}'s bundled wine directly. \
                     Proton's own prefix setup (DXVK/VKD3D, fonts, steam.exe) is skipped so the prefix is not restructured.",
                    prefix.path.display(),
                    runner.name
                ));
                let mut spec = CommandSpec::new(wine).arg(exe).args(inv.args.iter().cloned());
                wine_env(&mut spec, config, prefix, runners::wineserver_binary(runner));
                spec
            }
            ProtonMode::Direct => {
                uses_proton_script = true;
                let mut spec = CommandSpec::new(runner.path.join("proton"))
                    .arg(inv.verb.as_str())
                    .arg(exe)
                    .args(inv.args.iter().cloned())
                    .env("STEAM_COMPAT_DATA_PATH", path_str(&prefix.path))
                    .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", path_str(&ctx.steam_client_dir()));
                let app_id = steam_app_id(config.proton.umu_game_id.as_deref());
                spec.env.insert("SteamAppId".into(), app_id.clone());
                spec.env.insert("SteamGameId".into(), app_id);
                if let Some(dir) = inv.install_dir {
                    spec.env.insert("STEAM_COMPAT_INSTALL_PATH".into(), path_str(dir));
                }
                spec
            }
            ProtonMode::Umu(umu) => {
                uses_proton_script = true;
                let game_id = config
                    .proton
                    .umu_game_id
                    .clone()
                    .filter(|g| !g.trim().is_empty())
                    .unwrap_or_else(|| "umu-default".into());
                CommandSpec::new(umu)
                    .arg(exe)
                    .args(inv.args.iter().cloned())
                    .env("WINEPREFIX", path_str(&prefix.path))
                    .env("PROTONPATH", path_str(&runner.path))
                    .env("GAMEID", game_id)
                    .env("PROTON_VERB", inv.verb.as_str())
            }
        },
    };

    // 2. locale
    let locale = config
        .locale
        .locale
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .or_else(|| if config.locale.japanese_locale { Some("ja_JP.UTF-8".to_string()) } else { None });
    if let Some(locale) = locale {
        if let Some(available) = &ctx.locales {
            let want = normalize_locale(&locale);
            if !available.iter().any(|a| normalize_locale(a) == want) {
                warnings.push(format!(
                    "locale {locale} is not installed (missing from `locale -a`); Japanese text may be garbled. \
                     Enable it in /etc/locale.gen and run locale-gen."
                ));
            }
        }
        spec.env.insert("LANG".into(), locale.clone());
        spec.env.insert("LC_ALL".into(), locale.clone());
        if uses_proton_script {
            spec.env.insert("HOST_LC_ALL".into(), locale);
        }
    }

    // 3. timezone
    if let Some(tz) = config.locale.timezone.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        spec.env.insert("TZ".into(), tz.to_string());
    }

    // 4. DLL overrides
    if runner.kind != RunnerKind::Native && !config.dll_overrides.is_empty() {
        let value = config
            .dll_overrides
            .iter()
            .filter(|(k, _)| !k.trim().is_empty())
            .map(|(k, v)| format!("{}={}", k.trim(), v.trim()))
            .collect::<Vec<_>>()
            .join(";");
        spec.env.insert("WINEDLLOVERRIDES".into(), value);
    }

    // 5. Proton flags
    if runner.kind == RunnerKind::Proton && uses_proton_script {
        let p = &config.proton;
        for (flag, key) in [
            (p.use_wined3d, "PROTON_USE_WINED3D"),
            (p.no_esync, "PROTON_NO_ESYNC"),
            (p.no_fsync, "PROTON_NO_FSYNC"),
            (p.enable_log, "PROTON_LOG"),
        ] {
            if flag {
                spec.env.insert(key.into(), "1".into());
            }
        }
        if p.enable_log {
            spec.env.insert("PROTON_LOG_DIR".into(), path_str(&ctx.logs_dir));
        }
    }

    // 6. user env (wins)
    spec.env.extend(config.env.iter().map(|(k, v)| (k.clone(), v.clone())));

    spec.cwd = inv.cwd.clone();
    Ok(Built { spec, warnings })
}

pub fn build_game(
    ctx: &HostContext,
    game: &Game,
    config: &RuntimeConfig,
    runner: &Runner,
    prefix: &PrefixInfo,
) -> Result<Built, AppError> {
    let inv = Invocation {
        exe: &game.exe_path,
        args: &game.launch_args,
        cwd: Some(game.effective_working_dir()),
        verb: ProtonVerb::WaitForExitAndRun,
        install_dir: Some(&game.install_dir),
    };
    build(ctx, &inv, config, runner, prefix)
}

pub fn build_in_prefix(
    ctx: &HostContext,
    exe: &Path,
    args: &[String],
    cwd: Option<&Path>,
    config: &RuntimeConfig,
    runner: &Runner,
    prefix: &PrefixInfo,
) -> Result<Built, AppError> {
    let cwd = cwd
        .map(Path::to_path_buf)
        .or_else(|| exe.parent().filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf));
    let inv = Invocation { exe, args, cwd, verb: ProtonVerb::RunInPrefix, install_dir: None };
    build(ctx, &inv, config, runner, prefix)
}

/// Inside a Flatpak sandbox, run the command on the host via `flatpak-spawn --host`.
/// The environment and cwd are forwarded as `--env=`/`--unset-env=`/`--directory=`.
pub fn flatpak_host_command(spec: &CommandSpec) -> CommandSpec {
    let mut args = vec!["--host".to_string()];
    for (k, v) in &spec.env {
        args.push(format!("--env={k}={v}"));
    }
    for k in &spec.env_remove {
        args.push(format!("--unset-env={k}"));
    }
    if let Some(cwd) = &spec.cwd {
        args.push(format!("--directory={}", cwd.to_string_lossy()));
    }
    args.push(spec.program.to_string_lossy().into_owned());
    args.extend(spec.args.iter().cloned());
    CommandSpec { program: PathBuf::from("flatpak-spawn"), args, ..Default::default() }
}

/// Final transformation applied right before spawning.
pub fn finalize_for_spawn(spec: &CommandSpec, flatpak: bool) -> CommandSpec {
    if flatpak { flatpak_host_command(spec) } else { spec.clone() }
}

pub fn running_in_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// `wineserver` command (e.g. `-w`, `-k`) for this runner/prefix.
pub fn wineserver_command(runner: &Runner, prefix: &PrefixInfo, arg: &str) -> Option<CommandSpec> {
    let ws = runners::wineserver_binary(runner)?;
    Some(CommandSpec::new(ws).arg(arg).env("WINEPREFIX", path_str(&prefixes::wine_prefix_dir(prefix))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vnl_core::{GameMetadata, PlayStatus, PrefixKind, RunnerSource};

    const EXE: &str = "/games/My Game/ゲーム.exe";

    fn game() -> Game {
        Game {
            id: 1,
            title: "ゲーム".into(),
            original_title: None,
            exe_path: EXE.into(),
            install_dir: "/games/My Game".into(),
            working_dir: None,
            launch_args: vec!["-windowed".into(), "two words".into()],
            status: PlayStatus::Unplayed,
            favorite: false,
            playtime_seconds: 0,
            last_played_at: None,
            added_at: 0,
            metadata: GameMetadata::default(),
            notes: None,
        }
    }

    fn prefix(path: &Path, managed: bool) -> PrefixInfo {
        PrefixInfo {
            id: "p".into(),
            name: "p".into(),
            path: path.to_path_buf(),
            arch: PrefixArch::Win64,
            kind: PrefixKind::Dedicated { game_id: 1 },
            runner_id: None,
            created_at: 0,
            managed,
            initialized: false,
        }
    }

    fn wine() -> Runner {
        Runner {
            id: "wine:system".into(),
            name: "System Wine".into(),
            kind: RunnerKind::Wine,
            source: RunnerSource::System,
            path: "/usr/bin/wine".into(),
            version: None,
        }
    }

    fn proton(dir: &Path) -> Runner {
        Runner {
            id: "proton:GE".into(),
            name: "GE".into(),
            kind: RunnerKind::Proton,
            source: RunnerSource::Steam,
            path: dir.to_path_buf(),
            version: None,
        }
    }

    fn ctx(umu: Option<&str>) -> HostContext {
        HostContext {
            umu_run: umu.map(PathBuf::from),
            steam_root: None,
            fallback_steam_dir: "/data/steam-compat-client".into(),
            logs_dir: "/data/logs".into(),
            winetricks: None,
            locales: Some(vec!["C.utf8".into(), "en_US.utf8".into()]),
        }
    }

    fn config() -> RuntimeConfig {
        let mut c = RuntimeConfig::default();
        c.locale.timezone = Some("Asia/Tokyo".into());
        c.dll_overrides = BTreeMap::from([("d3d9".into(), "n,b".into()), ("dinput8".into(), "n".into())]);
        c.env = BTreeMap::from([("LANG".into(), "user-wins".into()), ("FOO".into(), "bar baz".into())]);
        c
    }

    #[test]
    fn wine_game_command() {
        let p = prefix(Path::new("/data/prefixes/game-1"), true);
        let mut cfg = config();
        let built = build_game(&ctx(None), &game(), &cfg, &wine(), &p).unwrap();
        let s = &built.spec;
        assert_eq!(s.program, PathBuf::from("/usr/bin/wine"));
        assert_eq!(s.args, vec![EXE, "-windowed", "two words"]);
        assert_eq!(s.env["WINEPREFIX"], "/data/prefixes/game-1/pfx");
        assert_eq!(s.env["WINEDEBUG"], "-all");
        assert_eq!(s.env["LC_ALL"], "ja_JP.UTF-8");
        assert_eq!(s.env["LANG"], "user-wins");
        assert_eq!(s.env["FOO"], "bar baz");
        assert_eq!(s.env["TZ"], "Asia/Tokyo");
        assert_eq!(s.env["WINEDLLOVERRIDES"], "d3d9=n,b;dinput8=n");
        assert!(!s.env.contains_key("WINEARCH"));
        assert!(!s.env.contains_key("HOST_LC_ALL"));
        assert_eq!(s.cwd.as_deref(), Some(Path::new("/games/My Game")));
        assert!(built.warnings.iter().any(|w| w.contains("ja_JP.UTF-8")), "missing locale warning");

        cfg.proton.enable_log = true;
        let mut p32 = p.clone();
        p32.arch = PrefixArch::Win32;
        let s = build_game(&ctx(None), &game(), &cfg, &wine(), &p32).unwrap().spec;
        assert!(!s.env.contains_key("WINEDEBUG"));
        assert_eq!(s.env["WINEARCH"], "win32");
        assert!(!s.env.contains_key("PROTON_LOG"));
    }

    #[test]
    fn locale_found_and_custom() {
        let p = prefix(Path::new("/x"), true);
        let mut c = ctx(None);
        c.locales = Some(vec!["ja_JP.utf8".into()]);
        let mut cfg = RuntimeConfig::default();
        assert!(build_game(&c, &game(), &cfg, &wine(), &p).unwrap().warnings.is_empty());
        cfg.locale.locale = Some("zh_CN.UTF-8".into());
        let b = build_game(&c, &game(), &cfg, &wine(), &p).unwrap();
        assert_eq!(b.spec.env["LC_ALL"], "zh_CN.UTF-8");
        assert_eq!(b.warnings.len(), 1);
        cfg.locale = vnl_core::LocaleConfig { japanese_locale: false, locale: None, timezone: None };
        let b = build_game(&c, &game(), &cfg, &wine(), &p).unwrap();
        assert!(!b.spec.env.contains_key("LANG"));
    }

    #[test]
    fn proton_direct_game_and_runinprefix() {
        let p = prefix(Path::new("/data/prefixes/My Prefix ゲーム"), true);
        let pr = proton(Path::new("/steam/compatibilitytools.d/GE-Proton11-11"));
        let mut cfg = config();
        cfg.proton.use_wined3d = true;
        cfg.proton.no_fsync = true;
        cfg.proton.enable_log = true;
        cfg.proton.launcher = ProtonLauncher::Auto;
        let s = build_game(&ctx(None), &game(), &cfg, &pr, &p).unwrap().spec;
        assert_eq!(s.program, PathBuf::from("/steam/compatibilitytools.d/GE-Proton11-11/proton"));
        assert_eq!(s.args, vec!["waitforexitandrun", EXE, "-windowed", "two words"]);
        assert_eq!(s.env["STEAM_COMPAT_DATA_PATH"], "/data/prefixes/My Prefix ゲーム");
        assert_eq!(s.env["STEAM_COMPAT_CLIENT_INSTALL_PATH"], "/data/steam-compat-client");
        assert_eq!(s.env["STEAM_COMPAT_INSTALL_PATH"], "/games/My Game");
        assert_eq!(s.env["SteamAppId"], "0");
        assert_eq!(s.env["SteamGameId"], "0");
        assert_eq!(s.env["HOST_LC_ALL"], "ja_JP.UTF-8");
        assert_eq!(s.env["PROTON_USE_WINED3D"], "1");
        assert_eq!(s.env["PROTON_NO_FSYNC"], "1");
        assert_eq!(s.env["PROTON_LOG"], "1");
        assert_eq!(s.env["PROTON_LOG_DIR"], "/data/logs");
        assert!(!s.env.contains_key("PROTON_NO_ESYNC"));
        assert!(!s.env.contains_key("WINEPREFIX"));

        let aux_exe = Path::new("/tools/Luna Hook/LunaHost64.exe");
        let args = vec!["-p".to_string(), "ゲーム.exe".to_string()];
        let mut c = ctx(None);
        c.steam_root = Some("/home/u/.local/share/Steam".into());
        cfg.proton.umu_game_id = Some("umu-12345".into());
        let s = build_in_prefix(&c, aux_exe, &args, None, &cfg, &pr, &p).unwrap().spec;
        assert_eq!(s.args, vec!["runinprefix", "/tools/Luna Hook/LunaHost64.exe", "-p", "ゲーム.exe"]);
        assert_eq!(s.cwd.as_deref(), Some(Path::new("/tools/Luna Hook")));
        assert_eq!(s.env["STEAM_COMPAT_CLIENT_INSTALL_PATH"], "/home/u/.local/share/Steam");
        assert_eq!(s.env["SteamAppId"], "12345");
        assert!(!s.env.contains_key("STEAM_COMPAT_INSTALL_PATH"));
    }

    #[test]
    fn umu_commands() {
        let p = prefix(Path::new("/data/prefixes/game 1"), true);
        let pr = proton(Path::new("/opt/GE-Proton"));
        let mut cfg = config();
        // Auto picks umu when installed
        let s = build_game(&ctx(Some("/usr/bin/umu-run")), &game(), &cfg, &pr, &p).unwrap().spec;
        assert_eq!(s.program, PathBuf::from("/usr/bin/umu-run"));
        assert_eq!(s.args, vec![EXE, "-windowed", "two words"]);
        assert_eq!(s.env["WINEPREFIX"], "/data/prefixes/game 1");
        assert_eq!(s.env["PROTONPATH"], "/opt/GE-Proton");
        assert_eq!(s.env["GAMEID"], "umu-default");
        assert_eq!(s.env["PROTON_VERB"], "waitforexitandrun");
        assert!(!s.env.contains_key("STEAM_COMPAT_DATA_PATH"));
        // aux -> runinprefix
        cfg.proton.umu_game_id = Some("umu-123".into());
        let s = build_in_prefix(&ctx(Some("/usr/bin/umu-run")), Path::new("C:/x.exe"), &[], None, &cfg, &pr, &p)
            .unwrap()
            .spec;
        assert_eq!(s.env["PROTON_VERB"], "runinprefix");
        assert_eq!(s.env["GAMEID"], "umu-123");
        // Direct overrides Auto
        cfg.proton.launcher = ProtonLauncher::Direct;
        let s = build_game(&ctx(Some("/usr/bin/umu-run")), &game(), &cfg, &pr, &p).unwrap().spec;
        assert_eq!(s.args[0], "waitforexitandrun");
        // Umu requested but missing
        cfg.proton.launcher = ProtonLauncher::Umu;
        let err = build_game(&ctx(None), &game(), &cfg, &pr, &p).unwrap_err();
        assert_eq!(err.code, ErrorCode::MissingDependency);
    }

    #[test]
    fn proton_on_plain_prefix_uses_bundled_wine() {
        let tmp = tempfile::tempdir().unwrap();
        let pdir = tmp.path().join("GE");
        std::fs::create_dir_all(pdir.join("files/bin")).unwrap();
        std::fs::write(pdir.join("files/bin/wine"), "").unwrap();
        std::fs::write(pdir.join("files/bin/wineserver"), "").unwrap();
        let plain = tmp.path().join("my wine prefix");
        std::fs::create_dir_all(plain.join("drive_c")).unwrap();
        let p = prefix(&plain, false);
        let built = build_game(&ctx(Some("/usr/bin/umu-run")), &game(), &config(), &proton(&pdir), &p).unwrap();
        let s = built.spec;
        assert_eq!(s.program, pdir.join("files/bin/wine"));
        assert_eq!(s.args[0], EXE);
        assert_eq!(PathBuf::from(&s.env["WINEPREFIX"]), plain);
        assert_eq!(PathBuf::from(&s.env["WINESERVER"]), pdir.join("files/bin/wineserver"));
        assert!(!s.env.contains_key("STEAM_COMPAT_DATA_PATH"));
        assert!(built.warnings.iter().any(|w| w.contains("plain WINEPREFIX")));
    }

    #[test]
    fn native_runner() {
        let p = prefix(Path::new("/x"), true);
        let mut g = game();
        g.exe_path = "/games/My Game/ゲーム.sh".into();
        g.working_dir = Some("/games/work dir".into());
        let s = build_game(&ctx(None), &g, &config(), &runners::native_runner(), &p).unwrap().spec;
        assert_eq!(s.program, PathBuf::from("/games/My Game/ゲーム.sh"));
        assert_eq!(s.args, vec!["-windowed", "two words"]);
        assert!(!s.env.contains_key("WINEPREFIX"));
        assert!(!s.env.contains_key("WINEDLLOVERRIDES"));
        assert_eq!(s.env["TZ"], "Asia/Tokyo");
        assert_eq!(s.cwd.as_deref(), Some(Path::new("/games/work dir")));
    }

    #[test]
    fn flatpak_transform() {
        let spec = CommandSpec::new("/opt/proton/proton")
            .args(["run", EXE])
            .env("WINEPREFIX", "/p q")
            .env("LANG", "ja_JP.UTF-8")
            .cwd("/games/My Game");
        let mut spec = spec;
        spec.env_remove.push("LD_PRELOAD".into());
        let out = flatpak_host_command(&spec);
        assert_eq!(out.program, PathBuf::from("flatpak-spawn"));
        assert_eq!(
            out.args,
            vec![
                "--host",
                "--env=LANG=ja_JP.UTF-8",
                "--env=WINEPREFIX=/p q",
                "--unset-env=LD_PRELOAD",
                "--directory=/games/My Game",
                "/opt/proton/proton",
                "run",
                EXE
            ]
        );
        assert!(out.env.is_empty() && out.cwd.is_none());
        assert_eq!(finalize_for_spawn(&spec, false), spec);
    }
}
