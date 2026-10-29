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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HookConfig {
    /// Hook backend id, e.g. `"lunahost"`.
    pub backend: Option<String>,
    /// Optional H-code / hook code.
    pub hook_code: Option<String>,
    pub attach_delay_ms: u64,
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
}
