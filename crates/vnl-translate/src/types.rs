//! Serializable types exchanged with the UI. Mirrored in
//! `src/contract/translation.ts` — change both together.

use serde::{Deserialize, Serialize};
use vnl_core::{CaptureRegion, GameId, SessionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Local,
    Online,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    String,
    Url,
    Number,
    Boolean,
    Select,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigField {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    pub required: bool,
    pub default: serde_json::Value,
}

impl ProviderConfigField {
    pub fn new(key: &str, label: &str, kind: FieldKind, required: bool, default: serde_json::Value) -> Self {
        Self { key: key.into(), label: label.into(), kind, options: None, required, default }
    }

    pub fn with_options(mut self, options: &[&str]) -> Self {
        self.options = Some(options.iter().map(|s| s.to_string()).collect());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    /// True when using it sends text to a remote server.
    pub sends_data_remotely: bool,
    pub requires_api_key: bool,
    pub has_api_key: bool,
    pub configured: bool,
    pub available: bool,
    /// Current non-secret configuration values.
    pub config: serde_json::Map<String, serde_json::Value>,
    pub fields: Vec<ProviderConfigField>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Language {
    pub code: String,
    pub name: String,
    pub native_name: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TranslateRequest {
    pub text: String,
    pub source_language: Option<String>,
    pub target_language: Option<String>,
    pub provider_id: Option<String>,
    pub game_id: Option<GameId>,
    pub session_id: Option<SessionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GlossaryKind {
    #[default]
    Term,
    CharacterName,
}

impl GlossaryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GlossaryKind::Term => "term",
            GlossaryKind::CharacterName => "characterName",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s == "characterName" { GlossaryKind::CharacterName } else { GlossaryKind::Term }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryEntry {
    /// 0 (or negative) = new entry on upsert.
    #[serde(default)]
    pub id: i64,
    /// `None` = global entry.
    #[serde(default)]
    pub game_id: Option<GameId>,
    #[serde(default)]
    pub kind: GlossaryKind,
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
    /// Why it is unavailable / what is missing / what was detected.
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OcrCaptureRequest {
    pub game_id: Option<GameId>,
    pub session_id: Option<SessionId>,
    pub region: Option<CaptureRegion>,
    pub backend: Option<String>,
    /// Translate the recognised text (default true).
    pub translate: bool,
    /// Capture method id (`x11`, `portal`, `file`); default `x11` on X11/XWayland.
    pub capture: Option<String>,
    /// X11 display to capture (e.g. gamescope's nested `:2`); default `$DISPLAY`.
    pub display: Option<String>,
    /// X11 window id (e.g. `0x3000003`); default: the game's window found by
    /// its WM_CLASS (= exe name under Wine), else the root window.
    pub window_id: Option<String>,
    /// OCR an existing image instead of capturing the screen.
    pub image_path: Option<std::path::PathBuf>,
}

impl Default for OcrCaptureRequest {
    fn default() -> Self {
        Self {
            game_id: None,
            session_id: None,
            region: None,
            backend: None,
            translate: true,
            capture: None,
            display: None,
            window_id: None,
            image_path: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextSourceState {
    #[default]
    Inactive,
    Starting,
    Active,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookThread {
    pub id: String,
    pub selector: vnl_core::HookThreadSelection,
    pub preview: String,
    pub received: u64,
    #[serde(default)]
    pub addr: String,
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub is_filtered: bool,
    #[serde(default)]
    pub is_preferred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSessionStatus {
    pub session_id: SessionId,
    pub hook: TextSourceState,
    pub ocr: TextSourceState,
    pub diagnostics: Vec<String>,
    pub hook_threads: Vec<HookThread>,
    pub selected_hook_thread: Option<String>,
}

impl TranslationSessionStatus {
    pub fn inactive(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            hook_threads: Vec::new(),
            selected_hook_thread: None,
            hook: TextSourceState::Inactive,
            ocr: TextSourceState::Inactive,
            diagnostics: Vec::new(),
        }
    }
}
