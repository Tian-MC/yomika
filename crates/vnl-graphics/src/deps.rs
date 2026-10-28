//! External dependency discovery for the diagnostics page.
//!
//! Guided installation: every entry carries per-distro install hints as
//! *data*; the UI shows/copies them. Nothing here ever runs a package manager
//! or `sudo`. Arch package names were verified with `pacman -Si` (all in
//! `extra`, `umu-launcher` in `multilib`); Debian/Fedora names are best-effort.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vnl_core::{Runner, RunnerKind};

use crate::probe::{Probe, extract_version};
use crate::upscaler::{RtUpscaler, Upscaler};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyCategory {
    Runtime,
    Graphics,
    Translation,
    Ocr,
    Packaging,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallHint {
    /// `arch`, `debian`, `fedora`, `flatpak`, `generic`.
    pub distro: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyStatus {
    pub id: String,
    pub name: String,
    pub category: DependencyCategory,
    pub required: bool,
    pub found: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub purpose: String,
    pub install_hints: Vec<InstallHint>,
    /// Extra detection details (e.g. portal backends, matching fonts).
    pub details: Option<String>,
}

fn hints(list: &[(&str, &str)]) -> Vec<InstallHint> {
    list.iter().map(|(d, c)| InstallHint { distro: d.to_string(), command: c.to_string() }).collect()
}

struct Entry {
    id: &'static str,
    name: &'static str,
    category: DependencyCategory,
    required: bool,
    purpose: &'static str,
    hints: &'static [(&'static str, &'static str)],
}

impl Entry {
    fn status(&self) -> DependencyStatus {
        DependencyStatus {
            id: self.id.into(),
            name: self.name.into(),
            category: self.category,
            required: self.required,
            found: false,
            version: None,
            path: None,
            purpose: self.purpose.into(),
            install_hints: hints(self.hints),
            details: None,
        }
    }
}

use DependencyCategory as C;

/// Simple "binary + version flag" dependencies.
const TOOLS: &[(Entry, &str, &[&str])] = &[
    (
        Entry {
            id: "wine",
            name: "Wine",
            category: C::Runtime,
            required: true,
            purpose: "Runs Windows visual novels (unless a Proton runner is used).",
            hints: &[
                ("arch", "sudo pacman -S wine"),
                ("debian", "sudo dpkg --add-architecture i386 && sudo apt update && sudo apt install wine wine32 wine64"),
                ("fedora", "sudo dnf install wine"),
                ("generic", "Install Wine from your distribution or https://gitlab.winehq.org/wine/wine/-/wikis/Download"),
            ],
        },
        "wine",
        &["--version"],
    ),
    (
        Entry {
            id: "winetricks",
            name: "winetricks",
            category: C::Runtime,
            required: false,
            purpose: "Installs fonts, codecs and runtimes (cjkfonts, lavfilters, vcrun...) into Wine prefixes.",
            hints: &[
                ("arch", "sudo pacman -S winetricks"),
                ("debian", "sudo apt install winetricks"),
                ("fedora", "sudo dnf install winetricks"),
                ("generic", "https://github.com/Winetricks/winetricks#installing"),
            ],
        },
        "winetricks",
        &["--version"],
    ),
    (
        Entry {
            id: "protontricks",
            name: "protontricks",
            category: C::Runtime,
            required: false,
            purpose: "winetricks for Steam/Proton prefixes.",
            hints: &[
                ("arch", "sudo pacman -S protontricks"),
                ("debian", "sudo apt install protontricks"),
                ("fedora", "sudo dnf install protontricks"),
                ("flatpak", "flatpak install flathub com.github.Matoking.protontricks"),
                ("generic", "pipx install protontricks"),
            ],
        },
        "protontricks",
        &["--version"],
    ),
    (
        Entry {
            id: "umu-run",
            name: "umu-launcher",
            category: C::Runtime,
            required: false,
            purpose: "Runs Proton outside Steam with the Steam Runtime container (recommended for Proton runners).",
            hints: &[
                ("arch", "sudo pacman -S umu-launcher"),
                ("fedora", "sudo dnf install umu-launcher"),
                ("debian", "Download the .deb from https://github.com/Open-Wine-Components/umu-launcher/releases"),
                ("generic", "https://github.com/Open-Wine-Components/umu-launcher#installing"),
            ],
        },
        "umu-run",
        &["--version"],
    ),
    (
        Entry {
            id: "gamescope",
            name: "gamescope",
            category: C::Graphics,
            required: false,
            purpose: "Micro-compositor for resolution control, FSR/NIS upscaling, fps limits and HDR.",
            hints: &[
                ("arch", "sudo pacman -S gamescope"),
                ("debian", "sudo apt install gamescope"),
                ("fedora", "sudo dnf install gamescope"),
                ("flatpak", "flatpak install flathub org.freedesktop.Platform.VulkanLayer.gamescope"),
            ],
        },
        "gamescope",
        &["--version"],
    ),
    (
        Entry {
            id: "vulkaninfo",
            name: "vulkaninfo (vulkan-tools)",
            category: C::Graphics,
            required: false,
            purpose: "Reports Vulkan driver/GPU details for diagnostics.",
            hints: &[
                ("arch", "sudo pacman -S vulkan-tools"),
                ("debian", "sudo apt install vulkan-tools"),
                ("fedora", "sudo dnf install vulkan-tools"),
            ],
        },
        "vulkaninfo",
        &["--summary"],
    ),
    (
        Entry {
            id: "ollama",
            name: "Ollama",
            category: C::Translation,
            required: false,
            purpose: "Local LLM server for offline translation and vision OCR.",
            hints: &[
                ("arch", "sudo pacman -S ollama"),
                ("generic", "curl -fsSL https://ollama.com/install.sh | sh"),
            ],
        },
        "ollama",
        &["--version"],
    ),
    (
        Entry {
            id: "imagemagick",
            name: "ImageMagick (import)",
            category: C::Ocr,
            required: false,
            purpose: "Screen-region capture on X11/XWayland for OCR (`import`).",
            hints: &[
                ("arch", "sudo pacman -S imagemagick"),
                ("debian", "sudo apt install imagemagick"),
                ("fedora", "sudo dnf install ImageMagick"),
            ],
        },
        "import",
        &["-version"],
    ),
    (
        Entry {
            id: "wl-clipboard",
            name: "wl-clipboard (wl-paste)",
            category: C::Translation,
            required: false,
            purpose: "Clipboard text extraction on Wayland.",
            hints: &[
                ("arch", "sudo pacman -S wl-clipboard"),
                ("debian", "sudo apt install wl-clipboard"),
                ("fedora", "sudo dnf install wl-clipboard"),
            ],
        },
        "wl-paste",
        &["--version"],
    ),
];

const TESSERACT: Entry = Entry {
    id: "tesseract",
    name: "Tesseract OCR",
    category: C::Ocr,
    required: false,
    purpose: "Offline OCR of dialogue text.",
    hints: &[
        ("arch", "sudo pacman -S tesseract tesseract-data-jpn"),
        ("debian", "sudo apt install tesseract-ocr tesseract-ocr-jpn"),
        ("fedora", "sudo dnf install tesseract tesseract-langpack-jpn"),
    ],
};

const TESSERACT_JPN: Entry = Entry {
    id: "tesseract-jpn",
    name: "Tesseract Japanese data (jpn)",
    category: C::Ocr,
    required: false,
    purpose: "Japanese language model for Tesseract (jpn.traineddata; jpn_vert for vertical text).",
    hints: &[
        ("arch", "sudo pacman -S tesseract-data-jpn"),
        ("debian", "sudo apt install tesseract-ocr-jpn"),
        ("fedora", "sudo dnf install tesseract-langpack-jpn"),
    ],
};

const PORTAL: Entry = Entry {
    id: "xdg-desktop-portal",
    name: "xdg-desktop-portal",
    category: C::Other,
    required: false,
    purpose: "File pickers and screen capture (ScreenCast) on Wayland/Flatpak.",
    hints: &[
        ("arch", "sudo pacman -S xdg-desktop-portal xdg-desktop-portal-gtk"),
        ("debian", "sudo apt install xdg-desktop-portal xdg-desktop-portal-gtk"),
        ("fedora", "sudo dnf install xdg-desktop-portal xdg-desktop-portal-gtk"),
    ],
};

const JA_LOCALE: Entry = Entry {
    id: "ja-locale",
    name: "Japanese locale (ja_JP.UTF-8)",
    category: C::Runtime,
    required: false,
    purpose: "Many Japanese VNs need a Japanese locale (LANG=ja_JP.UTF-8) to show text and paths correctly.",
    hints: &[
        ("arch", "sudo sed -i 's/^#ja_JP.UTF-8/ja_JP.UTF-8/' /etc/locale.gen && sudo locale-gen"),
        ("debian", "sudo sed -i 's/^# *ja_JP.UTF-8/ja_JP.UTF-8/' /etc/locale.gen && sudo locale-gen"),
        ("fedora", "sudo dnf install glibc-langpack-ja"),
    ],
};

const JA_FONTS: Entry = Entry {
    id: "ja-fonts",
    name: "Japanese fonts (host)",
    category: C::Other,
    required: false,
    purpose: "Japanese glyphs in the launcher UI, overlay and native tools.",
    hints: &[
        ("arch", "sudo pacman -S noto-fonts-cjk"),
        ("debian", "sudo apt install fonts-noto-cjk"),
        ("fedora", "sudo dnf install google-noto-sans-cjk-fonts"),
    ],
};

const PROTON: Entry = Entry {
    id: "proton",
    name: "Proton runners",
    category: C::Runtime,
    required: false,
    purpose: "Proton builds (GE-Proton recommended: includes Wine FSR and media fixes).",
    hints: &[
        ("generic", "Download GE-Proton from https://github.com/GloriousEggroll/proton-ge-custom/releases into ~/.local/share/Steam/compatibilitytools.d"),
        ("flatpak", "flatpak install flathub net.davidotek.pupgui2  # ProtonUp-Qt"),
    ],
};

const RT_UPSCALER: Entry = Entry {
    id: "rt-upscaler",
    name: "RT-Upscaler (linux-rt-upscaler)",
    category: C::Graphics,
    required: false,
    purpose: "Real-time CuNNy/SRCNN AI upscaling of X11/XWayland game windows.",
    hints: &[
        ("arch", "sudo pacman -S python-pipx xcb-util-cursor && pipx install linux-rt-upscaler"),
        ("debian", "sudo apt install pipx libxcb-cursor0 && pipx install linux-rt-upscaler"),
        ("fedora", "sudo dnf install pipx xcb-util-cursor && pipx install linux-rt-upscaler"),
        ("generic", "pipx install linux-rt-upscaler"),
    ],
};

const FLATPAK_SPAWN: Entry = Entry {
    id: "flatpak-spawn",
    name: "flatpak-spawn",
    category: C::Packaging,
    required: true,
    purpose: "Runs host programs (wine, gamescope, ...) from inside the Flatpak sandbox (needs --talk-name=org.freedesktop.Flatpak).",
    hints: &[("flatpak", "Provided by the Flatpak runtime; grant --talk-name=org.freedesktop.Flatpak")],
};

/// `locale -a` output contains a Japanese UTF-8 locale (`ja_JP.utf8`, `ja_JP.UTF-8`).
pub fn has_ja_utf8_locale(locale_a: &str) -> Option<String> {
    locale_a
        .lines()
        .map(str::trim)
        .find(|l| {
            let n = l.to_ascii_lowercase().replace('-', "");
            n == "ja_jp.utf8"
        })
        .map(str::to_string)
}

/// Proton builds in the usual Steam locations: `(name, path, version)`.
pub fn scan_proton_dirs(probe: &Probe) -> Vec<(String, PathBuf, Option<String>)> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(h) = probe.home() {
        for base in [".local/share/Steam", ".steam/root", ".steam/steam", ".var/app/com.valvesoftware.Steam/data/Steam"] {
            roots.push(h.join(base).join("compatibilitytools.d"));
            roots.push(h.join(base).join("steamapps/common"));
        }
    }
    roots.push(probe.sys_path("/usr/share/steam/compatibilitytools.d"));
    let mut out: Vec<(String, PathBuf, Option<String>)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if !path.join("proton").is_file() {
                continue;
            }
            let canon = std::fs::canonicalize(&path).unwrap_or(path.clone());
            if !seen.insert(canon) {
                continue;
            }
            let version = std::fs::read_to_string(path.join("version"))
                .ok()
                .and_then(|v| v.split_whitespace().nth(1).map(str::to_string));
            out.push((e.file_name().to_string_lossy().into_owned(), path, version));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn tesseract_data_dirs(probe: &Probe) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(p) = probe.var("TESSDATA_PREFIX") {
        dirs.push(PathBuf::from(p));
    }
    for d in ["/usr/share/tessdata", "/usr/share/tesseract/tessdata", "/usr/local/share/tessdata"] {
        dirs.push(probe.sys_path(d));
    }
    if let Ok(entries) = std::fs::read_dir(probe.sys_path("/usr/share/tesseract-ocr")) {
        for e in entries.flatten() {
            dirs.push(e.path().join("tessdata"));
        }
    }
    dirs
}

/// Detect every dependency. `runners` are runners already known to the
/// runtime (merged with a direct scan of Steam compatibility tool dirs).
pub fn detect(probe: &Probe, runners: &[Runner]) -> Vec<DependencyStatus> {
    let mut out = Vec::new();
    let flatpak = probe.inside_flatpak();

    for (entry, bin, version_args) in TOOLS {
        let mut s = entry.status();
        if let Some(path) = probe.find_tool(bin, &[]) {
            s.found = true;
            if let Some(o) = probe.run(&path, version_args) {
                let text = o.combined();
                s.version = if *bin == "vulkaninfo" {
                    crate::display::parse_vulkaninfo_summary(&text).instance_version
                } else {
                    extract_version(&text)
                };
                if *bin == "vulkaninfo" {
                    let sum = crate::display::parse_vulkaninfo_summary(&text);
                    if !sum.devices.is_empty() {
                        s.details = Some(sum.devices.join("; "));
                    }
                }
            }
            s.path = Some(path);
        } else if *bin == "gamescope"
            && let Some(path) = crate::display::find_gamescope(probe)
        {
            s.found = true;
            s.version = probe.run(&path, &["--version"]).and_then(|o| extract_version(&o.combined()));
            s.path = Some(path);
        }
        if *bin == "wine" {
            // Wine is only strictly required when no Proton runner exists.
            s.required = true;
        }
        out.push(s);
    }

    // Proton runners.
    let mut proton = PROTON.status();
    let mut names: Vec<String> = runners.iter().filter(|r| r.kind == RunnerKind::Proton).map(|r| r.name.clone()).collect();
    for (name, path, version) in scan_proton_dirs(probe) {
        if !runners.iter().any(|r| r.path == path) && !names.contains(&name) {
            names.push(match version {
                Some(v) if v != name => format!("{name} ({v})"),
                _ => name,
            });
        }
        proton.path.get_or_insert(path);
    }
    proton.found = !names.is_empty();
    if proton.found {
        proton.details = Some(names.join(", "));
    }
    // Wine is not required when a Proton runner is available.
    if proton.found
        && let Some(w) = out.iter_mut().find(|d| d.id == "wine")
    {
        w.required = false;
    }
    out.push(proton);

    // Tesseract + Japanese data.
    let mut tess = TESSERACT.status();
    let mut jpn = TESSERACT_JPN.status();
    if let Some(path) = probe.find_tool("tesseract", &[]) {
        tess.found = true;
        tess.version = probe.run(&path, &["--version"]).and_then(|o| extract_version(&o.combined()));
        if let Some(o) = probe.run(&path, &["--list-langs"]) {
            let langs: Vec<&str> = o.combined().lines().map(str::trim).filter(|l| l.starts_with("jpn")).map(|l| l).collect::<Vec<_>>();
            if !langs.is_empty() {
                jpn.found = true;
                jpn.details = Some(langs.join(", "));
            }
        }
        tess.path = Some(path);
    }
    if !jpn.found
        && let Some(f) = tesseract_data_dirs(probe).into_iter().map(|d| d.join("jpn.traineddata")).find(|f| f.is_file())
    {
        jpn.found = true;
        jpn.path = Some(f);
    }
    out.push(tess);
    out.push(jpn);

    // xdg-desktop-portal (not on PATH; lives in libexec dirs). Not executed:
    // starting a second portal instance is not side-effect free.
    let mut portal = PORTAL.status();
    for dir in ["/usr/lib", "/usr/libexec", "/usr/lib/x86_64-linux-gnu", "/usr/local/libexec"] {
        let dir = probe.sys_path(dir);
        let p = dir.join("xdg-desktop-portal");
        if p.is_file() {
            portal.found = true;
            portal.path = Some(p);
            let backends: Vec<String> = std::fs::read_dir(&dir)
                .map(|it| {
                    let mut v: Vec<String> = it
                        .flatten()
                        .filter_map(|e| e.file_name().to_string_lossy().strip_prefix("xdg-desktop-portal-").map(str::to_string))
                        .filter(|n| !n.starts_with("validate") && !n.starts_with("rewrite"))
                        .collect();
                    v.sort();
                    v
                })
                .unwrap_or_default();
            if !backends.is_empty() {
                portal.details = Some(format!("backends: {}", backends.join(", ")));
            }
            break;
        }
    }
    out.push(portal);

    // ja_JP.UTF-8 locale.
    let mut loc = JA_LOCALE.status();
    if let Some(locale) = probe.find_tool("locale", &[])
        && let Some(o) = probe.run(&locale, &["-a"])
        && let Some(name) = has_ja_utf8_locale(&o.stdout)
    {
        loc.found = true;
        loc.details = Some(name);
    }
    out.push(loc);

    // Japanese fonts via fontconfig.
    let mut fonts = JA_FONTS.status();
    if let Some(fc) = probe.find_tool("fc-list", &[])
        && let Some(o) = probe.run(&fc, &[":lang=ja", "family"])
    {
        let mut families: Vec<String> = o
            .stdout
            .lines()
            .filter_map(|l| l.split(',').next())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        families.sort();
        families.dedup();
        if !families.is_empty() {
            fonts.found = true;
            let preferred = families.iter().find(|f| f.contains("JP")).unwrap_or(&families[0]).clone();
            fonts.details = Some(format!("{} font families, e.g. {preferred}", families.len()));
        }
    }
    out.push(fonts);

    // RT-Upscaler.
    let mut rt = RT_UPSCALER.status();
    let info = RtUpscaler.info(probe);
    if let Some(tool) = info.tool {
        rt.found = true;
        rt.version = tool.version;
        rt.path = Some(tool.path);
        rt.details = Some(format!("{} models", info.models.len()));
    }
    out.push(rt);

    if flatpak {
        let mut fs = FLATPAK_SPAWN.status();
        if let Some(p) = probe.find_tool("flatpak-spawn", &[probe.sys_path("/usr/bin/flatpak-spawn")]) {
            fs.found = true;
            fs.path = Some(p);
        }
        out.push(fs);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::script;
    use std::collections::BTreeMap;

    #[test]
    fn locale_matching() {
        assert_eq!(has_ja_utf8_locale("C\nC.utf8\nen_US.utf8\nja_JP.utf8\n").as_deref(), Some("ja_JP.utf8"));
        assert_eq!(has_ja_utf8_locale("ja_JP.UTF-8\n").as_deref(), Some("ja_JP.UTF-8"));
        assert_eq!(has_ja_utf8_locale("ja_JP.eucjp\nC\n"), None);
    }

    #[test]
    fn detection_with_fake_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let root = dir.path().join("root");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&bin).unwrap();
        script(&bin, "wine", "echo wine-11.20");
        script(&bin, "winetricks", "echo 'Executing cd /usr/bin' >&2; echo '20260125 - sha256sum: 431f'");
        script(&bin, "gamescope", "echo 'gamescope version 3.17.0 (gcc 16.2.1)' >&2");
        script(&bin, "locale", "printf 'C\\nC.utf8\\nja_JP.utf8\\nPOSIX\\n'");
        script(&bin, "fc-list", "printf 'Noto Sans CJK TC,Noto Sans CJK TC Medium\\nNoto Sans CJK JP\\nIPAGothic\\n'");
        script(&bin, "tesseract", "if [ \"$1\" = --list-langs ]; then echo 'List of available languages (3):'; echo eng; echo jpn; echo jpn_vert; else echo 'tesseract 5.5.1'; fi");
        let portal_dir = root.join("usr/lib");
        std::fs::create_dir_all(&portal_dir).unwrap();
        for f in ["xdg-desktop-portal", "xdg-desktop-portal-gnome", "xdg-desktop-portal-gtk", "xdg-desktop-portal-validate-icon"] {
            std::fs::write(portal_dir.join(f), "").unwrap();
        }
        let ge = home.join(".local/share/Steam/compatibilitytools.d/GE-Proton11-11-x86_64");
        std::fs::create_dir_all(&ge).unwrap();
        std::fs::write(ge.join("proton"), "#!/usr/bin/env python3\n").unwrap();
        std::fs::write(ge.join("version"), "1792432585 GE-Proton11-11\n").unwrap();

        let env: BTreeMap<String, String> =
            [("PATH".to_string(), bin.display().to_string()), ("HOME".to_string(), home.display().to_string())].into();
        let probe = Probe::fixture(&root, env);
        let deps = detect(&probe, &[]);
        let get = |id: &str| deps.iter().find(|d| d.id == id).unwrap_or_else(|| panic!("{id} missing"));

        assert!(get("wine").found);
        assert_eq!(get("wine").version.as_deref(), Some("11.20"));
        assert_eq!(get("wine").path.as_deref(), Some(bin.join("wine").as_path()));
        assert!(!get("wine").required, "Proton found -> wine optional");
        assert_eq!(get("winetricks").version.as_deref(), Some("20260125"));
        assert_eq!(get("gamescope").version.as_deref(), Some("3.17.0"));
        assert!(!get("umu-run").found);
        assert!(!get("umu-run").install_hints.is_empty());
        assert!(get("umu-run").install_hints.iter().any(|h| h.distro == "arch" && h.command.contains("umu-launcher")));
        assert!(get("proton").found);
        assert_eq!(get("proton").details.as_deref(), Some("GE-Proton11-11-x86_64 (GE-Proton11-11)"));
        assert!(get("tesseract").found);
        assert_eq!(get("tesseract").version.as_deref(), Some("5.5.1"));
        assert!(get("tesseract-jpn").found);
        assert_eq!(get("tesseract-jpn").details.as_deref(), Some("jpn, jpn_vert"));
        assert!(get("xdg-desktop-portal").found);
        assert_eq!(get("xdg-desktop-portal").details.as_deref(), Some("backends: gnome, gtk"));
        assert!(get("ja-locale").found);
        assert!(get("ja-fonts").found);
        assert_eq!(get("ja-fonts").details.as_deref(), Some("3 font families, e.g. Noto Sans CJK JP"));
        assert!(!get("rt-upscaler").found);
        assert!(!get("ollama").found);
        assert!(deps.iter().all(|d| d.id != "flatpak-spawn"), "only inside flatpak");
        assert!(deps.iter().all(|d| d.install_hints.iter().all(|h| !h.command.is_empty())));

        // Inside Flatpak: flatpak-spawn becomes a required entry.
        std::fs::write(root.join(".flatpak-info"), "[Application]\nname=x\n").unwrap();
        let deps = detect(&probe, &[]);
        let fs = deps.iter().find(|d| d.id == "flatpak-spawn").unwrap();
        assert!(fs.required && !fs.found);
    }

    #[test]
    fn wine_required_without_proton() {
        let dir = tempfile::tempdir().unwrap();
        let env: BTreeMap<String, String> = [
            ("PATH".to_string(), dir.path().join("nope").display().to_string()),
            ("HOME".to_string(), dir.path().display().to_string()),
        ]
        .into();
        let deps = detect(&Probe::fixture(dir.path(), env), &[]);
        let wine = deps.iter().find(|d| d.id == "wine").unwrap();
        assert!(wine.required && !wine.found);
        let json = serde_json::to_value(wine).unwrap();
        assert_eq!(json["installHints"][0]["distro"], "arch");
        assert_eq!(json["category"], "runtime");
    }
}
