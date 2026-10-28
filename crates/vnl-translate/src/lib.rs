//! Translation subsystem: TextSource / HookAdapter / OcrAdapter /
//! TranslationProvider / TranslationPipeline / OverlayController.
//! Owned by Agent 4.
//!
//! CONTRACT (used by src-tauri/src/commands/session.rs — keep signatures stable,
//! you may add methods/fields freely).

use std::path::PathBuf;
use std::sync::Arc;

use vnl_core::{AppError, AppPaths, EventSink, GameId, PrefixCommandBuilder, SessionId, TranslationConfig, TranslationDefaults};

/// Context for attaching text extraction to a running game session.
pub struct SessionContext {
    pub session_id: SessionId,
    pub game_id: GameId,
    /// Already merged with app defaults via [`resolve_config`].
    pub config: TranslationConfig,
    pub game_exe: PathBuf,
    /// Builds commands that run inside the game's prefix (hook hosts...).
    pub prefix: Arc<dyn PrefixCommandBuilder>,
}

/// Fill per-game `None` fields from the app-level defaults.
pub fn resolve_config(game: &TranslationConfig, defaults: &TranslationDefaults) -> TranslationConfig {
    let mut cfg = game.clone();
    if cfg.source_language.is_none() {
        cfg.source_language = Some(defaults.source_language.clone());
    }
    if cfg.target_language.is_none() {
        cfg.target_language = defaults.target_language.clone();
    }
    if cfg.provider_id.is_none() {
        cfg.provider_id = defaults.provider_id.clone();
    }
    cfg
}

pub struct TranslationService {
    #[allow(dead_code)]
    paths: AppPaths,
    #[allow(dead_code)]
    events: Arc<dyn EventSink>,
}

impl TranslationService {
    pub fn new(paths: &AppPaths, events: Arc<dyn EventSink>) -> Result<Self, AppError> {
        Ok(Self { paths: paths.clone(), events })
    }

    /// Start hook/OCR sources for a session. Called only when
    /// `config.enabled && config.extraction != Disabled`. Failures are
    /// reported to the user but never abort the game session.
    pub async fn start_session(&self, ctx: SessionContext) -> Result<(), AppError> {
        let _ = ctx;
        Err(AppError::not_implemented("text extraction"))
    }

    pub async fn stop_session(&self, session_id: &str) {
        let _ = session_id;
    }
}
