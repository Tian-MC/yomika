//! Per-game and application settings. Every struct is `#[serde(default)]` so
//! settings stored by older versions keep deserializing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- runtime

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerRef {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PrefixChoice {
    /// One prefix per game, created lazily under the managed prefixes dir.
    #[default]
    Dedicated,
    /// A registered shared prefix (`PrefixInfo::id`).
    Shared { id: String },
    /// An arbitrary existing prefix path. Never modified destructively.
    Custom { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtonLauncher {
    /// Use umu-run when available, otherwise call the `proton` script directly.
    #[default]
    Auto,
    Umu,
    Direct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LocaleConfig {
    /// Run with a Japanese locale (`LANG`/`LC_ALL=ja_JP.UTF-8`).
    pub japanese_locale: bool,
    /// Custom locale string, overrides `japanese_locale` when set.
    pub locale: Option<String>,
    /// `TZ` value, e.g. `Asia/Tokyo`.
    pub timezone: Option<String>,
}

impl Default for LocaleConfig {
    fn default() -> Self {
        Self { japanese_locale: true, locale: None, timezone: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProtonOptions {
    pub launcher: ProtonLauncher,
    /// `PROTON_USE_WINED3D=1`
    pub use_wined3d: bool,
    /// `PROTON_NO_ESYNC=1`
    pub no_esync: bool,
    /// `PROTON_NO_FSYNC=1`
    pub no_fsync: bool,
    /// `PROTON_LOG=1`
    pub enable_log: bool,
    /// umu `GAMEID`; defaults to `umu-default` / `0` when unset.
    pub umu_game_id: Option<String>,
}

/// A secondary program started next to the game (e.g. a text hooker, a
/// save tool). Windows programs run in the game's prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AuxProgram {
    pub id: String,
    pub name: String,
    pub exe_path: PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    /// true: Windows program run inside the prefix; false: native Linux program.
    pub windows: bool,
    /// Start automatically after the game starts.
    pub autostart: bool,
    pub delay_ms: u64,
    /// Terminate this program when the game session ends.
    pub stop_with_game: bool,
    pub env: BTreeMap<String, String>,
}

impl Default for AuxProgram {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            exe_path: PathBuf::new(),
            args: Vec::new(),
            working_dir: None,
            windows: true,
            autostart: false,
            delay_ms: 0,
            stop_with_game: true,
            env: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RuntimeConfig {
    /// `None` = use `AppSettings::default_runner`.
    pub runner: Option<RunnerRef>,
    pub prefix: PrefixChoice,
    pub locale: LocaleConfig,
    /// Extra environment variables (applied last, override everything).
    pub env: BTreeMap<String, String>,
    /// DLL overrides, e.g. `{"d3d9": "n,b"}` -> `WINEDLLOVERRIDES`.
    pub dll_overrides: BTreeMap<String, String>,
    pub proton: ProtonOptions,
    pub auxiliary: Vec<AuxProgram>,
    /// Keep the session alive until the prefix's wineserver exits. Needed for
    /// games whose launcher exe spawns the real game and exits immediately.
    pub wait_for_wineserver: bool,
}

// --------------------------------------------------------------- graphics

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowMode {
    #[default]
    Windowed,
    Fullscreen,
    Borderless,
}

/// gamescope `-S` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GamescopeScaler {
    #[default]
    Auto,
    Integer,
    Fit,
    Fill,
    Stretch,
}

/// gamescope `-F` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GamescopeFilter {
    #[default]
    Linear,
    Nearest,
    Fsr,
    Nis,
    Pixel,
    /// Snapdragon Game Super Resolution (gamescope >= 3.16).
    Sgsr,
    /// Anime4K 2x CNN UL; only in the gamescope-anime4k fork (`-F anime4k`).
    Anime4k,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GamescopeConfig {
    pub enabled: bool,
    /// Game (nested) resolution: `-w`/`-h`.
    pub game_width: Option<u32>,
    pub game_height: Option<u32>,
    /// Output resolution: `-W`/`-H`.
    pub output_width: Option<u32>,
    pub output_height: Option<u32>,
    pub window_mode: WindowMode,
    pub scaler: GamescopeScaler,
    pub filter: GamescopeFilter,
    /// FSR/NIS sharpness 0 (max) – 20 (min): `--sharpness`.
    pub sharpness: Option<u8>,
    /// `-r`
    pub fps_limit: Option<u32>,
    /// `-o`
    pub fps_limit_unfocused: Option<u32>,
    /// Connector name for `--prefer-output`, e.g. `DP-1`.
    pub prefer_output: Option<String>,
    pub force_grab_cursor: bool,
    pub hdr: bool,
    /// Opt-in, off by default: gamescope should do the scaling (set the game
    /// resolution). Switch a windowed game to its own fullscreen mode (one
    /// Alt+Enter, only if its window does not already cover the nested display).
    /// Windowed Wine games inside gamescope keep the empty Windows frame
    /// area in their X window, which shows up as black bars and a slight
    /// downscale; the game's fullscreen mode has no frame.
    pub auto_fullscreen: bool,
    /// Use this gamescope binary for the game instead of the automatic choice
    /// (application custom path, system, managed build).
    pub binary_path: Option<PathBuf>,
    /// Appended verbatim (as separate argv entries) before `--`.
    pub extra_args: Vec<String>,
}

/// Wine/Proton-GE fullscreen FSR hack (`WINE_FULLSCREEN_FSR`). This is NOT
/// gamescope upscaling; it only works with Proton-GE builds and fullscreen
/// games rendering below native resolution.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WineFsrConfig {
    pub enabled: bool,
    /// `WINE_FULLSCREEN_FSR_STRENGTH` 0–5.
    pub strength: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpscalerConfig {
    pub enabled: bool,
    /// Backend id, e.g. `"rt-upscaler"`.
    pub backend: Option<String>,
    pub model: Option<String>,
    pub preset: Option<String>,
    pub scale: Option<f32>,
    /// Title of the game window to attach to (RT-Upscaler `-t`).
    pub target_title: Option<String>,
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphicsConfig {
    pub gamescope: GamescopeConfig,
    pub wine_fsr: WineFsrConfig,
    pub upscaler: UpscalerConfig,
}

// ------------------------------------------------------------ translation

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtractionMode {
    #[default]
    Disabled,
    Hook,
    Ocr,
    HookAndOcr,
}

/// Stable CLI fingerprint, excluding process IDs, handles and hook addresses.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookThreadSelection {
    pub name: String,
    pub hook_code: String,
    pub context: String,
    pub context2: String,
}

/// A user hook code (`H…`, `R…` or `E…`, optionally written with a leading
/// `/`) stored per game and inserted after attaching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HookCodeEntry {
    pub code: String,
    /// Free-form note shown in the UI.
    pub label: Option<String>,
    pub enabled: bool,
}

impl Default for HookCodeEntry {
    fn default() -> Self {
        Self { code: String::new(), label: None, enabled: true }
    }
}

/// Per-game filters applied to hook threads and their text.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HookFilters {
    /// Thread names (case-insensitive, exact) that are flagged as filtered in
    /// the live list and never auto-selected. They can still be picked by hand.
    pub hidden_names: Vec<String>,
    /// Drop text of selected threads shorter than this many characters (0 = off).
    pub min_chars: u32,
    /// Drop text of selected threads without any CJK character (kana, kanji, hangul…).
    pub require_cjk: bool,
}

/// Per-game extraction settings of the hook host.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HookExtraction {
    /// Text codepage the host uses for narrow strings (`=<codepage>`, e.g. 932).
    pub codepage: Option<u32>,
    /// Host sentence flush delay in milliseconds (`+<ms>`).
    pub flush_delay_ms: Option<u32>,
    /// Local fragment merging: a growing text (new text starts with the
    /// pending one) replaces it instead of being emitted piece by piece, and the
    /// result is emitted after this many quiet milliseconds (0 = off, emit at once).
    pub merge_window_ms: u64,
    /// Identical consecutive text is normally dropped as a duplicate; when
    /// non-zero, it is accepted again after this many milliseconds (legit repeats).
    pub repeat_gap_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HookConfig {
    /// Hook backend id, e.g. `"lunahost"`.
    pub backend: Option<String>,
    /// Legacy single H-code. Still honored; new code should use [`Self::hook_codes`].
    pub hook_code: Option<String>,
    pub attach_delay_ms: u64,
    /// Legacy single selected thread; mirrors the first entry of [`Self::threads`].
    pub thread: Option<HookThreadSelection>,
    /// Hook codes inserted after attaching (in addition to the legacy `hook_code`).
    pub hook_codes: Vec<HookCodeEntry>,
    /// Threads forwarded to translation. Empty falls back to the legacy `thread`.
    pub threads: Vec<HookThreadSelection>,
    /// Preferred hook: auto-selected by `name` + `hook_code` (contexts ignored)
    /// when no saved thread matches the current run.
    pub preferred: Option<HookThreadSelection>,
    pub filters: HookFilters,
    pub extraction: HookExtraction,
}

impl HookConfig {
    /// Saved thread selection, honoring the legacy single-thread field.
    pub fn selected_threads(&self) -> Vec<HookThreadSelection> {
        if self.threads.is_empty() { self.thread.iter().cloned().collect() } else { self.threads.clone() }
    }

    /// Stores a selection in both the list and the legacy field.
    pub fn set_selected_threads(&mut self, threads: Vec<HookThreadSelection>) {
        self.thread = threads.first().cloned();
        self.threads = threads;
    }

    /// Effective hook codes: the legacy code first, then the enabled list
    /// entries, trimmed and without exact duplicates. Not validated.
    pub fn effective_hook_codes(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let candidates = self
            .hook_code
            .iter()
            .map(String::as_str)
            .chain(self.hook_codes.iter().filter(|e| e.enabled).map(|e| e.code.as_str()));
        for code in candidates {
            let code = code.trim();
            if !code.is_empty() && !out.iter().any(|c| c == code) {
                out.push(code.to_string());
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OcrConfig {
    /// OCR backend id, e.g. `"tesseract"`, `"ollama"`.
    pub backend: Option<String>,
    pub region: Option<CaptureRegion>,
    /// Automatic capture interval; 0 = manual capture only.
    pub interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OverlayConfig {
    pub enabled: bool,
    pub font_size: u32,
    pub opacity: f32,
    pub show_original: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self { enabled: false, font_size: 22, opacity: 0.85, show_original: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TranslationConfig {
    /// Master switch. When false nothing is extracted or translated.
    pub enabled: bool,
    pub extraction: ExtractionMode,
    /// BCP-47 code; `None` = app default.
    pub source_language: Option<String>,
    /// BCP-47 code; `None` = app default. There is deliberately no built-in
    /// default target language: the user must choose one.
    pub target_language: Option<String>,
    /// `None` = app default provider.
    pub provider_id: Option<String>,
    pub hook: HookConfig,
    pub ocr: OcrConfig,
    pub keep_history: bool,
    pub overlay: OverlayConfig,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            extraction: ExtractionMode::Disabled,
            source_language: None,
            target_language: None,
            provider_id: None,
            hook: HookConfig::default(),
            ocr: OcrConfig::default(),
            keep_history: true,
            overlay: OverlayConfig::default(),
        }
    }
}

// ---------------------------------------------------------- per-game + app

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameSettings {
    pub runtime: RuntimeConfig,
    pub graphics: GraphicsConfig,
    pub translation: TranslationConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TranslationDefaults {
    pub source_language: String,
    /// No default: chosen by the user in settings or onboarding.
    pub target_language: Option<String>,
    pub provider_id: Option<String>,
}

impl Default for TranslationDefaults {
    fn default() -> Self {
        Self { source_language: "ja".into(), target_language: None, provider_id: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VndbSettings {
    /// Allow network requests to api.vndb.org.
    pub enabled: bool,
    /// Blur covers with VNDB sexual rating >= this threshold (0–2). None = never blur.
    pub blur_sexual_threshold: Option<f64>,
}

impl Default for VndbSettings {
    fn default() -> Self {
        Self { enabled: true, blur_sexual_threshold: Some(1.0) }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub default_runner: Option<RunnerRef>,
    /// Override of the managed prefixes directory.
    pub prefixes_dir: Option<PathBuf>,
    /// UI language (BCP-47). `None` = follow system.
    pub ui_language: Option<String>,
    pub theme: Theme,
    pub translation: TranslationDefaults,
    /// Applied to newly imported games.
    pub default_graphics: GraphicsConfig,
    pub vndb: VndbSettings,
    /// Custom gamescope binary used for every game (per-game `binary_path` wins).
    pub gamescope_path: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamescope_does_the_scaling_by_default() {
        // The game must not be pushed to its own fullscreen mode unless the user opts in.
        assert!(!GamescopeConfig::default().auto_fullscreen);
        // Settings stored by older versions without the field keep the default.
        let cfg: GamescopeConfig = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(cfg.enabled && !cfg.auto_fullscreen);
    }
}
