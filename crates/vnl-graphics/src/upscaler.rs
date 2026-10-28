//! Real-time upscaler adapters.
//!
//! # RT-Upscaler (`linux-rt-upscaler`) — research notes
//!
//! Sources (checked 2026-10): <https://github.com/baronsmv/linux-rt-upscaler>
//! (pyproject.toml, `src/upscaler/config/args.py`, `config/models.py`,
//! `config/yaml.py`, `env.py`, `config-example.yaml`,
//! `docs/developer/architecture/limitations.md`, `docs/user/installation.md`),
//! docs at <https://baronsmv.github.io/linux-rt-upscaler/user/cli/>, PyPI
//! `linux-rt-upscaler` (1.1.4 at the time of writing, GPL-3.0-or-later).
//!
//! * **What it is**: a Python (PySide6/Qt + xcffib) app that captures one
//!   X11/XWayland window with XShm + XDamage (tile-based, only changed tiles
//!   are reprocessed), upscales it 2x with CuNNy SRCNN networks on the GPU via
//!   Vulkan compute (optionally twice = 4x with `-2`), and presents the result
//!   in its **own separate Qt overlay window** (modes: `always-on-top`,
//!   `top-transparent`, `fullscreen`, `windowed`). It is *not* a Vulkan layer
//!   and does not hook the game. Mouse clicks/motion are forwarded to the
//!   original window with synthetic X events; **keyboard events are not
//!   forwarded**, so the game window must keep focus.
//! * **Entry points**: `upscale` (CLI) and `upscale-gui`. Installed with
//!   `pipx install linux-rt-upscaler`; the Qt XCB plugin needs
//!   `xcb-util-cursor` / `libxcb-cursor0`.
//! * **CLI** (verified in `args.py`): `-m/--model {none,veryfast,faster,fast,
//!   3x12,4x12,4x16,4x24,4x32,8x32}` (default `fast`), `-2/--double-upscale`,
//!   `-p/--profile NAME`, `-c/--config PATH`, target selection
//!   `-t/--target-title SUBSTR`, `--target-title-regex RE`, `-s/--select`,
//!   `-d/--daemon`, `-f/--follow-focus`, `--target-delay SECONDS` (delay
//!   before capturing the *active* window when no target is given),
//!   `-o/--output-geometry {fit,stretch,cover,...}`, `--overlay-mode`,
//!   `--monitor`, `-g/--gpu`, `--crop-*`, and a positional `program...`
//!   (`upscale -- program args`) which the docs call less reliable.
//!   `-v/--version` prints `upscale <version>`.
//! * **Models**: shipped *inside the Python package* as SPIR-V shaders,
//!   one directory per model: `<site-packages>/upscaler/srcnn/cunny/<model>/`
//!   (plus a hidden `.originals/` with HLSL sources). There is no user model
//!   directory; all CuNNy models are 2x.
//! * **Presets**: named *profiles* in `$XDG_CONFIG_HOME/linux-rt-upscaler/
//!   config.yaml` under a top-level `profiles:` mapping (selected with `-p`, or
//!   matched automatically by title/size rules).
//! * **Requirements/limitations**: GNU/Linux, Python ≥ 3.10, Vulkan GPU, X11 or
//!   XWayland. Native Wayland windows cannot be captured ("not planned"); on
//!   start it forces `QT_QPA_PLATFORM=xcb` and removes `WAYLAND_DISPLAY` from
//!   its own environment. gamescope's nested window on a Wayland session is a
//!   native Wayland window, so gamescope + RT-Upscaler do not combine there.
//!
//! # What is implemented
//! Detection (PATH, `~/.local/bin`, pipx venvs; the `upscale` script is
//! checked to really be linux-rt-upscaler), version (dist-info, else
//! `upscale --version`), model discovery from the package directory, profile
//! discovery from config.yaml, and a companion command
//! `upscale [-m M] [-2] [-p P] [--target-delay N] [extra...]` that attaches to
//! the game window (focused window after the delay, or the user's `-t` title).
//! Not verified end-to-end on this machine: RT-Upscaler is not installed here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vnl_core::{AppError, CommandSpec, ErrorCode, UpscalerConfig};

use crate::display::DisplaySessionType;
use crate::probe::{Probe, ToolInfo, extract_version, is_executable};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpscalerModel {
    pub id: String,
    pub name: String,
    pub path: Option<PathBuf>,
    pub scale: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpscalerInfo {
    pub available: bool,
    pub tool: Option<ToolInfo>,
    pub models: Vec<UpscalerModel>,
    pub presets: Vec<String>,
    pub limitations: Vec<String>,
    /// Backend id (`rt-upscaler`).
    pub backend: String,
    /// Directory the models were discovered in, if any.
    pub model_dir: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
}

/// Facts about the launch an adapter needs.
#[derive(Debug, Clone)]
pub struct UpscalerContext<'a> {
    pub session: DisplaySessionType,
    pub xwayland: bool,
    pub gamescope_enabled: bool,
    /// Environment of the game command (to spot Wine-Wayland).
    pub game_env: &'a std::collections::BTreeMap<String, String>,
    /// Preview mode: missing tools become warnings instead of errors.
    pub lenient: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpscalerPlan {
    pub companions: Vec<CommandSpec>,
    pub warnings: Vec<String>,
}

pub trait Upscaler: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn info(&self, probe: &Probe) -> UpscalerInfo;
    fn plan(&self, probe: &Probe, config: &UpscalerConfig, ctx: &UpscalerContext<'_>) -> Result<UpscalerPlan, AppError>;
}

pub fn backends() -> Vec<Box<dyn Upscaler>> {
    vec![Box::new(RtUpscaler)]
}

pub fn backend(id: Option<&str>) -> Result<Box<dyn Upscaler>, AppError> {
    let id = id.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(RtUpscaler::ID);
    backends().into_iter().find(|b| b.id() == id).ok_or_else(|| {
        AppError::new(ErrorCode::Unsupported, format!("Unknown upscaler backend '{id}'")).with_details("available: rt-upscaler")
    })
}

// ------------------------------------------------------------ RT-Upscaler

pub struct RtUpscaler;

/// Model ids in quality order, from `UPSCALING_MODELS` in
/// `upscaler/config/models.py` (v1.1.4). `none` = no SRCNN, scaler only.
pub const RT_MODELS: &[&str] = &["none", "veryfast", "faster", "fast", "3x12", "4x12", "4x16", "4x24", "4x32", "8x32"];
pub const RT_DEFAULT_MODEL: &str = "fast";
/// Seconds to wait before attaching to the focused window when the user gave
/// no explicit target (RT-Upscaler's own default is 5 s, too short for most
/// Wine/Proton game starts).
pub const RT_DEFAULT_TARGET_DELAY: u32 = 10;
const TARGET_FLAGS: &[&str] =
    &["-t", "--target-title", "--target-title-regex", "-s", "--select", "-d", "--daemon", "-f", "--follow-focus"];

const RT_INSTALL_HINT: &str = "pipx install linux-rt-upscaler";

#[derive(Debug, Clone, Default)]
pub struct RtInstall {
    pub executable: Option<PathBuf>,
    /// `site-packages` dirs to search for the `upscaler` package.
    pub site_dirs: Vec<PathBuf>,
    pub package_dir: Option<PathBuf>,
    pub version: Option<String>,
    /// An `upscale` executable exists but is not linux-rt-upscaler.
    pub foreign_executable: Option<PathBuf>,
}

impl RtUpscaler {
    pub const ID: &'static str = "rt-upscaler";

    fn candidates(probe: &Probe) -> Vec<PathBuf> {
        let mut c = Vec::new();
        if let Some(bin) = probe.var("PIPX_BIN_DIR") {
            c.push(PathBuf::from(bin).join("upscale"));
        }
        for venv in Self::venv_dirs(probe) {
            c.push(venv.join("bin/upscale"));
        }
        if let Some(home) = probe.home() {
            c.push(home.join(".local/bin/upscale"));
        }
        c
    }

    fn venv_dirs(probe: &Probe) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if let Some(p) = probe.var("PIPX_HOME") {
            v.push(PathBuf::from(p).join("venvs/linux-rt-upscaler"));
        }
        if let Some(d) = probe.data_home() {
            v.push(d.join("pipx/venvs/linux-rt-upscaler"));
        }
        if let Some(h) = probe.home() {
            v.push(h.join(".local/pipx/venvs/linux-rt-upscaler"));
        }
        v
    }

    /// Is this `upscale` script the linux-rt-upscaler entry point?
    fn is_rt_upscaler_script(path: &Path) -> bool {
        let Ok(bytes) = std::fs::read(path) else { return false };
        if bytes.len() > 64 * 1024 {
            return false; // a compiled binary, not the Python console script
        }
        let text = String::from_utf8_lossy(&bytes);
        text.contains("upscaler.cli") || text.contains("linux-rt-upscaler") || text.contains("linux_rt_upscaler")
    }

    /// venv root from a console-script shebang `#!/venv/bin/python`.
    fn venv_from_shebang(path: &Path) -> Option<PathBuf> {
        let text = std::fs::read_to_string(path).ok()?;
        let first = text.lines().next()?.strip_prefix("#!")?.trim();
        let interp = PathBuf::from(first.split_whitespace().next()?);
        if interp.file_name()?.to_string_lossy().starts_with("python") {
            Some(interp.parent()?.parent()?.to_path_buf())
        } else {
            None
        }
    }

    fn site_packages_in(prefix: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for lib in ["lib", "lib64"] {
            if let Ok(entries) = std::fs::read_dir(prefix.join(lib)) {
                for e in entries.flatten() {
                    let n = e.file_name().to_string_lossy().into_owned();
                    if n.starts_with("python3") {
                        for sub in ["site-packages", "dist-packages"] {
                            let p = e.path().join(sub);
                            if p.is_dir() {
                                out.push(p);
                            }
                        }
                    }
                }
            }
        }
        out.sort();
        out
    }

    pub fn locate(probe: &Probe) -> RtInstall {
        let mut inst = RtInstall::default();
        if let Some(exe) = probe.find_tool("upscale", &Self::candidates(probe)) {
            if Self::is_rt_upscaler_script(&exe) {
                if let Some(venv) = Self::venv_from_shebang(&exe) {
                    inst.site_dirs.extend(Self::site_packages_in(&venv));
                }
                inst.executable = Some(exe);
            } else {
                inst.foreign_executable = Some(exe);
            }
        }
        for venv in Self::venv_dirs(probe) {
            inst.site_dirs.extend(Self::site_packages_in(&venv));
        }
        if let Some(h) = probe.home() {
            inst.site_dirs.extend(Self::site_packages_in(&h.join(".local")));
        }
        for sys in ["/usr", "/usr/local"] {
            inst.site_dirs.extend(Self::site_packages_in(&probe.sys_path(sys)));
        }
        inst.site_dirs.dedup();

        for site in &inst.site_dirs {
            let pkg = site.join("upscaler");
            if pkg.join("srcnn").is_dir() {
                inst.package_dir.get_or_insert(pkg);
                if inst.version.is_none() {
                    inst.version = dist_info_version(site);
                }
                break;
            }
        }
        if inst.version.is_none()
            && let Some(exe) = &inst.executable
        {
            inst.version = probe.run(exe, &["--version"]).and_then(|o| extract_version(&o.combined()));
        }
        inst
    }

    pub fn models(inst: &RtInstall) -> (Vec<UpscalerModel>, Option<PathBuf>) {
        let model = |id: &str, path: Option<PathBuf>| UpscalerModel {
            id: id.to_string(),
            name: if id == "none" { "None (scaler only, no SRCNN)".into() } else { format!("CuNNy {id}") },
            path,
            scale: Some(2.0),
        };
        let dir = inst.package_dir.as_ref().map(|p| p.join("srcnn/cunny")).filter(|d| d.is_dir());
        let Some(dir) = dir else {
            return (RT_MODELS.iter().map(|m| model(m, None)).collect(), None);
        };
        let mut found: Vec<String> = std::fs::read_dir(&dir)
            .map(|it| {
                it.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| !n.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default();
        let rank = |id: &str| RT_MODELS.iter().position(|m| *m == id).unwrap_or(usize::MAX);
        found.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
        let mut models = vec![model("none", None)];
        models.extend(found.iter().map(|id| model(id, Some(dir.join(id)))));
        (models, Some(dir))
    }

    pub fn config_path(probe: &Probe) -> Option<PathBuf> {
        probe.config_home().map(|c| c.join("linux-rt-upscaler/config.yaml"))
    }
}

fn dist_info_version(site: &Path) -> Option<String> {
    let entries = std::fs::read_dir(site).ok()?;
    for e in entries.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if let Some(rest) = n.strip_prefix("linux_rt_upscaler-").and_then(|r| r.strip_suffix(".dist-info")) {
            return Some(rest.to_string());
        }
    }
    None
}

/// Profile names from the `profiles:` mapping of an RT-Upscaler config.yaml.
/// Minimal block-YAML reader: top-level `profiles:` followed by indented
/// `name:` keys; flow-style YAML is not supported.
pub fn parse_profiles(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_profiles = false;
    let mut child_indent: Option<usize> = None;
    for line in yaml.lines() {
        let trimmed = line.trim_end();
        let content = trimmed.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = trimmed.len() - content.len();
        if indent == 0 {
            in_profiles = content.split('#').next().unwrap_or("").trim() == "profiles:";
            child_indent = None;
            continue;
        }
        if !in_profiles {
            continue;
        }
        let ci = *child_indent.get_or_insert(indent);
        if indent != ci {
            continue;
        }
        if let Some((key, _)) = content.split_once(':') {
            let key = key.trim().trim_matches(|c| c == '"' || c == '\'');
            if !key.is_empty() && !key.starts_with('-') {
                out.push(key.to_string());
            }
        }
    }
    out
}

impl Upscaler for RtUpscaler {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn display_name(&self) -> &'static str {
        "RT-Upscaler (linux-rt-upscaler)"
    }

    fn info(&self, probe: &Probe) -> UpscalerInfo {
        let inst = Self::locate(probe);
        let (models, model_dir) = Self::models(&inst);
        let config_path = Self::config_path(probe);
        let presets = config_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).map(|y| parse_profiles(&y)).unwrap_or_default();
        let mut limitations = vec![
            "Captures X11/XWayland windows only; native Wayland windows cannot be upscaled.".to_string(),
            "Shows the result in a separate overlay window; keyboard input is not forwarded, so keep the game window focused.".into(),
            "Requires a Vulkan-capable GPU; upscales 2x per pass (4x with double upscale) to fit the monitor.".into(),
            "Does not combine with gamescope on Wayland (gamescope's window is a native Wayland window).".into(),
        ];
        let session = crate::display::session_type(probe);
        if session == DisplaySessionType::Wayland && !crate::display::xwayland_available(probe) {
            limitations.push("XWayland is not available in this session, so there is nothing RT-Upscaler can capture.".into());
        }
        if session == DisplaySessionType::Unknown {
            limitations.push("No graphical session detected.".into());
        }
        if let Some(foreign) = &inst.foreign_executable {
            limitations.push(format!("'{}' exists but is not linux-rt-upscaler; it will not be used.", foreign.display()));
        }
        if inst.executable.is_some() && model_dir.is_none() {
            limitations.push("Model directory not found; showing the model list known from linux-rt-upscaler 1.1.4.".into());
        }
        if inst.executable.is_none() {
            limitations.push(format!("Not installed. Install with: {RT_INSTALL_HINT}"));
        }
        if !xcb_cursor_present(probe) {
            limitations.push("libxcb-cursor (xcb-util-cursor / libxcb-cursor0) not found: the Qt XCB plugin RT-Upscaler needs may fail to load.".into());
        }
        UpscalerInfo {
            available: inst.executable.is_some(),
            tool: inst.executable.clone().map(|path| ToolInfo { path, version: inst.version.clone() }),
            models,
            presets,
            limitations,
            backend: Self::ID.into(),
            model_dir,
            config_path,
        }
    }

    fn plan(&self, probe: &Probe, config: &UpscalerConfig, ctx: &UpscalerContext<'_>) -> Result<UpscalerPlan, AppError> {
        let mut plan = UpscalerPlan::default();
        let inst = Self::locate(probe);
        let program = match &inst.executable {
            Some(p) => p.clone(),
            None if ctx.lenient => {
                plan.warnings.push(format!("RT-Upscaler is not installed ({RT_INSTALL_HINT}); launching will fail."));
                PathBuf::from("upscale")
            }
            None => {
                return Err(AppError::new(ErrorCode::MissingDependency, "RT-Upscaler (linux-rt-upscaler) is not installed")
                    .with_details(format!("Install it with `{RT_INSTALL_HINT}` or disable the upscaler for this game.")));
            }
        };

        let mut args: Vec<String> = Vec::new();
        if let Some(model) = config.model.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let (models, _) = Self::models(&inst);
            if !models.iter().any(|m| m.id == model) {
                return Err(AppError::invalid(format!(
                    "Unknown RT-Upscaler model '{model}' (available: {})",
                    models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")
                )));
            }
            args.extend(["-m".into(), model.to_string()]);
        }
        if let Some(scale) = config.scale {
            if !(scale.is_finite() && scale > 0.0) {
                return Err(AppError::invalid(format!("Invalid upscaler scale {scale}")));
            }
            let four = scale > 3.0;
            if (scale - 2.0).abs() > 0.01 && (scale - 4.0).abs() > 0.01 {
                plan.warnings.push(format!(
                    "RT-Upscaler supports 2x or 4x (double 2x pass) only; scale {scale} is treated as {}x. The result is then fitted to the monitor.",
                    if four { 4 } else { 2 }
                ));
            }
            if four {
                args.push("-2".into());
            }
        }
        if let Some(preset) = config.preset.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let known = Self::config_path(probe)
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|y| parse_profiles(&y))
                .unwrap_or_default();
            if !known.iter().any(|k| k.eq_ignore_ascii_case(preset)) {
                plan.warnings.push(format!("RT-Upscaler profile '{preset}' was not found in its config.yaml."));
            }
            args.extend(["-p".into(), preset.to_string()]);
        }
        if config.extra_args.iter().any(|a| a == "--") {
            return Err(AppError::invalid(
                "Upscaler extra arguments must not contain '--' (that would make RT-Upscaler launch a second program)",
            ));
        }
        let has_target = config.extra_args.iter().any(|a| {
            TARGET_FLAGS.contains(&a.as_str()) || TARGET_FLAGS.iter().any(|f| f.starts_with("--") && a.starts_with(&format!("{f}=")))
        });
        if !has_target {
            args.extend(["--target-delay".into(), RT_DEFAULT_TARGET_DELAY.to_string()]);
            plan.warnings.push(format!(
                "RT-Upscaler will attach to whichever window is focused {RT_DEFAULT_TARGET_DELAY} s after launch. Keep the game focused, or add `-t <window title>` to the upscaler extra arguments."
            ));
        }
        args.extend(config.extra_args.iter().cloned());

        match ctx.session {
            DisplaySessionType::Wayland if !ctx.xwayland => plan.warnings.push(
                "No XWayland display (DISPLAY) in this Wayland session: RT-Upscaler cannot capture anything.".into(),
            ),
            DisplaySessionType::Unknown => plan.warnings.push("No graphical session detected for RT-Upscaler.".into()),
            _ => {}
        }
        let wine_wayland = ctx.game_env.get("PROTON_ENABLE_WAYLAND").is_some_and(|v| v == "1")
            || ctx.game_env.get("DISPLAY").is_some_and(|v| v.is_empty());
        if wine_wayland {
            plan.warnings.push("The game is configured to use Wine's Wayland driver; RT-Upscaler can only capture X11/XWayland windows.".into());
        }
        if ctx.gamescope_enabled {
            plan.warnings.push(if ctx.session == DisplaySessionType::Wayland {
                "gamescope and RT-Upscaler are both enabled: on Wayland gamescope's window is a native Wayland window that RT-Upscaler cannot capture. Disable one of them.".into()
            } else {
                "gamescope and RT-Upscaler are both enabled: the image would be scaled twice. Disable one of them.".into()
            });
        }

        let mut spec = CommandSpec::new(program).args(args);
        spec.cwd = None;
        plan.companions.push(spec);
        Ok(plan)
    }
}

fn xcb_cursor_present(probe: &Probe) -> bool {
    ["/usr/lib", "/usr/lib64", "/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu"].iter().any(|d| {
        let dir = probe.sys_path(d);
        dir.join("libxcb-cursor.so.0").exists() || is_executable(&dir.join("libxcb-cursor.so.0"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::script;
    use std::collections::BTreeMap;

    struct Fx {
        _dir: tempfile::TempDir,
        probe: Probe,
        venv: PathBuf,
    }

    /// Fake pipx install: ~/.local/share/pipx/venvs/linux-rt-upscaler with the
    /// real console-script layout and model directory layout.
    fn fixture(installed: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let venv = home.join(".local/share/pipx/venvs/linux-rt-upscaler");
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        if installed {
            let site = venv.join("lib/python3.13/site-packages");
            for m in ["fast", "4x24", "veryfast", "8x32", ".originals"] {
                std::fs::create_dir_all(site.join("upscaler/srcnn/cunny").join(m)).unwrap();
            }
            std::fs::create_dir_all(site.join("linux_rt_upscaler-1.1.4.dist-info")).unwrap();
            std::fs::create_dir_all(venv.join("bin")).unwrap();
            let body = format!(
                "#!{}/bin/python\n# -*- coding: utf-8 -*-\nimport re\nimport sys\nfrom upscaler.cli import main\nif __name__ == '__main__':\n    sys.exit(main())\n",
                venv.display()
            );
            std::fs::write(venv.join("bin/upscale"), &body).unwrap();
            std::fs::write(bin.join("upscale"), &body).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(bin.join("upscale"), std::fs::Permissions::from_mode(0o755)).unwrap();
            let cfg = home.join(".config/linux-rt-upscaler");
            std::fs::create_dir_all(&cfg).unwrap();
            std::fs::write(
                cfg.join("config.yaml"),
                "# example\nmodel: fast\nprofiles:\n  manual:\n    options:\n      model: 4x24\n  \"vlc_player\":\n    match:\n      title: VLC media player\n    options:\n      crop_top: 2\n  720p:\n    match:\n      height: 720\nhotkeys:\n  toggle: ctrl+alt+u\n",
            )
            .unwrap();
        }
        let env: BTreeMap<String, String> = [
            ("HOME".to_string(), home.display().to_string()),
            ("PATH".to_string(), bin.display().to_string()),
            ("XDG_SESSION_TYPE".to_string(), "wayland".to_string()),
            ("DISPLAY".to_string(), ":1".to_string()),
        ]
        .into();
        let mut probe = Probe::fixture(dir.path().join("root"), env);
        probe.run_commands = false;
        Fx { _dir: dir, probe, venv }
    }

    fn ctx(env: &BTreeMap<String, String>) -> UpscalerContext<'_> {
        UpscalerContext { session: DisplaySessionType::Wayland, xwayland: true, gamescope_enabled: false, game_env: env, lenient: false }
    }

    #[test]
    fn profile_parsing() {
        let y = "profiles:\n  manual:\n    options:\n      model: 4x24\n  wide_firefox:\n    match: {title_contains: Firefox}\n# c\nother:\n  a: 1\n";
        assert_eq!(parse_profiles(y), vec!["manual", "wide_firefox"]);
        assert!(parse_profiles("model: fast\n").is_empty());
    }

    #[test]
    fn detects_install_models_presets() {
        let fx = fixture(true);
        let info = RtUpscaler.info(&fx.probe);
        assert!(info.available);
        let tool = info.tool.unwrap();
        assert!(tool.path.ends_with(".local/bin/upscale"));
        assert_eq!(tool.version.as_deref(), Some("1.1.4"));
        let ids: Vec<_> = info.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["none", "veryfast", "fast", "4x24", "8x32"]);
        assert_eq!(info.model_dir.unwrap(), fx.venv.join("lib/python3.13/site-packages/upscaler/srcnn/cunny"));
        assert_eq!(info.presets, vec!["manual", "vlc_player", "720p"]);
        assert!(info.limitations.iter().any(|l| l.contains("X11/XWayland")));
    }

    #[test]
    fn not_installed() {
        let fx = fixture(false);
        let info = RtUpscaler.info(&fx.probe);
        assert!(!info.available);
        assert_eq!(info.models.len(), RT_MODELS.len());
        assert!(info.models.iter().all(|m| m.path.is_none()));
        assert!(info.limitations.iter().any(|l| l.contains("pipx install linux-rt-upscaler")));
        let env = BTreeMap::new();
        let e = RtUpscaler.plan(&fx.probe, &UpscalerConfig { enabled: true, ..Default::default() }, &ctx(&env)).unwrap_err();
        assert_eq!(e.code, ErrorCode::MissingDependency);
        let lenient = UpscalerContext { lenient: true, ..ctx(&env) };
        let p = RtUpscaler.plan(&fx.probe, &UpscalerConfig { enabled: true, ..Default::default() }, &lenient).unwrap();
        assert!(p.warnings[0].contains("not installed"));
    }

    #[test]
    fn foreign_upscale_binary_is_ignored() {
        let fx = fixture(false);
        let bin = PathBuf::from(fx.probe.var("PATH").unwrap());
        script(&bin, "upscale", "echo 'some other image upscaler'");
        let info = RtUpscaler.info(&fx.probe);
        assert!(!info.available);
        assert!(info.limitations.iter().any(|l| l.contains("is not linux-rt-upscaler")));
    }

    #[test]
    fn companion_command() {
        let fx = fixture(true);
        let env = BTreeMap::new();
        let cfg = UpscalerConfig {
            enabled: true,
            backend: Some("rt-upscaler".into()),
            model: Some("4x24".into()),
            preset: Some("manual".into()),
            scale: Some(4.0),
            extra_args: vec![],
        };
        let plan = RtUpscaler.plan(&fx.probe, &cfg, &ctx(&env)).unwrap();
        let c = &plan.companions[0];
        assert!(c.program.ends_with(".local/bin/upscale"));
        assert_eq!(c.args, ["-m", "4x24", "-2", "-p", "manual", "--target-delay", "10"]);
        assert_eq!(plan.warnings.len(), 1, "{:?}", plan.warnings);

        // Explicit target: no delay, title passed through as one argv entry.
        let cfg = UpscalerConfig { extra_args: vec!["-t".into(), "ゲーム タイトル".into()], scale: None, preset: None, ..cfg };
        let plan = RtUpscaler.plan(&fx.probe, &cfg, &ctx(&env)).unwrap();
        assert_eq!(plan.companions[0].args, ["-m", "4x24", "-t", "ゲーム タイトル"]);
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn companion_validation_and_warnings() {
        let fx = fixture(true);
        let env = BTreeMap::new();
        let bad = UpscalerConfig { enabled: true, model: Some("4x99".into()), ..Default::default() };
        assert!(RtUpscaler.plan(&fx.probe, &bad, &ctx(&env)).unwrap_err().message.contains("available: none, veryfast"));
        let sep = UpscalerConfig { enabled: true, extra_args: vec!["--".into(), "x".into()], ..Default::default() };
        assert!(RtUpscaler.plan(&fx.probe, &sep, &ctx(&env)).is_err());

        let odd = UpscalerConfig { enabled: true, scale: Some(3.0), preset: Some("nope".into()), ..Default::default() };
        let wl: BTreeMap<String, String> = [("PROTON_ENABLE_WAYLAND".to_string(), "1".to_string())].into();
        let c = UpscalerContext { gamescope_enabled: true, ..ctx(&wl) };
        let plan = RtUpscaler.plan(&fx.probe, &odd, &c).unwrap();
        let all = plan.warnings.join("\n");
        assert!(all.contains("treated as 2x"));
        assert!(all.contains("'nope' was not found"));
        assert!(all.contains("Wine's Wayland driver"));
        assert!(all.contains("gamescope and RT-Upscaler"));
        assert!(!plan.companions[0].args.contains(&"-2".to_string()));
    }

    #[test]
    fn unknown_backend() {
        assert_eq!(backend(Some("magpie")).err().unwrap().code, ErrorCode::Unsupported);
        assert_eq!(backend(None).unwrap().id(), "rt-upscaler");
    }
}
