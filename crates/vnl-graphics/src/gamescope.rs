//! gamescope argv construction and validation.
//!
//! Every flag used here was verified against the installed binary
//! (`gamescope --help`, gamescope 3.17.0):
//!
//! | config                | flag                         | help section        |
//! |-----------------------|------------------------------|---------------------|
//! | game resolution       | `-w` / `-h`                  | `--nested-width/-height` |
//! | output resolution     | `-W` / `-H`                  | `--output-width/-height` |
//! | fps limit             | `-r`                         | `--nested-refresh`  |
//! | unfocused fps limit   | `-o`                         | `--nested-unfocused-refresh` (nested mode) |
//! | fullscreen            | `-f`                         | nested mode         |
//! | borderless            | `-b`                         | nested mode         |
//! | scaler                | `-S auto/integer/fit/fill/stretch` |               |
//! | filter                | `-F linear/nearest/fsr/nis/pixel` (3.17 also has `sgsr`) | |
//! | sharpness             | `--sharpness 0..=20` (alias `--fsr-sharpness`; 0 = max) | |
//! | preferred output      | `--prefer-output` (alias `-O`) | **embedded mode** only |
//! | force grab cursor     | `--force-grab-cursor`        | nested mode         |
//! | HDR                   | `--hdr-enabled`              | needs Gamescope WSI layer for clients |
//!
//! gamescope rejects unknown options (`unrecognized option`, exit 1) and
//! invalid enum values (`invalid value for --scaler`, exit 1), but silently
//! accepts an out-of-range `--sharpness`, so the range is validated here.
//! Note: gamescope exits 0 even when the child exits non-zero
//! (observed with 3.17.0), so session exit codes are those of gamescope.
//!
//! Defaults (`-S auto`, `-F linear`, windowed) are not emitted to keep the
//! command line minimal.

use vnl_core::{AppError, GamescopeConfig, GamescopeFilter, GamescopeScaler, WindowMode};

use crate::display::DisplaySessionType;

pub const SHARPNESS_MAX: u8 = 20;
const MAX_DIMENSION: u32 = 16_384;

/// Environment facts that influence validation warnings.
#[derive(Debug, Clone)]
pub struct GamescopeContext {
    pub session: DisplaySessionType,
    /// A desktop session is running, so gamescope runs as a nested window.
    pub nested: bool,
    /// All DRM connector names (connected or not), e.g. `DP-1`.
    pub connectors: Vec<String>,
    /// Connected connector names.
    pub connected: Vec<String>,
    /// Gamescope WSI Vulkan layer manifest found.
    pub wsi_layer: bool,
    pub desktop: Option<String>,
}

impl Default for GamescopeContext {
    fn default() -> Self {
        Self {
            session: DisplaySessionType::Wayland,
            nested: true,
            connectors: Vec::new(),
            connected: Vec::new(),
            wsi_layer: true,
            desktop: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GamescopeArgs {
    /// Wrapper arguments, always ending with `--`.
    pub args: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn scaler_value(s: GamescopeScaler) -> &'static str {
    match s {
        GamescopeScaler::Auto => "auto",
        GamescopeScaler::Integer => "integer",
        GamescopeScaler::Fit => "fit",
        GamescopeScaler::Fill => "fill",
        GamescopeScaler::Stretch => "stretch",
    }
}

pub fn filter_value(f: GamescopeFilter) -> &'static str {
    match f {
        GamescopeFilter::Linear => "linear",
        GamescopeFilter::Nearest => "nearest",
        GamescopeFilter::Fsr => "fsr",
        GamescopeFilter::Nis => "nis",
        GamescopeFilter::Pixel => "pixel",
        GamescopeFilter::Sgsr => "sgsr",
        GamescopeFilter::Anime4k => "anime4k",
    }
}

/// Notes shown when the Anime4K filter is selected.
pub fn anime4k_warnings(cfg: &GamescopeConfig, nvidia_without_fix: bool) -> Vec<String> {
    let mut w = vec![
        "Anime4K (2x CNN UL) is the heaviest gamescope filter: it runs a neural network on the GPU every frame."
            .to_string(),
    ];
    if nvidia_without_fix {
        w.push(
            "Known issue: the gamescope-anime4k fork renders a black image on NVIDIA unless it contains the uniform-buffer alignment fix (it binds constants at 16-byte offsets, NVIDIA needs 64). The Anime4K build made by the launcher includes the fix; other builds may not. If the screen is black, rebuild from Settings > Gamescope or switch the filter to FSR or Linear."
                .to_string(),
        );
    }
    match (cfg.game_width, cfg.game_height, cfg.output_width, cfg.output_height) {
        (Some(gw), Some(gh), Some(ow), Some(oh)) if ow <= gw && oh <= gh => w.push(
            "Anime4K only has an effect when the output resolution is larger than the game resolution.".to_string(),
        ),
        (None, _, _, _) | (_, None, _, _) => w.push(
            "Set the game resolution (the game's native size) so gamescope knows how much to upscale.".to_string(),
        ),
        _ => {}
    }
    w
}

/// Filters that only change anything when gamescope actually upscales.
pub fn is_upscaling_filter(f: GamescopeFilter) -> bool {
    matches!(
        f,
        GamescopeFilter::Fsr
            | GamescopeFilter::Nis
            | GamescopeFilter::Pixel
            | GamescopeFilter::Sgsr
            | GamescopeFilter::Anime4k
    )
}

fn pair(name: &str, w: Option<u32>, h: Option<u32>) -> Result<Option<(u32, u32)>, AppError> {
    match (w, h) {
        (None, None) => Ok(None),
        (Some(w), Some(h)) => {
            for (axis, v) in [("width", w), ("height", h)] {
                if v == 0 || v > MAX_DIMENSION {
                    return Err(AppError::invalid(format!(
                        "gamescope {name} {axis} must be between 1 and {MAX_DIMENSION} (got {v})"
                    )));
                }
            }
            Ok(Some((w, h)))
        }
        _ => Err(AppError::invalid(format!("gamescope {name} resolution needs both width and height"))),
    }
}

fn valid_connector_list(s: &str) -> bool {
    !s.is_empty()
        && s.split(',')
            .all(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '*'))
}

/// Build the gamescope wrapper arguments (without the program name).
pub fn build_args(cfg: &GamescopeConfig, ctx: &GamescopeContext) -> Result<GamescopeArgs, AppError> {
    let mut args: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let game = pair("game", cfg.game_width, cfg.game_height)?;
    let output = pair("output", cfg.output_width, cfg.output_height)?;

    if let Some((w, h)) = game {
        args.extend(["-w".into(), w.to_string(), "-h".into(), h.to_string()]);
    }
    if let Some((w, h)) = output {
        args.extend(["-W".into(), w.to_string(), "-H".into(), h.to_string()]);
    }
    for (flag, label, v) in [("-r", "fps limit", cfg.fps_limit), ("-o", "unfocused fps limit", cfg.fps_limit_unfocused)]
    {
        if let Some(v) = v {
            if v == 0 || v > 1000 {
                return Err(AppError::invalid(format!("gamescope {label} must be between 1 and 1000 (got {v})")));
            }
            args.extend([flag.to_string(), v.to_string()]);
        }
    }
    if cfg.fps_limit_unfocused.is_some() && !ctx.nested {
        warnings.push("The unfocused fps limit (-o) only applies when gamescope runs nested inside a desktop.".into());
    }

    match cfg.window_mode {
        WindowMode::Windowed => {}
        WindowMode::Fullscreen => args.push("-f".into()),
        WindowMode::Borderless => args.push("-b".into()),
    }

    if cfg.scaler != GamescopeScaler::Auto {
        args.extend(["-S".into(), scaler_value(cfg.scaler).into()]);
    }
    if cfg.filter != GamescopeFilter::Linear {
        args.extend(["-F".into(), filter_value(cfg.filter).into()]);
    }

    if let Some(s) = cfg.sharpness {
        if s > SHARPNESS_MAX {
            return Err(AppError::invalid(format!(
                "gamescope sharpness must be between 0 (sharpest) and {SHARPNESS_MAX} (softest), got {s}"
            )));
        }
        if matches!(cfg.filter, GamescopeFilter::Fsr | GamescopeFilter::Nis) {
            args.extend(["--sharpness".into(), s.to_string()]);
        } else {
            warnings.push(format!(
                "Sharpness is only used by the FSR and NIS filters; it is ignored with the '{}' filter.",
                filter_value(cfg.filter)
            ));
        }
    }

    if let Some(out) = cfg.prefer_output.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !valid_connector_list(out) {
            return Err(AppError::invalid(format!(
                "Invalid output connector '{out}': expected a connector name such as DP-1 or HDMI-A-1"
            )));
        }
        args.extend(["--prefer-output".into(), out.to_string()]);
        if ctx.nested {
            warnings.push(format!(
                "--prefer-output {out} only applies when gamescope drives the display itself (embedded/DRM mode); in a desktop session the gamescope window opens on the compositor's output."
            ));
        }
        if !ctx.connectors.is_empty() {
            for c in out.split(',').filter(|c| !c.contains('*')) {
                if !ctx.connectors.iter().any(|k| k == c) {
                    warnings.push(format!(
                        "Output '{c}' does not exist on this system (connected: {}).",
                        if ctx.connected.is_empty() { "none".into() } else { ctx.connected.join(", ") }
                    ));
                } else if !ctx.connected.iter().any(|k| k == c) {
                    warnings.push(format!("Output '{c}' is not connected."));
                }
            }
        }
    }

    if cfg.force_grab_cursor {
        args.push("--force-grab-cursor".into());
    }

    if cfg.hdr {
        args.push("--hdr-enabled".into());
        match ctx.session {
            DisplaySessionType::Wayland if ctx.nested => warnings.push(format!(
                "HDR in nested gamescope needs a compositor with HDR enabled for this display{} and an HDR monitor; otherwise HDR games are tonemapped to SDR.",
                match ctx.desktop.as_deref() {
                    Some(d) if d.contains("GNOME") => " (GNOME 48+: Settings > Displays > HDR)",
                    Some(d) if d.contains("KDE") => " (KDE Plasma 6: Display Configuration > HDR)",
                    _ => "",
                }
            )),
            DisplaySessionType::X11 => warnings.push(
                "HDR is not available when gamescope runs nested in an X11 session; HDR content will be tonemapped to SDR. Use a Wayland session or embedded gamescope.".into(),
            ),
            _ => {}
        }
        if !ctx.wsi_layer {
            warnings.push(
                "Gamescope WSI Vulkan layer (VkLayer_FROG_gamescope_wsi) not found: games cannot output HDR inside gamescope.".into(),
            );
        }
    }

    // Upscaling sanity checks.
    if let (Some((gw, gh)), Some((ow, oh))) = (game, output)
        && cfg.scaler == GamescopeScaler::Integer
        && (ow < gw || oh < gh)
    {
        warnings.push(format!(
            "Integer scaling cannot shrink the image: output {ow}x{oh} is smaller than game resolution {gw}x{gh}. Use the fit/auto scaler or raise the output resolution."
        ));
    }
    if is_upscaling_filter(cfg.filter) {
        let f = filter_value(cfg.filter).to_uppercase();
        match (game, output) {
            (None, _) => warnings.push(format!(
                "The {f} filter only has an effect when the game resolution (-w/-h) is lower than the output resolution; set a game resolution."
            )),
            (Some((gw, gh)), Some((ow, oh))) if gw >= ow && gh >= oh => warnings.push(format!(
                "The {f} filter has no effect: game resolution {gw}x{gh} is not lower than output {ow}x{oh}."
            )),
            (Some(_), None) if cfg.window_mode != WindowMode::Fullscreen => warnings.push(format!(
                "The {f} filter has no effect in a window sized to the game resolution; set an output resolution (-W/-H) or use fullscreen."
            )),
            _ => {}
        }
    }

    for a in &cfg.extra_args {
        if a == "--" {
            return Err(AppError::invalid(
                "gamescope extra arguments must not contain '--' (Yomika adds it before the game command)",
            ));
        }
    }
    args.extend(cfg.extra_args.iter().cloned());
    args.push("--".into());
    Ok(GamescopeArgs { args, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> GamescopeConfig {
        GamescopeConfig { enabled: true, ..Default::default() }
    }

    fn ctx() -> GamescopeContext {
        GamescopeContext {
            connectors: vec!["DP-1".into(), "DP-8".into(), "HDMI-A-2".into()],
            connected: vec!["DP-8".into(), "HDMI-A-2".into()],
            ..Default::default()
        }
    }

    fn args(c: &GamescopeConfig) -> Vec<String> {
        build_args(c, &ctx()).unwrap().args
    }

    #[test]
    fn minimal_is_just_separator() {
        assert_eq!(args(&cfg()), vec!["--"]);
    }

    #[test]
    fn golden_full_config() {
        let c = GamescopeConfig {
            game_width: Some(1280),
            game_height: Some(720),
            output_width: Some(2560),
            output_height: Some(1440),
            window_mode: WindowMode::Fullscreen,
            scaler: GamescopeScaler::Fit,
            filter: GamescopeFilter::Fsr,
            sharpness: Some(5),
            fps_limit: Some(60),
            fps_limit_unfocused: Some(10),
            prefer_output: Some("DP-8".into()),
            force_grab_cursor: true,
            hdr: true,
            extra_args: vec!["--adaptive-sync".into()],
            ..cfg()
        };
        assert_eq!(
            args(&c),
            [
                "-w",
                "1280",
                "-h",
                "720",
                "-W",
                "2560",
                "-H",
                "1440",
                "-r",
                "60",
                "-o",
                "10",
                "-f",
                "-S",
                "fit",
                "-F",
                "fsr",
                "--sharpness",
                "5",
                "--prefer-output",
                "DP-8",
                "--force-grab-cursor",
                "--hdr-enabled",
                "--adaptive-sync",
                "--"
            ]
        );
    }

    #[test]
    fn golden_borderless_integer_nearest() {
        let c = GamescopeConfig {
            game_width: Some(800),
            game_height: Some(600),
            output_width: Some(1600),
            output_height: Some(1200),
            window_mode: WindowMode::Borderless,
            scaler: GamescopeScaler::Integer,
            filter: GamescopeFilter::Nearest,
            ..cfg()
        };
        let r = build_args(&c, &ctx()).unwrap();
        assert_eq!(
            r.args,
            ["-w", "800", "-h", "600", "-W", "1600", "-H", "1200", "-b", "-S", "integer", "-F", "nearest", "--"]
        );
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn golden_nis_stretch_windowed() {
        let c = GamescopeConfig {
            game_width: Some(1024),
            game_height: Some(768),
            output_width: Some(1920),
            output_height: Some(1080),
            scaler: GamescopeScaler::Stretch,
            filter: GamescopeFilter::Nis,
            sharpness: Some(0),
            ..cfg()
        };
        assert_eq!(
            args(&c),
            [
                "-w",
                "1024",
                "-h",
                "768",
                "-W",
                "1920",
                "-H",
                "1080",
                "-S",
                "stretch",
                "-F",
                "nis",
                "--sharpness",
                "0",
                "--"
            ]
        );
    }

    #[test]
    fn sharpness_validation() {
        let c = GamescopeConfig { sharpness: Some(21), filter: GamescopeFilter::Fsr, ..cfg() };
        let e = build_args(&c, &ctx()).unwrap_err();
        assert_eq!(e.code, vnl_core::ErrorCode::InvalidInput);
        assert!(e.message.contains("0 (sharpest) and 20"));
        let c = GamescopeConfig { sharpness: Some(20), filter: GamescopeFilter::Fsr, ..cfg() };
        assert!(build_args(&c, &ctx()).unwrap().args.contains(&"20".to_string()));
        // Sharpness without FSR/NIS: not emitted, warned.
        let c = GamescopeConfig { sharpness: Some(3), ..cfg() };
        let r = build_args(&c, &ctx()).unwrap();
        assert!(!r.args.contains(&"--sharpness".to_string()));
        assert!(r.warnings.iter().any(|w| w.contains("ignored")));
    }

    #[test]
    fn resolution_validation() {
        let c = GamescopeConfig { game_width: Some(1280), ..cfg() };
        assert!(build_args(&c, &ctx()).unwrap_err().message.contains("both width and height"));
        let c = GamescopeConfig { output_width: Some(0), output_height: Some(1080), ..cfg() };
        assert!(build_args(&c, &ctx()).is_err());
        let c = GamescopeConfig { fps_limit: Some(0), ..cfg() };
        assert!(build_args(&c, &ctx()).is_err());
    }

    #[test]
    fn integer_downscale_warns() {
        let c = GamescopeConfig {
            game_width: Some(1920),
            game_height: Some(1080),
            output_width: Some(1280),
            output_height: Some(720),
            scaler: GamescopeScaler::Integer,
            ..cfg()
        };
        let r = build_args(&c, &ctx()).unwrap();
        assert!(r.warnings.iter().any(|w| w.contains("Integer scaling cannot shrink")), "{:?}", r.warnings);
    }

    #[test]
    fn upscaling_filter_warnings() {
        let no_game = GamescopeConfig { filter: GamescopeFilter::Fsr, ..cfg() };
        assert!(build_args(&no_game, &ctx()).unwrap().warnings.iter().any(|w| w.contains("set a game resolution")));
        let same = GamescopeConfig {
            filter: GamescopeFilter::Nis,
            game_width: Some(1920),
            game_height: Some(1080),
            output_width: Some(1920),
            output_height: Some(1080),
            ..cfg()
        };
        assert!(build_args(&same, &ctx()).unwrap().warnings.iter().any(|w| w.contains("no effect")));
        let windowed =
            GamescopeConfig { filter: GamescopeFilter::Fsr, game_width: Some(1280), game_height: Some(720), ..cfg() };
        assert!(build_args(&windowed, &ctx()).unwrap().warnings.iter().any(|w| w.contains("-W/-H")));
        let fullscreen = GamescopeConfig { window_mode: WindowMode::Fullscreen, ..windowed };
        assert!(build_args(&fullscreen, &ctx()).unwrap().warnings.is_empty());
    }

    #[test]
    fn prefer_output_checks() {
        let c = GamescopeConfig { prefer_output: Some("DP 1; rm".into()), ..cfg() };
        assert!(build_args(&c, &ctx()).is_err());
        let c = GamescopeConfig { prefer_output: Some("DP-3".into()), ..cfg() };
        let r = build_args(&c, &ctx()).unwrap();
        assert!(r.warnings.iter().any(|w| w.contains("does not exist")));
        assert!(r.warnings.iter().any(|w| w.contains("embedded")));
        let c = GamescopeConfig { prefer_output: Some("DP-1".into()), ..cfg() };
        let embedded = GamescopeContext { nested: false, ..ctx() };
        let r = build_args(&c, &embedded).unwrap();
        assert_eq!(r.warnings, vec!["Output 'DP-1' is not connected."]);
        let c = GamescopeConfig { prefer_output: Some("HDMI-A-2,DP-8".into()), ..cfg() };
        assert!(build_args(&c, &embedded).unwrap().warnings.is_empty());
    }

    #[test]
    fn hdr_warnings() {
        let c = GamescopeConfig { hdr: true, ..cfg() };
        let x11 = GamescopeContext { session: DisplaySessionType::X11, wsi_layer: false, ..ctx() };
        let r = build_args(&c, &x11).unwrap();
        assert!(r.warnings.iter().any(|w| w.contains("X11")));
        assert!(r.warnings.iter().any(|w| w.contains("WSI")));
        let gnome = GamescopeContext { desktop: Some("GNOME".into()), ..ctx() };
        assert!(build_args(&c, &gnome).unwrap().warnings[0].contains("GNOME 48+"));
    }

    #[test]
    fn extra_args_cannot_contain_separator() {
        let c = GamescopeConfig { extra_args: vec!["--".into(), "evil".into()], ..cfg() };
        assert!(build_args(&c, &ctx()).is_err());
        let c = GamescopeConfig {
            extra_args: vec!["--mangoapp".into(), "--prefer-vk-device".into(), "10de:2d04".into()],
            ..cfg()
        };
        assert_eq!(args(&c), ["--mangoapp", "--prefer-vk-device", "10de:2d04", "--"]);
    }
}
