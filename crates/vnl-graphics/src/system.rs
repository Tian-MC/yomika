//! System report for diagnostics / bug reports.

use serde::{Deserialize, Serialize};

use crate::display::{self, DisplaySessionType};
use crate::probe::Probe;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemReport {
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub desktop: Option<String>,
    pub session_type: DisplaySessionType,
    pub inside_flatpak: bool,
    pub cpu: Option<String>,
    pub memory_mb: Option<u64>,
    pub gpus: Vec<String>,
    pub vulkan: Option<String>,
    /// `ID` from os-release (e.g. `arch`), useful to pick install hints.
    pub os_id: Option<String>,
    /// `ID_LIKE` from os-release (e.g. `debian`).
    pub os_id_like: Option<String>,
}

/// Parse `/etc/os-release` into (PRETTY_NAME or NAME VERSION, ID, ID_LIKE).
pub fn parse_os_release(text: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim();
            let v = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(v);
            let v = v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')).unwrap_or(v);
            map.insert(k.trim().to_string(), v.replace("\\\"", "\""));
        }
    }
    let pretty = map.get("PRETTY_NAME").cloned().or_else(|| {
        let name = map.get("NAME")?;
        Some(match map.get("VERSION") {
            Some(v) => format!("{name} {v}"),
            None => name.clone(),
        })
    });
    (pretty, map.get("ID").cloned(), map.get("ID_LIKE").cloned())
}

pub fn parse_cpu_model(cpuinfo: &str) -> Option<String> {
    cpuinfo
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            matches!(k.trim(), "model name" | "Model" | "Hardware").then(|| v.trim().to_string())
        })
        .filter(|s| !s.is_empty())
}

pub fn parse_mem_total_mb(meminfo: &str) -> Option<u64> {
    meminfo.lines().find_map(|l| {
        let rest = l.strip_prefix("MemTotal:")?;
        let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
        Some(kb / 1024)
    })
}

pub fn report(probe: &Probe) -> SystemReport {
    let os_text = probe.read_sys("/etc/os-release").or_else(|| probe.read_sys("/usr/lib/os-release"));
    let (os, os_id, os_id_like) = os_text.as_deref().map(parse_os_release).unwrap_or((None, None, None));
    let kernel = probe.read_sys("/proc/sys/kernel/osrelease").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    SystemReport {
        os,
        kernel,
        desktop: display::desktop(probe),
        session_type: display::session_type(probe),
        inside_flatpak: probe.inside_flatpak(),
        cpu: probe.read_sys("/proc/cpuinfo").as_deref().and_then(parse_cpu_model),
        memory_mb: probe.read_sys("/proc/meminfo").as_deref().and_then(parse_mem_total_mb),
        gpus: display::gpus(probe).iter().map(display::GpuInfo::label).collect(),
        vulkan: display::vulkan(probe),
        os_id,
        os_id_like,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn os_release_parsing() {
        let arch = "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\nBUILD_ID=rolling\n";
        assert_eq!(parse_os_release(arch), (Some("Arch Linux".into()), Some("arch".into()), None));
        let ubuntu = "NAME=\"Ubuntu\"\nVERSION=\"24.04.1 LTS (Noble Numbat)\"\nID=ubuntu\nID_LIKE=debian\n";
        let (p, id, like) = parse_os_release(ubuntu);
        assert_eq!(p.as_deref(), Some("Ubuntu 24.04.1 LTS (Noble Numbat)"));
        assert_eq!(id.as_deref(), Some("ubuntu"));
        assert_eq!(like.as_deref(), Some("debian"));
    }

    #[test]
    fn report_from_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("etc")).unwrap();
        std::fs::create_dir_all(r.join("proc/sys/kernel")).unwrap();
        std::fs::write(r.join("etc/os-release"), "PRETTY_NAME=\"Fedora Linux 43 (Workstation Edition)\"\nID=fedora\n")
            .unwrap();
        std::fs::write(r.join("proc/sys/kernel/osrelease"), "7.2.11-zen1-1-zen\n").unwrap();
        std::fs::write(
            r.join("proc/cpuinfo"),
            "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD Ryzen 5 8600G w/ Radeon 760M Graphics\n",
        )
        .unwrap();
        std::fs::write(r.join("proc/meminfo"), "MemTotal:       31929912 kB\nMemFree: 1 kB\n").unwrap();
        std::fs::create_dir_all(r.join("usr/share/vulkan/icd.d")).unwrap();
        std::fs::write(r.join("usr/share/vulkan/icd.d/nvidia_icd.json"), "{}").unwrap();
        std::fs::write(r.join(".flatpak-info"), "").unwrap();
        crate::test_support::drm_fixture(r);
        let env: BTreeMap<String, String> = [
            ("XDG_SESSION_TYPE".to_string(), "wayland".to_string()),
            ("XDG_CURRENT_DESKTOP".to_string(), "KDE".to_string()),
            ("PATH".to_string(), "/nonexistent".to_string()),
        ]
        .into();
        let rep = report(&Probe::fixture(r, env));
        assert_eq!(rep.os.as_deref(), Some("Fedora Linux 43 (Workstation Edition)"));
        assert_eq!(rep.os_id.as_deref(), Some("fedora"));
        assert_eq!(rep.kernel.as_deref(), Some("7.2.11-zen1-1-zen"));
        assert_eq!(rep.cpu.as_deref(), Some("AMD Ryzen 5 8600G w/ Radeon 760M Graphics"));
        assert_eq!(rep.memory_mb, Some(31181));
        assert_eq!(rep.session_type, DisplaySessionType::Wayland);
        assert_eq!(rep.desktop.as_deref(), Some("KDE"));
        assert!(rep.inside_flatpak);
        assert_eq!(rep.gpus.len(), 2);
        assert_eq!(rep.vulkan.as_deref(), Some("vulkaninfo not installed; Vulkan ICDs present: nvidia_icd.json"));
        let json = serde_json::to_value(&rep).unwrap();
        assert_eq!(json["memoryMb"], 31181);
        assert_eq!(json["insideFlatpak"], true);
    }
}
