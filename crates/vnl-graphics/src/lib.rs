//! Graphics subsystem: gamescope wrapping, Wine FSR env, upscaler adapter,
//! display/dependency detection. Owned by Agent 5.
//!
//! CONTRACT (used by src-tauri/src/commands/session.rs — keep signatures stable,
//! you may add methods/fields freely).
//!
//! Modules:
//! * [`gamescope`] — argv builder + validation (flags verified with gamescope 3.17 `--help`).
//! * [`wine_fsr`] — `WINE_FULLSCREEN_FSR` env + Proton-GE support check.
//! * [`upscaler`] — `Upscaler` trait + RT-Upscaler (linux-rt-upscaler) adapter.
//! * [`display`] — session type, monitors (sysfs/EDID), GPUs, Vulkan.
//! * [`deps`] — dependency discovery with install hints (never installs anything).
//! * [`system`] — system report.
//! * [`prefix`] — Japanese-VN prefix component catalog + installed detection.
//! * [`probe`] — system access abstraction (fixture roots / fake PATH in tests).

pub mod deps;
pub mod display;
pub mod fullscreen;
pub mod gamescope;
pub mod gamescope_bin;
pub mod gamescope_build;
pub mod prefix;
pub mod probe;
pub mod system;
#[cfg(test)]
mod test_support;
pub mod upscaler;
pub mod wine_fsr;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vnl_core::{AppError, AppPaths, CommandSpec, GraphicsConfig, Runner, RunnerKind};

pub use deps::{DependencyCategory, DependencyStatus, InstallHint};
pub use display::{DisplaySessionType, GraphicsCapabilities, MonitorInfo};
pub use gamescope_bin::{GamescopeBinary, GamescopeSource};
pub use prefix::PrefixComponent;
pub use probe::{Probe, ToolInfo};
pub use system::SystemReport;
pub use upscaler::{Upscaler, UpscalerInfo, UpscalerModel};

/// Result of applying a graphics config to a game command.
#[derive(Debug, Clone, Default)]
pub struct GraphicsPlan {
    /// Game command, wrapped by gamescope when enabled, with extra env.
    pub command: CommandSpec,
    /// Native helper processes to start alongside the game (e.g. upscaler).
    pub companions: Vec<CommandSpec>,
    pub warnings: Vec<String>,
}

/// What `graphics_preview` returns: the wrapper and env graphics would add.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsPreview {
    /// gamescope program + args up to and including `--` (empty when disabled).
    pub wrapper_args: Vec<String>,
    /// Environment variables added by graphics features.
    pub env: BTreeMap<String, String>,
    pub warnings: Vec<String>,
    /// Shell-quoted preview of the full (placeholder) game command.
    pub command_preview: String,
    /// Shell-quoted previews of companion processes (upscaler).
    pub companions: Vec<String>,
}

/// Application-level inputs to [`GraphicsService::apply_with`].
#[derive(Debug, Clone, Default)]
pub struct GraphicsOptions {
    /// `AppSettings::gamescope_path`.
    pub gamescope_path: Option<PathBuf>,
}

pub struct GraphicsService {
    #[allow(dead_code)]
    paths: AppPaths,
    probe: Probe,
}

const INSTALL_GAMESCOPE: &str = "Install gamescope (Arch: sudo pacman -S gamescope, Debian/Ubuntu: sudo apt install gamescope, Fedora: sudo dnf install gamescope; Flatpak: org.freedesktop.Platform.VulkanLayer.gamescope) or disable gamescope for this game.";

impl GraphicsService {
    pub fn new(paths: &AppPaths) -> Self {
        Self { paths: paths.clone(), probe: Probe::system() }
    }

    /// Service bound to a specific probe (tests, fixture trees).
    pub fn with_probe(paths: &AppPaths, probe: Probe) -> Self {
        Self { paths: paths.clone(), probe }
    }

    pub fn probe(&self) -> &Probe {
        &self.probe
    }

    /// Apply `config` to the fully-built runtime command. When every graphics
    /// feature is disabled this must return the command unchanged.
    pub fn apply(
        &self,
        config: &GraphicsConfig,
        command: CommandSpec,
        runner: &Runner,
    ) -> Result<GraphicsPlan, AppError> {
        self.apply_with(config, command, runner, &GraphicsOptions::default())
    }

    /// Like [`apply`](Self::apply) with application-level options (custom gamescope path).
    pub fn apply_with(
        &self,
        config: &GraphicsConfig,
        command: CommandSpec,
        runner: &Runner,
        options: &GraphicsOptions,
    ) -> Result<GraphicsPlan, AppError> {
        self.plan(config, command, Some(runner), false, options)
    }

    /// Preview the wrapper/env for `config` without a game. Missing tools are
    /// reported as warnings instead of errors; invalid settings still error.
    pub fn preview(&self, config: &GraphicsConfig, runner: Option<&Runner>) -> Result<GraphicsPreview, AppError> {
        self.preview_with(config, runner, &GraphicsOptions::default())
    }

    pub fn preview_with(
        &self,
        config: &GraphicsConfig,
        runner: Option<&Runner>,
        options: &GraphicsOptions,
    ) -> Result<GraphicsPreview, AppError> {
        let base = placeholder_command(runner);
        let plan = self.plan(config, base.clone(), runner, true, options)?;
        let wrapper_args = if config.gamescope.enabled {
            let inner = base.args.len() + 1;
            let mut v = vec![plan.command.program.to_string_lossy().into_owned()];
            v.extend(plan.command.args[..plan.command.args.len() - inner].iter().cloned());
            v
        } else {
            Vec::new()
        };
        let env = plan
            .command
            .env
            .iter()
            .filter(|(k, v)| base.env.get(*k) != Some(v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(GraphicsPreview {
            wrapper_args,
            env,
            warnings: plan.warnings,
            command_preview: plan.command.preview(),
            companions: plan.companions.iter().map(CommandSpec::preview).collect(),
        })
    }

    fn plan(
        &self,
        config: &GraphicsConfig,
        command: CommandSpec,
        runner: Option<&Runner>,
        lenient: bool,
        options: &GraphicsOptions,
    ) -> Result<GraphicsPlan, AppError> {
        if !config.gamescope.enabled && !config.wine_fsr.enabled && !config.upscaler.enabled {
            return Ok(GraphicsPlan { command, ..Default::default() });
        }
        let probe = &self.probe;
        let mut command = command;
        let mut warnings = Vec::new();
        let mut companions = Vec::new();
        let gs = &config.gamescope;

        // 1. Wine FSR env (set on the inner command; wrapping preserves env).
        warnings.extend(wine_fsr::apply(&config.wine_fsr, &mut command, runner)?.warnings);
        if config.wine_fsr.enabled && gs.enabled && gamescope::is_upscaling_filter(gs.filter) {
            warnings.push(format!(
                "Both Wine FSR and gamescope's {} filter are enabled: the image may be upscaled twice. Usually enable only one (gamescope FSR works for any game; Wine FSR only for fullscreen Vulkan games in Proton-GE).",
                gamescope::filter_value(gs.filter).to_uppercase()
            ));
        }

        // 2. Upscaler companion.
        if config.upscaler.enabled {
            let backend = upscaler::backend(config.upscaler.backend.as_deref())?;
            let ctx = upscaler::UpscalerContext {
                session: display::session_type(probe),
                xwayland: display::xwayland_available(probe),
                gamescope_enabled: gs.enabled,
                game_env: &command.env,
                lenient,
            };
            let plan = backend.plan(probe, &config.upscaler, &ctx)?;
            warnings.extend(plan.warnings);
            companions.extend(plan.companions);
        }

        // 3. gamescope wrapper. Never silently dropped: missing binary = error.
        if gs.enabled {
            let conns = display::connectors(probe);
            let ctx = gamescope::GamescopeContext {
                session: display::session_type(probe),
                nested: display::nested_session(probe),
                connectors: conns.iter().map(|(_, n, _)| n.clone()).collect(),
                connected: conns.iter().filter(|(_, _, c)| *c).map(|(_, n, _)| n.clone()).collect(),
                wsi_layer: display::gamescope_wsi_layer(probe),
                desktop: display::desktop(probe),
            };
            let built = gamescope::build_args(gs, &ctx)?;
            let binaries = gamescope_bin::discover(
                probe,
                &self.paths,
                gs.binary_path.as_deref(),
                options.gamescope_path.as_deref(),
            );
            let program = match gamescope_bin::select(&binaries, gs.filter) {
                Ok((bin, w)) => {
                    warnings.extend(w);
                    bin.path.clone()
                }
                Err(e) if lenient => {
                    warnings.push(format!("{}: launching with these settings will fail.", e.message));
                    PathBuf::from("gamescope")
                }
                Err(e) if binaries.is_empty() => return Err(e.with_details(INSTALL_GAMESCOPE)),
                Err(e) => return Err(e),
            };
            if gs.filter == vnl_core::GamescopeFilter::Anime4k {
                let nvidia = probe.sys_path("/proc/driver/nvidia/version").exists();
                warnings.extend(gamescope::anime4k_warnings(gs, nvidia));
            }
            if runner.is_some_and(|r| r.kind == RunnerKind::Proton) && command.env.contains_key("PROTON_ENABLE_WAYLAND")
            {
                warnings.push("PROTON_ENABLE_WAYLAND is set: gamescope's nested session only provides XWayland by default (use --expose-wayland in extra args).".into());
            }
            warnings.extend(built.warnings);
            command = command.wrapped_by(program, built.args);
        }

        Ok(GraphicsPlan { command, companions, warnings })
    }

    pub fn capabilities(&self) -> GraphicsCapabilities {
        let rt = upscaler::RtUpscaler.info(&self.probe).tool;
        display::capabilities(&self.probe, rt)
    }

    pub fn upscaler_info(&self) -> UpscalerInfo {
        upscaler::RtUpscaler.info(&self.probe)
    }

    pub fn dependencies(&self, runners: &[Runner]) -> Vec<DependencyStatus> {
        deps::detect(&self.probe, runners)
    }

    pub fn system_report(&self) -> SystemReport {
        system::report(&self.probe)
    }

    pub fn prefix_components(&self, prefix_path: &Path) -> Result<Vec<PrefixComponent>, AppError> {
        prefix::components(prefix_path)
    }
}

/// Placeholder game command used by previews (no game needed).
pub fn placeholder_command(runner: Option<&Runner>) -> CommandSpec {
    const EXE: &str = "/games/Example VN/game.exe";
    let mut spec = match runner {
        Some(r) if r.kind == RunnerKind::Proton => CommandSpec::new(r.path.join("proton")).args(["run", EXE]),
        Some(r) if r.kind == RunnerKind::Native => CommandSpec::new("/games/Example VN/game"),
        Some(r) => CommandSpec::new(&r.path).arg(EXE),
        None => CommandSpec::new("wine").arg(EXE),
    };
    spec.cwd = Some(PathBuf::from("/games/Example VN"));
    spec
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::script;
    use vnl_core::ErrorCode;
    use vnl_core::{
        GamescopeConfig, GamescopeFilter, GamescopeScaler, RunnerSource, UpscalerConfig, WindowMode, WineFsrConfig,
    };

    struct Fx {
        dir: tempfile::TempDir,
        svc: GraphicsService,
    }

    fn fx(with_gamescope: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        if with_gamescope {
            script(
                &bin,
                "gamescope",
                "case \"$1\" in\n--version) echo 'gamescope version 3.17.0' >&2;;\n--help) echo 'gamescope usage'; echo '  -F, --filter  upscaler filter (linear, nearest, fsr, nis, pixel, sgsr)';;\nesac\nexit 0",
            );
        }
        let root = dir.path().join("root");
        crate::test_support::drm_fixture(&root);
        std::fs::create_dir_all(root.join("usr/share/vulkan/implicit_layer.d")).unwrap();
        std::fs::write(root.join("usr/share/vulkan/implicit_layer.d/VkLayer_FROG_gamescope_wsi.x86_64.json"), "{}")
            .unwrap();
        let env: BTreeMap<String, String> = [
            ("PATH", bin.to_str().unwrap()),
            ("HOME", dir.path().join("home").to_str().unwrap()),
            ("XDG_SESSION_TYPE", "wayland"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":1"),
            ("XDG_CURRENT_DESKTOP", "GNOME"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let mut probe = Probe::fixture(&root, env);
        probe.run_commands = false;
        let paths = AppPaths::under(&dir.path().join("app"));
        Fx { svc: GraphicsService::with_probe(&paths, probe), dir }
    }

    fn ge_runner(dir: &Path) -> Runner {
        let p = dir.join("GE-Proton11-11");
        let lib = p.join("files/lib/wine/x86_64-unix");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("win32u.so"), b"..WINE_FULLSCREEN_FSR..").unwrap();
        Runner {
            id: "proton:GE-Proton11-11".into(),
            name: "GE-Proton11-11".into(),
            kind: RunnerKind::Proton,
            source: RunnerSource::Steam,
            path: p,
            version: Some("GE-Proton11-11".into()),
        }
    }

    fn game_cmd(runner: &Runner) -> CommandSpec {
        CommandSpec::new(runner.path.join("proton"))
            .args(["run", "/games/My Game/ゲーム.exe"])
            .env("WINEPREFIX", "/prefixes/my game/pfx")
            .env("STEAM_COMPAT_DATA_PATH", "/prefixes/my game")
            .cwd("/games/My Game")
    }

    #[test]
    fn disabled_returns_command_unchanged() {
        let f = fx(false);
        let r = ge_runner(f.dir.path());
        let cmd = game_cmd(&r);
        // Disabled features keep their (even invalid) values without effect.
        let cfg = GraphicsConfig {
            gamescope: GamescopeConfig { enabled: false, sharpness: Some(99), ..Default::default() },
            wine_fsr: WineFsrConfig { enabled: false, strength: Some(42) },
            upscaler: UpscalerConfig { enabled: false, model: Some("bogus".into()), ..Default::default() },
        };
        let plan = f.svc.apply(&cfg, cmd.clone(), &r).unwrap();
        assert_eq!(plan.command, cmd);
        assert!(plan.companions.is_empty() && plan.warnings.is_empty());
        let plan = f.svc.apply(&GraphicsConfig::default(), cmd.clone(), &r).unwrap();
        assert_eq!(plan.command, cmd);
    }

    /// Same fixture, but helper programs (`gamescope --help`) may run.
    fn with_commands(f: &Fx) -> GraphicsService {
        let mut probe = f.svc.probe().clone();
        probe.run_commands = true;
        GraphicsService::with_probe(&AppPaths::under(&f.dir.path().join("app")), probe)
    }

    /// A gamescope that lists `anime4k` (like the managed fork build).
    fn fork_fx() -> Fx {
        let f = fx(true);
        script(
            &f.dir.path().join("bin"),
            "gamescope",
            "case \"$1\" in\n--version) echo 'gamescope version 863ff3b' >&2;;\n--help) echo 'gamescope usage'; echo '  -F, --filter  upscaler filter (linear, nearest, fsr, nis, pixel, anime4k)';;\nesac\nexit 0",
        );
        f
    }

    #[test]
    fn anime4k_uses_a_capable_binary_and_warns_on_nvidia() {
        let f = fork_fx();
        let svc = with_commands(&f);
        let r = ge_runner(f.dir.path());
        let cfg = GraphicsConfig {
            gamescope: GamescopeConfig {
                enabled: true,
                game_width: Some(1280),
                game_height: Some(720),
                output_width: Some(1920),
                output_height: Some(1080),
                filter: GamescopeFilter::Anime4k,
                ..Default::default()
            },
            ..Default::default()
        };
        let plan = svc.apply(&cfg, game_cmd(&r), &r).unwrap();
        let sep = plan.command.args.iter().position(|a| a == "--").unwrap();
        assert!(plan.command.args[..sep].windows(2).any(|w| w == ["-F", "anime4k"]));
        assert!(plan.warnings.iter().any(|w| w.contains("heaviest")));
        assert!(!plan.warnings.iter().any(|w| w.contains("NVIDIA")), "no NVIDIA driver in the fixture");
        // Same config on a machine with the NVIDIA proprietary driver.
        let nv = f.dir.path().join("root/proc/driver/nvidia");
        std::fs::create_dir_all(&nv).unwrap();
        std::fs::write(nv.join("version"), "NVRM version: 615.78").unwrap();
        let plan = svc.apply(&cfg, game_cmd(&r), &r).unwrap();
        assert!(plan.warnings.iter().any(|w| w.contains("black image")));
    }

    #[test]
    fn anime4k_without_a_capable_binary_is_a_clear_error_and_other_filters_still_work() {
        let f = fx(true); // standard gamescope: no anime4k
        let svc = with_commands(&f);
        let r = ge_runner(f.dir.path());
        let mk = |filter| GraphicsConfig {
            gamescope: GamescopeConfig { enabled: true, filter, ..Default::default() },
            ..Default::default()
        };
        let err = svc.apply(&mk(GamescopeFilter::Anime4k), game_cmd(&r), &r).unwrap_err();
        assert_eq!(err.code, ErrorCode::MissingDependency);
        assert!(err.details.unwrap().contains("Settings > Gamescope"));
        for filter in [GamescopeFilter::Fsr, GamescopeFilter::Nis, GamescopeFilter::Linear] {
            let plan = svc.apply(&mk(filter), game_cmd(&r), &r).unwrap();
            assert!(plan.command.program.ends_with("gamescope"));
        }
        // A custom binary supporting anime4k is picked for that filter only.
        let custom = crate::test_support::fake_gamescope(f.dir.path(), "fork-gs", "linear, fsr, anime4k");
        let opts = GraphicsOptions { gamescope_path: Some(custom.clone()) };
        let plan = svc.apply_with(&mk(GamescopeFilter::Anime4k), game_cmd(&r), &r, &opts).unwrap();
        assert_eq!(plan.command.program, custom);
    }

    #[test]
    fn golden_gamescope_wrap_order() {
        let f = fx(true);
        let r = ge_runner(f.dir.path());
        let cfg = GraphicsConfig {
            gamescope: GamescopeConfig {
                enabled: true,
                game_width: Some(1280),
                game_height: Some(720),
                output_width: Some(1920),
                output_height: Some(1080),
                window_mode: WindowMode::Fullscreen,
                scaler: GamescopeScaler::Fit,
                filter: GamescopeFilter::Fsr,
                sharpness: Some(5),
                fps_limit: Some(60),
                ..Default::default()
            },
            ..Default::default()
        };
        let plan = f.svc.apply(&cfg, game_cmd(&r), &r).unwrap();
        let proton = r.path.join("proton").to_string_lossy().into_owned();
        assert_eq!(plan.command.program, f.dir.path().join("bin/gamescope"));
        assert_eq!(
            plan.command.args,
            [
                "-w",
                "1280",
                "-h",
                "720",
                "-W",
                "1920",
                "-H",
                "1080",
                "-r",
                "60",
                "-f",
                "-S",
                "fit",
                "-F",
                "fsr",
                "--sharpness",
                "5",
                "--",
                &proton,
                "run",
                "/games/My Game/ゲーム.exe"
            ]
        );
        assert_eq!(plan.command.env.get("WINEPREFIX").unwrap(), "/prefixes/my game/pfx");
        assert_eq!(plan.command.cwd.as_deref(), Some(Path::new("/games/My Game")));
        assert!(plan.command.preview().ends_with(&format!("-- {proton} run '/games/My Game/ゲーム.exe'")));
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn missing_gamescope_is_an_error_not_dropped() {
        let f = fx(false);
        let r = ge_runner(f.dir.path());
        let cfg =
            GraphicsConfig { gamescope: GamescopeConfig { enabled: true, ..Default::default() }, ..Default::default() };
        let e = f.svc.apply(&cfg, game_cmd(&r), &r).unwrap_err();
        assert_eq!(e.code, ErrorCode::MissingDependency);
        assert!(e.details.unwrap().contains("pacman -S gamescope"));
        // Preview is lenient.
        let p = f.svc.preview(&cfg, Some(&r)).unwrap();
        assert_eq!(p.wrapper_args, ["gamescope", "--"]);
        assert!(p.warnings.iter().any(|w| w.contains("not installed")));
    }

    #[test]
    fn wine_fsr_env_and_combination_warning() {
        let f = fx(true);
        let r = ge_runner(f.dir.path());
        let cfg = GraphicsConfig { wine_fsr: WineFsrConfig { enabled: true, strength: Some(2) }, ..Default::default() };
        let plan = f.svc.apply(&cfg, game_cmd(&r), &r).unwrap();
        assert_eq!(plan.command.program, r.path.join("proton"), "Wine FSR alone does not wrap");
        assert_eq!(plan.command.env.get("WINE_FULLSCREEN_FSR").unwrap(), "1");
        assert_eq!(plan.command.env.get("WINE_FULLSCREEN_FSR_STRENGTH").unwrap(), "2");
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);

        let both = GraphicsConfig {
            gamescope: GamescopeConfig {
                enabled: true,
                filter: GamescopeFilter::Fsr,
                game_width: Some(1280),
                game_height: Some(720),
                window_mode: WindowMode::Fullscreen,
                ..Default::default()
            },
            ..cfg
        };
        let plan = f.svc.apply(&both, game_cmd(&r), &r).unwrap();
        assert!(plan.warnings.iter().any(|w| w.contains("Both Wine FSR and gamescope's FSR")));
        assert_eq!(plan.command.env.get("WINE_FULLSCREEN_FSR").unwrap(), "1", "env survives wrapping");
    }

    #[test]
    fn preview_without_game() {
        let f = fx(true);
        let cfg = GraphicsConfig {
            gamescope: GamescopeConfig {
                enabled: true,
                game_width: Some(1280),
                game_height: Some(720),
                window_mode: WindowMode::Borderless,
                ..Default::default()
            },
            wine_fsr: WineFsrConfig { enabled: true, strength: None },
            ..Default::default()
        };
        let p = f.svc.preview(&cfg, None).unwrap();
        assert_eq!(
            p.wrapper_args,
            [f.dir.path().join("bin/gamescope").to_string_lossy().as_ref(), "-w", "1280", "-h", "720", "-b", "--"]
        );
        assert_eq!(p.env, BTreeMap::from([("WINE_FULLSCREEN_FSR".to_string(), "1".to_string())]));
        assert!(p.warnings.iter().any(|w| w.contains("No runner selected")));
        assert!(p.command_preview.contains("-- wine '/games/Example VN/game.exe'"));
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("wrapperArgs").is_some() && json.get("commandPreview").is_some());

        // Proton runner placeholder.
        let r = ge_runner(f.dir.path());
        let p = f.svc.preview(&cfg, Some(&r)).unwrap();
        assert!(p.command_preview.contains("proton run '/games/Example VN/game.exe'"));
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);

        // Validation errors still surface in previews.
        let bad = GraphicsConfig {
            gamescope: GamescopeConfig {
                enabled: true,
                sharpness: Some(30),
                filter: GamescopeFilter::Fsr,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(f.svc.preview(&bad, None).unwrap_err().code, ErrorCode::InvalidInput);
    }

    #[test]
    fn upscaler_missing_is_error_and_unknown_backend_unsupported() {
        let f = fx(true);
        let r = ge_runner(f.dir.path());
        let cfg =
            GraphicsConfig { upscaler: UpscalerConfig { enabled: true, ..Default::default() }, ..Default::default() };
        assert_eq!(f.svc.apply(&cfg, game_cmd(&r), &r).unwrap_err().code, ErrorCode::MissingDependency);
        let cfg = GraphicsConfig {
            upscaler: UpscalerConfig { enabled: true, backend: Some("lossless-scaling".into()), ..Default::default() },
            ..Default::default()
        };
        assert_eq!(f.svc.apply(&cfg, game_cmd(&r), &r).unwrap_err().code, ErrorCode::Unsupported);
        // Preview: companion shown with a warning.
        let cfg = GraphicsConfig {
            upscaler: UpscalerConfig { enabled: true, model: Some("4x24".into()), ..Default::default() },
            ..Default::default()
        };
        let p = f.svc.preview(&cfg, Some(&r)).unwrap();
        assert_eq!(p.companions, ["upscale -m 4x24 --target-delay 10"]);
        assert!(p.wrapper_args.is_empty());
    }

    /// REAL integration test: run the installed gamescope with generated args
    /// using the headless backend (no window, no DRM output) and a trivial
    /// child. gamescope exits 1 with "unrecognized option" / "invalid value"
    /// for bad flags, so success proves the generated flags are accepted.
    /// Run with: cargo test -p vnl-graphics -- --ignored --nocapture
    #[test]
    #[ignore = "spawns the real gamescope binary"]
    fn real_gamescope_accepts_generated_flags() {
        let probe = Probe::system();
        let Some(gamescope) = display::find_gamescope(&probe) else {
            panic!("gamescope not installed");
        };
        let svc = GraphicsService::new(&AppPaths::under(tempfile::tempdir().unwrap().path()));
        let configs = [
            GamescopeConfig {
                enabled: true,
                game_width: Some(1280),
                game_height: Some(720),
                output_width: Some(1920),
                output_height: Some(1080),
                window_mode: WindowMode::Fullscreen,
                scaler: GamescopeScaler::Fit,
                filter: GamescopeFilter::Fsr,
                sharpness: Some(5),
                fps_limit: Some(60),
                fps_limit_unfocused: Some(10),
                force_grab_cursor: true,
                prefer_output: Some("DP-1".into()),
                ..Default::default()
            },
            GamescopeConfig {
                enabled: true,
                game_width: Some(800),
                game_height: Some(600),
                output_width: Some(1600),
                output_height: Some(1200),
                window_mode: WindowMode::Borderless,
                scaler: GamescopeScaler::Integer,
                filter: GamescopeFilter::Nis,
                sharpness: Some(10),
                hdr: true,
                ..Default::default()
            },
            GamescopeConfig {
                enabled: true,
                scaler: GamescopeScaler::Stretch,
                filter: GamescopeFilter::Pixel,
                ..Default::default()
            },
            GamescopeConfig {
                enabled: true,
                scaler: GamescopeScaler::Fill,
                filter: GamescopeFilter::Nearest,
                ..Default::default()
            },
        ];
        for gs in configs {
            let mut gs = gs;
            gs.extra_args = vec!["--backend".into(), "headless".into()];
            let cfg = GraphicsConfig { gamescope: gs, ..Default::default() };
            let game = CommandSpec::new("/bin/sh")
                .args(["-c", "echo \"VNL_CHILD_OK display=$DISPLAY fsr=$WINE_FULLSCREEN_FSR\""])
                .env("WINE_FULLSCREEN_FSR", "1");
            let runner = Runner {
                id: "native".into(),
                name: "native".into(),
                kind: RunnerKind::Native,
                source: RunnerSource::System,
                path: "/bin/sh".into(),
                version: None,
            };
            let plan = svc.apply(&cfg, game, &runner).unwrap();
            assert_eq!(plan.command.program, gamescope);
            println!("running: {}", plan.command.preview());
            let mut cmd = plan.command.to_std_command();
            cmd.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let out = probe::run_with_timeout(cmd, std::time::Duration::from_secs(30)).expect("gamescope timed out");
            let text = out.combined();
            assert!(!text.contains("unrecognized option") && !text.contains("invalid value"), "{text}");
            assert!(out.success, "gamescope failed:\n{text}");
            let line = text
                .lines()
                .find(|l| l.contains("VNL_CHILD_OK"))
                .unwrap_or_else(|| panic!("child did not run:\n{text}"));
            println!("  -> {line}");
            assert!(line.contains("fsr=1"), "env must reach the child: {line}");
            assert!(!line.contains("display= "), "child must get gamescope's DISPLAY: {line}");
        }
    }

    /// Prints real-machine detection results (read-only). Run with
    /// `cargo test -p vnl-graphics real_machine_report -- --ignored --nocapture`.
    #[test]
    #[ignore = "inspects the real machine"]
    fn real_machine_report() {
        let svc = GraphicsService::new(&AppPaths::under(tempfile::tempdir().unwrap().path()));
        println!("CAPS {}", serde_json::to_string_pretty(&svc.capabilities()).unwrap());
        println!("SYSTEM {}", serde_json::to_string_pretty(&svc.system_report()).unwrap());
        for d in svc.dependencies(&[]) {
            println!(
                "DEP {:<20} found={:<5} req={:<5} ver={:?} path={:?} details={:?}",
                d.id, d.found, d.required, d.version, d.path, d.details
            );
        }
        println!("UPSCALER {}", serde_json::to_string_pretty(&svc.upscaler_info()).unwrap());
    }
}
