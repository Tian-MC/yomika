//! Catalog of Wine prefix components useful for Japanese visual novels, with
//! installed-state detection. Installation itself is done by the runtime's
//! `runtime_prefix_tool` (winetricks/protontricks) — this module is read-only.
//!
//! Verb names were verified against `winetricks list-all` (winetricks
//! 20260125). Detection sources:
//! * `<prefix>/winetricks.log` — winetricks appends each executed verb (with
//!   its arguments) as one line (`winetricks_stats_log_command`), and also
//!   `w_workaround_wine_bug-N` lines; the first word of each line is the verb.
//! * Font files in `drive_c/windows/Fonts` (`fonts` on some old prefixes),
//!   using the file names the verbs install (e.g. `sourcehansans.ttc` for
//!   `fakejapanese`, `msgothic.ttc` shipped by Proton).
//! * Native DLLs in `system32`/`syswow64`: Wine's own builtin stubs contain
//!   the marker `Wine builtin DLL` (verified in a real prefix), so a DLL
//!   without that marker was installed by a redistributable.
//!
//! Both a plain `WINEPREFIX` and a Proton compat-data dir (`<dir>/pfx/`) are
//! accepted.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vnl_core::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixComponent {
    pub id: String,
    pub name: String,
    /// `fonts`, `codecs`, `runtime`.
    pub category: String,
    pub winetricks_verbs: Vec<String>,
    /// `None` when the prefix is not initialised yet.
    pub installed: Option<bool>,
    pub description: String,
    /// What the detection was based on (`winetricks.log`, a file name...).
    pub detected_by: Option<String>,
}

/// One detection signal.
enum Sig {
    /// File in the Fonts dir (case-insensitive).
    Font(&'static str),
    /// Native (non-builtin) DLL in system32 or syswow64.
    NativeDll(&'static str),
    /// Any file whose lowercase name starts with the prefix + ends with `.dll`, native.
    NativeDllPrefix(&'static str),
    /// Path relative to drive_c exists.
    File(&'static str),
    /// `system.reg` contains this substring.
    Reg(&'static str),
}

struct Def {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    verbs: &'static [&'static str],
    description: &'static str,
    signals: &'static [Sig],
}

const CATALOG: &[Def] = &[
    Def {
        id: "cjkfonts",
        name: "CJK fonts",
        category: "fonts",
        verbs: &["cjkfonts"],
        description: "All Chinese/Japanese/Korean fonts and aliases (fakechinese + fakejapanese + fakekorean + unifont). Fixes tofu/boxes in menus and dialogue.",
        signals: &[],
    },
    Def {
        id: "fakejapanese",
        name: "Japanese font aliases (Source Han Sans)",
        category: "fonts",
        verbs: &["fakejapanese"],
        description: "Maps MS Gothic/Mincho, Meiryo, Yu Gothic (and ＭＳ ゴシック etc.) to Source Han Sans. The usual fix for missing Japanese text.",
        signals: &[Sig::Font("sourcehansans.ttc")],
    },
    Def {
        id: "fakejapanese_ipamona",
        name: "Japanese font aliases (IPAMona)",
        category: "fonts",
        verbs: &["fakejapanese_ipamona"],
        description: "Maps MS Gothic/Mincho to IPAMona fonts (metric-compatible, good for 2ch-style ASCII art and older VNs).",
        signals: &[Sig::Font("ipag-mona.ttf")],
    },
    Def {
        id: "fakejapanese_vlgothic",
        name: "Japanese font aliases (VL Gothic)",
        category: "fonts",
        verbs: &["fakejapanese_vlgothic"],
        description: "Maps Meiryo to VL Gothic.",
        signals: &[Sig::Font("vl-gothic-regular.ttf")],
    },
    Def {
        id: "takao",
        name: "Takao fonts",
        category: "fonts",
        verbs: &["takao"],
        description: "Takao Gothic/Mincho Japanese fonts.",
        signals: &[Sig::Font("takaogothic.ttf")],
    },
    Def {
        id: "meiryo",
        name: "Meiryo",
        category: "fonts",
        verbs: &["meiryo"],
        description: "Microsoft Meiryo font (used by many newer VN engines).",
        signals: &[Sig::Font("meiryo.ttc")],
    },
    Def {
        id: "msgothic",
        name: "MS Gothic replacement (Proton)",
        category: "fonts",
        verbs: &[],
        description: "Proton/GE-Proton ship an msgothic.ttc replacement in new prefixes; no winetricks verb needed.",
        signals: &[Sig::Font("msgothic.ttc")],
    },
    Def {
        id: "corefonts",
        name: "MS core fonts",
        category: "fonts",
        verbs: &["corefonts"],
        description: "Arial, Times New Roman, Courier New, ... (some launchers/config tools expect them).",
        signals: &[Sig::Font("corefonts.installed")],
    },
    Def {
        id: "quartz",
        name: "DirectShow (quartz)",
        category: "codecs",
        verbs: &["quartz"],
        description: "Native quartz.dll for DirectShow video playback (opening movies, OP/ED).",
        signals: &[Sig::NativeDll("quartz.dll")],
    },
    Def {
        id: "lavfilters",
        name: "LAV Filters",
        category: "codecs",
        verbs: &["lavfilters"],
        description: "DirectShow decoders for H.264/MPEG/WMV movies; fixes black/frozen video in many VNs.",
        signals: &[Sig::File("Program Files (x86)/LAV Filters"), Sig::File("Program Files/LAV Filters")],
    },
    Def {
        id: "wmp9",
        name: "Windows Media Player 9",
        category: "codecs",
        verbs: &["wmp9"],
        description: "WMV/WMA codecs for older VNs (32-bit prefixes; codecs are not supported in win64 prefixes).",
        signals: &[],
    },
    Def {
        id: "wmp11",
        name: "Windows Media Player 11",
        category: "codecs",
        verbs: &["wmp11"],
        description: "WMV/WMA playback through the WMP runtime, needed by games that embed WMP.",
        signals: &[],
    },
    Def {
        id: "mf",
        name: "Media Foundation",
        category: "codecs",
        verbs: &["mf"],
        description: "Native Media Foundation (mf.dll) for newer engines' video playback. GE-Proton usually does not need it.",
        signals: &[Sig::NativeDll("mf.dll")],
    },
    Def {
        id: "d3dx9",
        name: "DirectX 9 (d3dx9)",
        category: "runtime",
        verbs: &["d3dx9"],
        description: "d3dx9_*.dll helper libraries from the DirectX June 2010 redistributable.",
        signals: &[Sig::NativeDllPrefix("d3dx9_")],
    },
    Def {
        id: "vcrun2005",
        name: "Visual C++ 2005",
        category: "runtime",
        verbs: &["vcrun2005"],
        description: "MSVC 8 runtime (msvcr80/mfc80).",
        signals: &[],
    },
    Def {
        id: "vcrun2008",
        name: "Visual C++ 2008",
        category: "runtime",
        verbs: &["vcrun2008"],
        description: "MSVC 9 runtime (msvcr90/mfc90).",
        signals: &[],
    },
    Def {
        id: "vcrun2010",
        name: "Visual C++ 2010",
        category: "runtime",
        verbs: &["vcrun2010"],
        description: "MSVC 10 runtime (msvcr100/mfc100).",
        signals: &[Sig::NativeDll("msvcr100.dll")],
    },
    Def {
        id: "vcrun2012",
        name: "Visual C++ 2012",
        category: "runtime",
        verbs: &["vcrun2012"],
        description: "MSVC 11 runtime (msvcr110/mfc110).",
        signals: &[Sig::NativeDll("msvcr110.dll")],
    },
    Def {
        id: "vcrun2013",
        name: "Visual C++ 2013",
        category: "runtime",
        verbs: &["vcrun2013"],
        description: "MSVC 12 runtime (msvcr120/mfc120).",
        signals: &[Sig::NativeDll("msvcr120.dll")],
    },
    Def {
        id: "vcrun2022",
        name: "Visual C++ 2015–2022",
        category: "runtime",
        verbs: &["vcrun2022", "vcrun2019", "vcrun2017", "vcrun2015"],
        description: "MSVC 14 runtime (vcruntime140/msvcp140); vcrun2022 supersedes 2015/2017/2019.",
        signals: &[Sig::NativeDll("msvcp140.dll")],
    },
    Def {
        id: "dotnet40",
        name: ".NET Framework 4.0",
        category: "runtime",
        verbs: &["dotnet40"],
        description: "Only for games/tools written in .NET that fail with wine-mono.",
        signals: &[],
    },
    Def {
        id: "dotnet48",
        name: ".NET Framework 4.8",
        category: "runtime",
        verbs: &["dotnet48"],
        description: "Only for .NET games/tools (some patchers and config utilities) that fail with wine-mono.",
        signals: &[Sig::Reg("NET Framework Setup\\\\NDP\\\\v4\\\\Full")],
    },
];

/// Resolve a WINEPREFIX from a plain prefix or a Proton compat-data dir.
pub fn resolve_prefix(path: &Path) -> PathBuf {
    let pfx = path.join("pfx");
    if pfx.join("drive_c").is_dir() || (!path.join("drive_c").is_dir() && pfx.is_dir()) {
        pfx
    } else {
        path.to_path_buf()
    }
}

/// Verbs recorded in a winetricks.log.
pub fn parse_winetricks_log(text: &str) -> BTreeSet<String> {
    text.lines().filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect()
}

fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

fn is_native_dll(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut buf = [0u8; 512];
    let n = f.read(&mut buf).unwrap_or(0);
    if n < 2 || &buf[..2] != b"MZ" {
        return false;
    }
    let hay = &buf[..n];
    let builtin = |m: &[u8]| hay.windows(m.len()).any(|w| w == m);
    !builtin(b"Wine builtin DLL") && !builtin(b"Wine placeholder DLL")
}

struct PrefixDirs {
    drive_c: PathBuf,
    fonts: Option<PathBuf>,
    dll_dirs: Vec<PathBuf>,
}

impl PrefixDirs {
    fn new(prefix: &Path) -> Self {
        let drive_c = prefix.join("drive_c");
        let windows = find_ci(&drive_c, "windows").unwrap_or_else(|| drive_c.join("windows"));
        let fonts = find_ci(&windows, "Fonts");
        let dll_dirs = ["system32", "syswow64"].iter().filter_map(|d| find_ci(&windows, d)).collect();
        Self { drive_c, fonts, dll_dirs }
    }

    fn check(&self, prefix: &Path, sig: &Sig) -> Option<String> {
        match sig {
            Sig::Font(f) => self.fonts.as_ref().and_then(|d| find_ci(d, f)).map(|_| format!("font {f}")),
            Sig::NativeDll(dll) => self
                .dll_dirs
                .iter()
                .filter_map(|d| find_ci(d, dll))
                .find(|p| is_native_dll(p))
                .map(|_| format!("native {dll}")),
            Sig::NativeDllPrefix(pre) => self.dll_dirs.iter().find_map(|d| {
                std::fs::read_dir(d).ok()?.flatten().find_map(|e| {
                    let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                    (n.starts_with(pre) && n.ends_with(".dll") && is_native_dll(&e.path()))
                        .then(|| format!("native {n}"))
                })
            }),
            Sig::File(rel) => self.drive_c.join(rel).exists().then(|| format!("C:/{rel}")),
            Sig::Reg(needle) => std::fs::read_to_string(prefix.join("system.reg"))
                .ok()
                .filter(|r| r.contains(needle))
                .map(|_| "registry".to_string()),
        }
    }
}

/// Catalog with installed state for `prefix_path`.
pub fn components(prefix_path: &Path) -> Result<Vec<PrefixComponent>, AppError> {
    if !prefix_path.exists() {
        return Err(AppError::not_found(format!("Prefix '{}' does not exist", prefix_path.display())));
    }
    if !prefix_path.is_dir() {
        return Err(AppError::invalid(format!("'{}' is not a directory", prefix_path.display())));
    }
    let prefix = resolve_prefix(prefix_path);
    let initialized = prefix.join("drive_c").is_dir();
    let log =
        std::fs::read_to_string(prefix.join("winetricks.log")).map(|t| parse_winetricks_log(&t)).unwrap_or_default();
    let dirs = PrefixDirs::new(&prefix);

    Ok(CATALOG
        .iter()
        .map(|def| {
            let (installed, detected_by) = if !initialized {
                (None, None)
            } else if let Some(v) = def.verbs.iter().find(|v| log.contains(**v)) {
                (Some(true), Some(format!("winetricks.log ({v})")))
            } else if let Some(d) = def.signals.iter().find_map(|s| dirs.check(&prefix, s)) {
                (Some(true), Some(d))
            } else {
                (Some(false), None)
            };
            PrefixComponent {
                id: def.id.into(),
                name: def.name.into(),
                category: def.category.into(),
                winetricks_verbs: def.verbs.iter().take(1).map(|v| v.to_string()).collect(),
                installed,
                description: def.description.into(),
                detected_by,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(dir: &Path) -> PathBuf {
        let p = dir.to_path_buf();
        for d in ["drive_c/windows/Fonts", "drive_c/windows/system32", "drive_c/windows/syswow64"] {
            std::fs::create_dir_all(p.join(d)).unwrap();
        }
        p
    }

    fn get<'a>(c: &'a [PrefixComponent], id: &str) -> &'a PrefixComponent {
        c.iter().find(|x| x.id == id).unwrap()
    }

    #[test]
    fn log_parsing() {
        let log = "w_workaround_wine_bug-12345\ncjkfonts\nvcrun2019\nfontsmooth=rgb\nlavfilters --force\r\n\n";
        let v = parse_winetricks_log(log);
        assert!(v.contains("cjkfonts") && v.contains("vcrun2019") && v.contains("lavfilters"));
        assert!(v.contains("fontsmooth=rgb"));
        assert!(!v.contains("--force"));
    }

    #[test]
    fn detection_in_plain_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let p = prefix(dir.path());
        std::fs::write(p.join("winetricks.log"), "cjkfonts\nvcrun2019\n").unwrap();
        std::fs::write(p.join("drive_c/windows/Fonts/SourceHanSans.ttc"), "x").unwrap();
        // Builtin stub (must NOT count) vs. native DLL (counts).
        std::fs::write(p.join("drive_c/windows/system32/quartz.dll"), b"MZ\x90\0........Wine builtin DLL\0").unwrap();
        std::fs::write(
            p.join("drive_c/windows/syswow64/d3dx9_43.dll"),
            b"MZ\x90\0This program cannot be run in DOS mode",
        )
        .unwrap();
        std::fs::create_dir_all(p.join("drive_c/Program Files (x86)/LAV Filters")).unwrap();

        let c = components(&p).unwrap();
        assert_eq!(get(&c, "cjkfonts").installed, Some(true));
        assert_eq!(get(&c, "cjkfonts").detected_by.as_deref(), Some("winetricks.log (cjkfonts)"));
        assert_eq!(get(&c, "vcrun2022").installed, Some(true), "vcrun2019 counts for the 2015-2022 runtime");
        assert_eq!(get(&c, "vcrun2022").winetricks_verbs, vec!["vcrun2022"]);
        assert_eq!(get(&c, "fakejapanese").installed, Some(true), "case-insensitive font match");
        assert_eq!(get(&c, "quartz").installed, Some(false), "builtin stub is not native");
        assert_eq!(get(&c, "d3dx9").installed, Some(true));
        assert_eq!(get(&c, "d3dx9").detected_by.as_deref(), Some("native d3dx9_43.dll"));
        assert_eq!(get(&c, "lavfilters").installed, Some(true));
        assert_eq!(get(&c, "wmp11").installed, Some(false));
        assert!(c.iter().all(|x| x.id == "msgothic" || !x.winetricks_verbs.is_empty()));
    }

    #[test]
    fn proton_compatdata_and_uninitialised() {
        let dir = tempfile::tempdir().unwrap();
        let pfx = prefix(&dir.path().join("pfx"));
        std::fs::write(pfx.join("drive_c/windows/Fonts/msgothic.ttc"), "x").unwrap();
        std::fs::write(pfx.join("winetricks.log"), "lavfilters\n").unwrap();
        let c = components(dir.path()).unwrap();
        assert_eq!(get(&c, "msgothic").installed, Some(true));
        assert_eq!(get(&c, "lavfilters").installed, Some(true));

        let empty = tempfile::tempdir().unwrap();
        let c = components(empty.path()).unwrap();
        assert!(c.iter().all(|x| x.installed.is_none()));

        assert_eq!(components(Path::new("/nonexistent/prefix")).unwrap_err().code, vnl_core::ErrorCode::NotFound);
        let json = serde_json::to_value(&c[0]).unwrap();
        assert!(json.get("winetricksVerbs").is_some());
    }
}
