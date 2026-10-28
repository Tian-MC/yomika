//! Wine/Proton fullscreen FSR hack (`WINE_FULLSCREEN_FSR`).
//!
//! This is *not* gamescope upscaling. It lives in the fullscreen hack of
//! Proton-GE-style Wine builds: when a fullscreen game requests a mode below
//! the native resolution, Wine renders at that size and upscales with FSR 1.
//! It only applies to Vulkan presentation (DXVK / vkd3d-proton / native
//! Vulkan games) and to fullscreen modes below native resolution.
//!
//! Variables (GE-Proton release notes, e.g. GE-Proton7-29, and ValveSoftware
//! /wine PR #116):
//! * `WINE_FULLSCREEN_FSR=1` enables it.
//! * `WINE_FULLSCREEN_FSR_STRENGTH` = sharpening, 0 = maximum sharpness,
//!   higher = softer; default 2 (AMD's recommendation, per PR #116). GE docs
//!   give the range 0–5, which is what we accept.
//! * `WINE_FULLSCREEN_FSR_MODE` / `WINE_FULLSCREEN_FSR_CUSTOM_MODE` add extra
//!   fake resolutions to the game's list (not exposed by the config yet).
//!
//! Support detection is done on the runner's actual `win32u.so`: in
//! GE-Proton11-11 and Proton-CachyOS the strings `WINE_FULLSCREEN_FSR` /
//! `WINE_FULLSCREEN_FSR_STRENGTH` are present in
//! `files/lib/wine/x86_64-unix/win32u.so`; in Proton-TKG and Arch's
//! `wine 11.20` they are absent. When the library cannot be found we fall back
//! to a name heuristic.

use std::path::{Path, PathBuf};

use vnl_core::{AppError, CommandSpec, Runner, RunnerKind, WineFsrConfig};

pub const STRENGTH_MAX: u8 = 5;
const MARKER: &[u8] = b"WINE_FULLSCREEN_FSR";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsrSupport {
    /// The runner's win32u.so contains the FSR hack.
    Supported,
    /// The runner's win32u.so exists but does not contain it.
    Unsupported,
    /// Could not inspect the runner.
    Unknown,
}

/// Candidate `win32u.so` locations for a runner.
pub fn win32u_candidates(runner: &Runner) -> Vec<PathBuf> {
    let rel = ["lib/wine/x86_64-unix/win32u.so", "lib64/wine/x86_64-unix/win32u.so", "lib/x86_64-linux-gnu/wine/x86_64-unix/win32u.so"];
    let roots: Vec<PathBuf> = match runner.kind {
        RunnerKind::Proton => vec![runner.path.join("files"), runner.path.join("dist")],
        // `.../bin/wine` -> `...`
        RunnerKind::Wine => runner.path.parent().and_then(Path::parent).map(|p| vec![p.to_path_buf()]).unwrap_or_default(),
        RunnerKind::Native => Vec::new(),
    };
    roots.iter().flat_map(|r| rel.iter().map(move |p| r.join(p))).collect()
}

pub fn detect_support(runner: &Runner) -> FsrSupport {
    for candidate in win32u_candidates(runner) {
        if let Ok(bytes) = std::fs::read(&candidate) {
            return if bytes.windows(MARKER.len()).any(|w| w == MARKER) {
                FsrSupport::Supported
            } else {
                FsrSupport::Unsupported
            };
        }
    }
    FsrSupport::Unknown
}

/// Name/version heuristic for builds we could not inspect.
pub fn looks_like_ge(runner: &Runner) -> bool {
    let hay = format!("{} {} {}", runner.id, runner.name, runner.version.as_deref().unwrap_or("")).to_ascii_lowercase();
    hay.contains("ge-proton") || hay.contains("proton-ge") || hay.contains("wine-ge") || hay.contains("cachyos")
}

#[derive(Debug, Clone, Default)]
pub struct WineFsrResult {
    pub warnings: Vec<String>,
}

/// Add the Wine FSR variables to `command` (if enabled) and report problems.
/// `runner` may be `None` for previews without a runner.
pub fn apply(cfg: &WineFsrConfig, command: &mut CommandSpec, runner: Option<&Runner>) -> Result<WineFsrResult, AppError> {
    let mut out = WineFsrResult::default();
    if !cfg.enabled {
        return Ok(out);
    }
    if let Some(s) = cfg.strength
        && s > STRENGTH_MAX
    {
        return Err(AppError::invalid(format!(
            "Wine FSR strength must be between 0 (sharpest) and {STRENGTH_MAX} (softest), got {s}"
        )));
    }
    command.env.insert("WINE_FULLSCREEN_FSR".into(), "1".into());
    if let Some(s) = cfg.strength {
        command.env.insert("WINE_FULLSCREEN_FSR_STRENGTH".into(), s.to_string());
    }

    match runner {
        None => out.warnings.push("No runner selected: cannot check whether it supports Wine FSR (Proton-GE builds only).".into()),
        Some(r) if r.kind == RunnerKind::Native => out
            .warnings
            .push("Wine FSR has no effect on native Linux games (it is part of Proton-GE's Wine).".into()),
        Some(r) => match detect_support(r) {
            FsrSupport::Supported => {}
            FsrSupport::Unsupported => out.warnings.push(format!(
                "Runner '{}' does not include the Wine fullscreen FSR hack (no WINE_FULLSCREEN_FSR in its win32u.so); Wine FSR will have no effect. Use a GE-Proton build or gamescope's FSR filter.",
                r.name
            )),
            FsrSupport::Unknown if looks_like_ge(r) => {}
            FsrSupport::Unknown => out.warnings.push(format!(
                "Runner '{}' does not look like a Proton-GE build; Wine FSR (WINE_FULLSCREEN_FSR) only works with Proton-GE-style builds.",
                r.name
            )),
        },
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vnl_core::RunnerSource;

    pub(crate) fn proton_runner(dir: &Path, name: &str, with_fsr: Option<bool>) -> Runner {
        if let Some(fsr) = with_fsr {
            let lib = dir.join("files/lib/wine/x86_64-unix");
            std::fs::create_dir_all(&lib).unwrap();
            let body: &[u8] = if fsr { b"\x7fELF....WINE_FULLSCREEN_FSR_STRENGTH\0..." } else { b"\x7fELF....nothing\0" };
            std::fs::write(lib.join("win32u.so"), body).unwrap();
        }
        Runner {
            id: format!("proton:{name}"),
            name: name.into(),
            kind: RunnerKind::Proton,
            source: RunnerSource::Steam,
            path: dir.to_path_buf(),
            version: None,
        }
    }

    #[test]
    fn env_and_strength() {
        let dir = tempfile::tempdir().unwrap();
        let r = proton_runner(dir.path(), "GE-Proton11-11", Some(true));
        let mut cmd = CommandSpec::new("/p/proton");
        let res = apply(&WineFsrConfig { enabled: true, strength: Some(3) }, &mut cmd, Some(&r)).unwrap();
        assert_eq!(cmd.env.get("WINE_FULLSCREEN_FSR").unwrap(), "1");
        assert_eq!(cmd.env.get("WINE_FULLSCREEN_FSR_STRENGTH").unwrap(), "3");
        assert!(res.warnings.is_empty());

        let mut cmd = CommandSpec::new("/p/proton");
        apply(&WineFsrConfig { enabled: true, strength: None }, &mut cmd, Some(&r)).unwrap();
        assert!(!cmd.env.contains_key("WINE_FULLSCREEN_FSR_STRENGTH"));

        let e = apply(&WineFsrConfig { enabled: true, strength: Some(6) }, &mut CommandSpec::new("x"), Some(&r)).unwrap_err();
        assert!(e.message.contains("between 0"));

        let mut cmd = CommandSpec::new("x");
        apply(&WineFsrConfig { enabled: false, strength: Some(9) }, &mut cmd, Some(&r)).unwrap();
        assert!(cmd.env.is_empty());
    }

    #[test]
    fn runner_support_detection() {
        let dir = tempfile::tempdir().unwrap();
        let tkg = proton_runner(dir.path(), "Proton-Tkg-928", Some(false));
        assert_eq!(detect_support(&tkg), FsrSupport::Unsupported);
        let w = apply(&WineFsrConfig { enabled: true, strength: None }, &mut CommandSpec::new("x"), Some(&tkg)).unwrap();
        assert!(w.warnings[0].contains("does not include"));

        let unknown_dir = tempfile::tempdir().unwrap();
        let ge = proton_runner(unknown_dir.path(), "GE-Proton9-20", None);
        assert_eq!(detect_support(&ge), FsrSupport::Unknown);
        assert!(apply(&WineFsrConfig { enabled: true, strength: None }, &mut CommandSpec::new("x"), Some(&ge)).unwrap().warnings.is_empty());

        let valve = proton_runner(unknown_dir.path(), "Proton 9.0", None);
        let w = apply(&WineFsrConfig { enabled: true, strength: None }, &mut CommandSpec::new("x"), Some(&valve)).unwrap();
        assert!(w.warnings[0].contains("does not look like a Proton-GE"));
    }

    #[test]
    fn wine_runner_lib_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let lib = dir.path().join("lib/wine/x86_64-unix");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("win32u.so"), b"plain wine").unwrap();
        let r = Runner {
            id: "wine:system".into(),
            name: "Wine 11.20".into(),
            kind: RunnerKind::Wine,
            source: RunnerSource::System,
            path: bin.join("wine"),
            version: Some("11.20".into()),
        };
        assert_eq!(detect_support(&r), FsrSupport::Unsupported);
    }
}
