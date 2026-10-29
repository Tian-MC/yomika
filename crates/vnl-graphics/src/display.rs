//! Display/GPU capability detection.
//!
//! * Session type: `XDG_SESSION_TYPE`, falling back to `WAYLAND_DISPLAY` / `DISPLAY`.
//! * Monitors: `/sys/class/drm/card<N>-<connector>/{status,modes,edid}`. The
//!   connector part (`DP-1`, `HDMI-A-1`, `eDP-1`) is the kernel connector name,
//!   which is what gamescope's `--prefer-output` matches: gamescope's DRM
//!   backend names connectors with libdrm's `drmModeGetConnectorTypeName()` +
//!   `connector_type_id` (verified: gamescope 3.17 links `libdrm.so.2` and
//!   imports `drmModeGetConnectorTypeName`; libdrm's table mirrors the kernel's
//!   `drm_connector_enum_list`, e.g. `HDMI-A`). Note that `--prefer-output` is
//!   listed under "Embedded mode options" in `gamescope --help` — it only
//!   matters when gamescope drives the display itself (DRM backend, no desktop).
//! * Refresh rate: sysfs `modes` has no refresh rate, so it is computed from
//!   the EDID's first detailed timing descriptor (the preferred mode).
//! * GPUs: `/sys/class/drm/card<N>/device/{vendor,device,driver}` + `lspci -mm`
//!   names when available + `vulkaninfo --summary` when available.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::probe::{Probe, ToolInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplaySessionType {
    Wayland,
    X11,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    /// Connector, e.g. `DP-1` (usable with gamescope `--prefer-output`).
    pub name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub refresh_hz: Option<f64>,
    /// Only known when exactly one monitor is connected (sysfs has no notion
    /// of a "primary" output; that is compositor state).
    pub primary: bool,
    /// Monitor model from the EDID, e.g. `Odyssey G40B`.
    pub model: Option<String>,
    /// DRM card, e.g. `card1`.
    pub card: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub card: String,
    pub pci_slot: Option<String>,
    /// `0x10de` -> `10de`
    pub vendor_id: Option<String>,
    pub device_id: Option<String>,
    pub driver: Option<String>,
    /// From `lspci -mm`, e.g. `NVIDIA Corporation GB206 [GeForce RTX 5060 Ti]`.
    pub name: Option<String>,
}

impl GpuInfo {
    /// `vendor:device` as accepted by gamescope `--prefer-vk-device`.
    pub fn vk_device_id(&self) -> Option<String> {
        Some(format!("{}:{}", self.vendor_id.as_ref()?, self.device_id.as_ref()?))
    }

    pub fn label(&self) -> String {
        let name = self.name.clone().unwrap_or_else(|| match self.vendor_id.as_deref() {
            Some("10de") => "NVIDIA GPU".into(),
            Some("1002") => "AMD GPU".into(),
            Some("8086") => "Intel GPU".into(),
            _ => "GPU".into(),
        });
        let mut extra = Vec::new();
        if let Some(id) = self.vk_device_id() {
            extra.push(id);
        }
        if let Some(driver) = &self.driver {
            extra.push(format!("driver {driver}"));
        }
        extra.push(self.card.clone());
        format!("{name} ({})", extra.join(", "))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VulkanSummary {
    pub instance_version: Option<String>,
    pub devices: Vec<String>,
}

impl VulkanSummary {
    pub fn describe(&self) -> String {
        let mut s = match &self.instance_version {
            Some(v) => format!("Vulkan {v}"),
            None => "Vulkan".to_string(),
        };
        if !self.devices.is_empty() {
            s.push_str(&format!(": {}", self.devices.join("; ")));
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsCapabilities {
    pub session_type: DisplaySessionType,
    pub xwayland: bool,
    pub desktop: Option<String>,
    pub gamescope: Option<ToolInfo>,
    pub rt_upscaler: Option<ToolInfo>,
    pub monitors: Vec<MonitorInfo>,
    pub gpus: Vec<String>,
    pub notes: Vec<String>,
    /// Structured GPU data (ids usable with `--prefer-vk-device`).
    pub gpu_details: Vec<GpuInfo>,
    pub gamescope_wsi_layer: bool,
    pub inside_flatpak: bool,
}

pub fn session_type(probe: &Probe) -> DisplaySessionType {
    match probe.var("XDG_SESSION_TYPE").map(str::to_ascii_lowercase).as_deref() {
        Some("wayland") => return DisplaySessionType::Wayland,
        Some("x11") => return DisplaySessionType::X11,
        _ => {}
    }
    if probe.var("WAYLAND_DISPLAY").is_some() {
        DisplaySessionType::Wayland
    } else if probe.var("DISPLAY").is_some() {
        DisplaySessionType::X11
    } else {
        DisplaySessionType::Unknown
    }
}

/// True when an X server is reachable from a Wayland session (XWayland).
pub fn xwayland_available(probe: &Probe) -> bool {
    session_type(probe) == DisplaySessionType::Wayland && probe.var("DISPLAY").is_some()
}

/// A desktop compositor/X server is running, so gamescope will run nested
/// (as a window) instead of driving the display via DRM.
pub fn nested_session(probe: &Probe) -> bool {
    probe.var("WAYLAND_DISPLAY").is_some() || probe.var("DISPLAY").is_some()
}

pub fn desktop(probe: &Probe) -> Option<String> {
    probe.var("XDG_CURRENT_DESKTOP").or_else(|| probe.var("DESKTOP_SESSION")).map(str::to_string)
}

/// Every DRM connector (connected or not) under `/sys/class/drm`.
pub fn connectors(probe: &Probe) -> Vec<(String, String, bool)> {
    let dir = probe.sys_path("/sys/class/drm");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else { return out };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Some((card, connector)) = split_connector(&file_name) else { continue };
        if connector.starts_with("Writeback") {
            continue;
        }
        let status = std::fs::read_to_string(entry.path().join("status")).unwrap_or_default();
        out.push((card.to_string(), connector.to_string(), status.trim() == "connected"));
    }
    out.sort_by_key(|a| natural_key(&a.0, &a.1));
    out
}

fn natural_key(card: &str, conn: &str) -> (u32, String, u32) {
    let num = |s: &str| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
    let (kind, idx) = conn.rsplit_once('-').unwrap_or((conn, "0"));
    (num(card), kind.to_string(), idx.parse().unwrap_or(0))
}

/// `card1-DP-1` -> (`card1`, `DP-1`). Plain `card1` / `renderD128` -> None.
pub fn split_connector(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("card")?;
    let dash = rest.find('-')?;
    if dash == 0 || !rest[..dash].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((&name[..4 + dash], &rest[dash + 1..]))
}

pub fn monitors(probe: &Probe) -> Vec<MonitorInfo> {
    let dir = probe.sys_path("/sys/class/drm");
    let mut monitors = Vec::new();
    for (card, connector, connected) in connectors(probe) {
        if !connected {
            continue;
        }
        let base = dir.join(format!("{card}-{connector}"));
        let modes = std::fs::read_to_string(base.join("modes")).unwrap_or_default();
        let edid = std::fs::read(base.join("edid")).ok().and_then(|b| parse_edid(&b));
        let (mut width, mut height) =
            modes.lines().next().and_then(parse_mode).map_or((None, None), |(w, h)| (Some(w), Some(h)));
        if width.is_none()
            && let Some(e) = &edid
        {
            width = Some(e.width);
            height = Some(e.height);
        }
        // Only trust the EDID refresh when it describes the same mode.
        let refresh_hz = edid
            .as_ref()
            .filter(|e| width.is_none_or(|w| w == e.width) && height.is_none_or(|h| h == e.height))
            .map(|e| e.refresh_hz);
        monitors.push(MonitorInfo {
            name: connector,
            width,
            height,
            refresh_hz,
            primary: false,
            model: edid.and_then(|e| e.model),
            card: Some(card),
        });
    }
    if monitors.len() == 1 {
        monitors[0].primary = true;
    }
    monitors
}

/// `1920x1080` / `1920x1080i` -> (1920, 1080)
pub fn parse_mode(line: &str) -> Option<(u32, u32)> {
    let (w, h) = line.trim().split_once('x')?;
    let h: String = h.chars().take_while(|c| c.is_ascii_digit()).collect();
    Some((w.parse().ok()?, h.parse().ok()?))
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdidInfo {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
    pub model: Option<String>,
}

/// Parse the preferred detailed timing descriptor and the monitor name
/// descriptor (tag 0xFC) of an EDID 1.x base block.
pub fn parse_edid(bytes: &[u8]) -> Option<EdidInfo> {
    const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
    if bytes.len() < 128 || bytes[..8] != HEADER {
        return None;
    }
    let mut timing = None;
    let mut model = None;
    for i in 0..4 {
        let d = &bytes[54 + i * 18..54 + (i + 1) * 18];
        let clock = u16::from_le_bytes([d[0], d[1]]) as f64 * 10_000.0;
        if clock > 0.0 {
            if timing.is_none() {
                let h_active = d[2] as u32 | ((d[4] as u32 & 0xf0) << 4);
                let h_blank = d[3] as u32 | ((d[4] as u32 & 0x0f) << 8);
                let v_active = d[5] as u32 | ((d[7] as u32 & 0xf0) << 4);
                let v_blank = d[6] as u32 | ((d[7] as u32 & 0x0f) << 8);
                let total = ((h_active + h_blank) * (v_active + v_blank)) as f64;
                if total > 0.0 {
                    let hz = (clock / total * 100.0).round() / 100.0;
                    timing = Some((h_active, v_active, hz));
                }
            }
        } else if d[3] == 0xfc {
            let name: String =
                d[5..18].iter().take_while(|&&b| b != 0x0a).map(|&b| b as char).collect::<String>().trim().to_string();
            if !name.is_empty() {
                model = Some(name);
            }
        }
    }
    let (width, height, refresh_hz) = timing?;
    Some(EdidInfo { width, height, refresh_hz, model })
}

pub fn gpus(probe: &Probe) -> Vec<GpuInfo> {
    let dir = probe.sys_path("/sys/class/drm");
    let mut cards: Vec<GpuInfo> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(num) = name.strip_prefix("card") else { continue };
            if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let device = entry.path().join("device");
            let read_id = |f: &str| {
                std::fs::read_to_string(device.join(f))
                    .ok()
                    .map(|s| s.trim().trim_start_matches("0x").to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
            };
            let link_name = |p: PathBuf| {
                std::fs::read_link(p).ok().and_then(|t| t.file_name().map(|f| f.to_string_lossy().into_owned()))
            };
            cards.push(GpuInfo {
                card: name.clone(),
                pci_slot: link_name(device.clone()),
                vendor_id: read_id("vendor"),
                device_id: read_id("device"),
                driver: link_name(device.join("driver")),
                name: None,
            });
        }
    }
    cards.sort_by_key(|g| g.card.trim_start_matches("card").parse::<u32>().unwrap_or(0));

    if let Some(lspci) = probe.find_tool("lspci", &[])
        && let Some(out) = probe.run(&lspci, &["-mm"])
    {
        let devices = parse_lspci_mm(&out.stdout);
        for gpu in &mut cards {
            if let Some(slot) = &gpu.pci_slot
                && let Some(d) = devices.iter().find(|d| slot.ends_with(&d.slot))
            {
                gpu.name = Some(format!("{} {}", d.vendor, d.device));
            }
        }
        // No DRM nodes (e.g. sandbox without /sys): fall back to lspci only.
        if cards.is_empty() {
            for d in devices.into_iter().filter(|d| d.is_display()) {
                cards.push(GpuInfo {
                    card: format!("pci {}", d.slot),
                    pci_slot: Some(d.slot.clone()),
                    vendor_id: None,
                    device_id: None,
                    driver: None,
                    name: Some(format!("{} {}", d.vendor, d.device)),
                });
            }
        }
    }
    cards
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PciDevice {
    pub slot: String,
    pub class: String,
    pub vendor: String,
    pub device: String,
}

impl PciDevice {
    pub fn is_display(&self) -> bool {
        let c = self.class.to_ascii_lowercase();
        c.contains("vga") || c.contains("3d controller") || c.contains("display controller")
    }
}

/// Parse `lspci -mm` (machine readable, quoted fields).
pub fn parse_lspci_mm(text: &str) -> Vec<PciDevice> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((slot, rest)) = line.split_once(' ') else { continue };
        let fields: Vec<String> = shlex_fields(rest).into_iter().filter(|f| !f.starts_with('-')).collect();
        if fields.len() >= 3 {
            out.push(PciDevice {
                slot: slot.to_string(),
                class: fields[0].clone(),
                vendor: fields[1].clone(),
                device: fields[2].clone(),
            });
        }
    }
    out
}

fn shlex_fields(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut field = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                field.push(c);
            }
            out.push(field);
        } else {
            let mut field = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                field.push(c);
                chars.next();
            }
            out.push(field);
        }
    }
    out
}

/// Parse `vulkaninfo --summary`.
pub fn parse_vulkaninfo_summary(text: &str) -> VulkanSummary {
    let mut summary = VulkanSummary::default();
    let mut current: Option<String> = None;
    let mut api: Option<String> = None;
    let flush = |name: &mut Option<String>, api: &mut Option<String>, devices: &mut Vec<String>| {
        if let Some(n) = name.take() {
            devices.push(match api.take() {
                Some(a) => format!("{n} (API {a})"),
                None => n,
            });
        }
        *api = None;
    };
    for line in text.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("Vulkan Instance Version:") {
            summary.instance_version = Some(v.trim().to_string());
        } else if t.starts_with("GPU") && t.ends_with(':') {
            flush(&mut current, &mut api, &mut summary.devices);
        } else if let Some((k, v)) = t.split_once('=') {
            match k.trim() {
                "deviceName" => current = Some(v.trim().to_string()),
                "apiVersion" => api = Some(v.split_whitespace().next().unwrap_or("").to_string()),
                _ => {}
            }
        }
    }
    flush(&mut current, &mut api, &mut summary.devices);
    summary
}

/// `vulkaninfo --summary` when installed; otherwise the list of installed
/// Vulkan ICD manifests (proves a driver is present, not that it works).
pub fn vulkan(probe: &Probe) -> Option<String> {
    if let Some(tool) = probe.find_tool("vulkaninfo", &[])
        && let Some(out) = probe.run(&tool, &["--summary"])
    {
        let s = parse_vulkaninfo_summary(&out.stdout);
        if s.instance_version.is_some() || !s.devices.is_empty() {
            return Some(s.describe());
        }
    }
    let icds = vulkan_icds(probe);
    if icds.is_empty() {
        None
    } else {
        Some(format!("vulkaninfo not installed; Vulkan ICDs present: {}", icds.join(", ")))
    }
}

pub fn vulkan_icds(probe: &Probe) -> Vec<String> {
    let mut out = Vec::new();
    for dir in ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d", "/usr/lib/x86_64-linux-gnu/GL/vulkan/icd.d"] {
        if let Ok(entries) = std::fs::read_dir(probe.sys_path(dir)) {
            for e in entries.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.ends_with(".json") && !out.contains(&n) {
                    out.push(n);
                }
            }
        }
    }
    out.sort();
    out
}

/// Gamescope's WSI Vulkan layer (needed for HDR / proper presentation of
/// Vulkan clients inside gamescope).
pub fn gamescope_wsi_layer(probe: &Probe) -> bool {
    let mut dirs: Vec<PathBuf> = [
        "/usr/share/vulkan/implicit_layer.d",
        "/etc/vulkan/implicit_layer.d",
        "/usr/lib/extensions/vulkan/gamescope/share/vulkan/implicit_layer.d",
    ]
    .iter()
    .map(|d| probe.sys_path(d))
    .collect();
    if let Some(data) = probe.data_home() {
        dirs.push(data.join("vulkan/implicit_layer.d"));
    }
    dirs.iter().any(|d| dir_has(d, |n| n.contains("gamescope_wsi")))
}

fn dir_has(dir: &Path, pred: impl Fn(&str) -> bool) -> bool {
    std::fs::read_dir(dir).map(|it| it.flatten().any(|e| pred(&e.file_name().to_string_lossy()))).unwrap_or(false)
}

/// Candidate gamescope locations beyond `PATH` (Flatpak extension).
pub fn gamescope_candidates(probe: &Probe) -> Vec<PathBuf> {
    vec![probe.sys_path("/usr/lib/extensions/vulkan/gamescope/bin/gamescope")]
}

pub fn find_gamescope(probe: &Probe) -> Option<PathBuf> {
    probe.find_tool("gamescope", &gamescope_candidates(probe))
}

pub fn gamescope_info(probe: &Probe) -> Option<ToolInfo> {
    let path = find_gamescope(probe)?;
    // `gamescope --version` prints "gamescope version 3.17.0 (gcc ...)" on stderr.
    let version = probe.run(&path, &["--version"]).and_then(|o| crate::probe::extract_version(&o.combined()));
    Some(ToolInfo { path, version })
}

pub fn capabilities(probe: &Probe, rt_upscaler: Option<ToolInfo>) -> GraphicsCapabilities {
    let session = session_type(probe);
    let xwayland = xwayland_available(probe);
    let desktop = desktop(probe);
    let gamescope = gamescope_info(probe);
    let monitors = monitors(probe);
    let gpu_details = gpus(probe);
    let wsi = gamescope_wsi_layer(probe);
    let flatpak = probe.inside_flatpak();

    let mut notes = Vec::new();
    match session {
        DisplaySessionType::Wayland => {
            notes.push(
                "Wayland session: Wine/Proton games run through XWayland unless the Wine Wayland driver is enabled."
                    .into(),
            );
            if !xwayland {
                notes.push("No DISPLAY is set: XWayland seems unavailable, so X11-only tools (RT-Upscaler, X11 text hookers/OCR capture) will not work.".into());
            }
        }
        DisplaySessionType::X11 => {
            notes.push("X11 session: gamescope HDR output is not possible (HDR clients are tonemapped to SDR).".into())
        }
        DisplaySessionType::Unknown => notes.push("No graphical session detected (no WAYLAND_DISPLAY/DISPLAY).".into()),
    }
    if nested_session(probe) {
        notes.push("gamescope will run nested (as a window). `--prefer-output` is an embedded-mode (DRM) option and has no effect here.".into());
    }
    match &gamescope {
        Some(g) => {
            if !wsi {
                notes.push("Gamescope WSI Vulkan layer (VkLayer_FROG_gamescope_wsi) not found: HDR from games inside gamescope will not work.".into());
            }
            if let Some(v) = &g.version {
                notes.push(format!("gamescope {v} found at {}.", g.path.display()));
            }
        }
        None => notes.push(if flatpak {
            "gamescope not found. Inside Flatpak install the org.freedesktop.Platform.VulkanLayer.gamescope extension."
                .into()
        } else {
            "gamescope not installed: the gamescope wrapper is unavailable.".into()
        }),
    }
    if gpu_details.len() > 1 {
        let ids: Vec<String> = gpu_details.iter().filter_map(|g| g.vk_device_id()).collect();
        notes.push(format!(
            "Multiple GPUs detected. gamescope can be pinned to one with the extra argument `--prefer-vk-device <vendor:device>` (available: {}).",
            ids.join(", ")
        ));
    }
    if gpu_details.iter().any(|g| g.driver.as_deref() == Some("nvidia")) {
        notes.push(
            "NVIDIA proprietary driver detected: gamescope requires DRM kernel modesetting (nvidia-drm.modeset=1)."
                .into(),
        );
    }
    if rt_upscaler.is_some() && session == DisplaySessionType::Wayland {
        notes.push("RT-Upscaler can only capture X11/XWayland windows; native Wayland windows (including gamescope's own window) cannot be upscaled.".into());
    }
    if flatpak {
        notes.push("Running inside Flatpak: host tools are reached through `flatpak-spawn --host`.".into());
    }
    let refresh: Vec<String> = monitors
        .iter()
        .filter_map(|m| Some(format!("{} {}x{}@{:.0}Hz", m.name, m.width?, m.height?, m.refresh_hz?)))
        .collect();
    if !refresh.is_empty() {
        notes.push(format!("Connected outputs: {}.", refresh.join(", ")));
    }

    GraphicsCapabilities {
        session_type: session,
        xwayland,
        desktop,
        gamescope,
        rt_upscaler,
        monitors,
        gpus: gpu_details.iter().map(GpuInfo::label).collect(),
        notes,
        gpu_details,
        gamescope_wsi_layer: wsi,
        inside_flatpak: flatpak,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{EDID_G40B, drm_fixture};
    use std::collections::BTreeMap;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn session_detection() {
        let p = |e| Probe::fixture("/nonexistent", e);
        assert_eq!(session_type(&p(env(&[("XDG_SESSION_TYPE", "wayland")]))), DisplaySessionType::Wayland);
        assert_eq!(session_type(&p(env(&[("XDG_SESSION_TYPE", "x11")]))), DisplaySessionType::X11);
        assert_eq!(
            session_type(&p(env(&[("XDG_SESSION_TYPE", "tty"), ("WAYLAND_DISPLAY", "wayland-0")]))),
            DisplaySessionType::Wayland
        );
        assert_eq!(session_type(&p(env(&[("DISPLAY", ":0")]))), DisplaySessionType::X11);
        assert_eq!(session_type(&p(env(&[]))), DisplaySessionType::Unknown);
        assert!(xwayland_available(&p(env(&[("XDG_SESSION_TYPE", "wayland"), ("DISPLAY", ":1")]))));
        assert!(!xwayland_available(&p(env(&[("XDG_SESSION_TYPE", "wayland")]))));
    }

    #[test]
    fn connector_names() {
        assert_eq!(split_connector("card1-DP-1"), Some(("card1", "DP-1")));
        assert_eq!(split_connector("card12-HDMI-A-2"), Some(("card12", "HDMI-A-2")));
        assert_eq!(split_connector("card0-eDP-1"), Some(("card0", "eDP-1")));
        assert_eq!(split_connector("card1"), None);
        assert_eq!(split_connector("renderD128"), None);
    }

    #[test]
    fn edid_parsing() {
        let e = parse_edid(&EDID_G40B).unwrap();
        assert_eq!((e.width, e.height), (1920, 1080));
        assert!((e.refresh_hz - 239.76).abs() < 0.05, "{}", e.refresh_hz);
        assert_eq!(e.model.as_deref(), Some("Odyssey G40B"));
        assert!(parse_edid(&[0u8; 128]).is_none());
        assert!(parse_edid(&EDID_G40B[..64]).is_none());
    }

    #[test]
    fn monitors_and_gpus_from_fixture() {
        let dir = tempfile::tempdir().unwrap();
        drm_fixture(dir.path());
        let probe = Probe::fixture(dir.path(), env(&[("PATH", "/nonexistent")]));
        let mons = monitors(&probe);
        assert_eq!(mons.len(), 2);
        assert_eq!(mons[0].name, "DP-8");
        assert_eq!(mons[0].card.as_deref(), Some("card2"));
        assert_eq!((mons[0].width, mons[0].height), (Some(1920), Some(1080)));
        assert!(mons[0].refresh_hz.unwrap() > 239.0);
        assert_eq!(mons[1].name, "HDMI-A-2");
        assert_eq!((mons[1].width, mons[1].height), (Some(2560), Some(1440)));
        assert_eq!(mons[1].refresh_hz, None);
        assert!(!mons[0].primary && !mons[1].primary);

        let conns = connectors(&probe);
        assert!(conns.iter().any(|(_, n, c)| n == "DP-1" && !c));
        assert!(!conns.iter().any(|(_, n, _)| n.starts_with("Writeback")));

        let g = gpus(&probe);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].card, "card1");
        assert_eq!(g[0].vk_device_id().as_deref(), Some("1002:15bf"));
        assert_eq!(g[0].driver.as_deref(), Some("amdgpu"));
        assert_eq!(g[1].pci_slot.as_deref(), Some("0000:01:00.0"));
    }

    #[test]
    fn lspci_names_are_merged() {
        let dir = tempfile::tempdir().unwrap();
        drm_fixture(dir.path());
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        crate::test_support::script(
            &bin,
            "lspci",
            r#"echo '01:00.0 "VGA compatible controller" "NVIDIA Corporation" "GB206 [GeForce RTX 5060 Ti]" -ra1 -p00 "Micro-Star" "Device 5360"'
echo '44:00.0 "VGA compatible controller" "Advanced Micro Devices, Inc. [AMD/ATI]" "Phoenix1" -r05 -p00 "MSI" "Device 7e24"'
echo '00:14.0 "USB controller" "Intel" "xHCI"'"#,
        );
        let probe = Probe::fixture(dir.path(), env(&[("PATH", bin.to_str().unwrap())]));
        let g = gpus(&probe);
        assert_eq!(g[1].name.as_deref(), Some("NVIDIA Corporation GB206 [GeForce RTX 5060 Ti]"));
        assert_eq!(g[1].label(), "NVIDIA Corporation GB206 [GeForce RTX 5060 Ti] (10de:2d04, driver nvidia, card2)");
        assert_eq!(g[0].name.as_deref(), Some("Advanced Micro Devices, Inc. [AMD/ATI] Phoenix1"));
    }

    #[test]
    fn vulkaninfo_summary_parsing() {
        let text = "==========\nVULKANINFO\n==========\n\nVulkan Instance Version: 1.4.321\n\nDevices:\n========\nGPU0:\n\tapiVersion         = 1.4.312\n\tdriverVersion      = 580.0\n\tdeviceName         = NVIDIA GeForce RTX 5060 Ti\nGPU1:\n\tapiVersion         = 1.4.318\n\tdeviceName         = AMD Radeon 760M Graphics (RADV PHOENIX)\n";
        let s = parse_vulkaninfo_summary(text);
        assert_eq!(s.instance_version.as_deref(), Some("1.4.321"));
        assert_eq!(
            s.devices,
            vec!["NVIDIA GeForce RTX 5060 Ti (API 1.4.312)", "AMD Radeon 760M Graphics (RADV PHOENIX) (API 1.4.318)"]
        );
    }

    #[test]
    fn capabilities_notes() {
        let dir = tempfile::tempdir().unwrap();
        drm_fixture(dir.path());
        let probe = Probe::fixture(
            dir.path(),
            env(&[
                ("XDG_SESSION_TYPE", "wayland"),
                ("WAYLAND_DISPLAY", "wayland-0"),
                ("DISPLAY", ":1"),
                ("XDG_CURRENT_DESKTOP", "GNOME"),
                ("PATH", "/nonexistent"),
            ]),
        );
        let caps = capabilities(&probe, None);
        assert_eq!(caps.session_type, DisplaySessionType::Wayland);
        assert!(caps.xwayland);
        assert_eq!(caps.desktop.as_deref(), Some("GNOME"));
        assert!(caps.gamescope.is_none());
        assert!(caps.notes.iter().any(|n| n.contains("Multiple GPUs")));
        assert!(caps.notes.iter().any(|n| n.contains("--prefer-output")));
        let json = serde_json::to_value(&caps).unwrap();
        assert_eq!(json["sessionType"], "wayland");
        assert!(json["monitors"][0]["refreshHz"].is_number());
    }
}
