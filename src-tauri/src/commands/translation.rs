//! Tauri commands for the Agent 4 (translation) area. Owned by Agent 4 (translation).
//! Names and argument names mirror `src/contract/translation.ts`.

use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use vnl_core::{AppError, GameId, OverlayConfig, TranslationConfig, TranslationDefaults, TranslationLine};
use vnl_translate::overlay::{OverlayController, sanitize};
use vnl_translate::types::{
    BackendInfo, GlossaryEntry, Language, OcrCaptureRequest, ProviderInfo, TranslateRequest, TranslationSessionStatus,
};

use crate::state::AppState;

pub const OVERLAY_LABEL: &str = "overlay";
/// Event sent to the overlay window with its `OverlayConfig` after `overlay_show`.
pub const OVERLAY_CONFIG_EVENT: &str = "vnl://overlay-config";

/// Per-game translation config merged with app defaults, plus the defaults.
fn configs(
    state: &AppState,
    game_id: Option<GameId>,
) -> Result<(Option<TranslationConfig>, TranslationDefaults), AppError> {
    let defaults = state.library.get_app_settings()?.translation;
    let game = match game_id {
        Some(id) => Some(vnl_translate::resolve_config(&state.library.get_settings(id)?.translation, &defaults)),
        None => None,
    };
    Ok((game, defaults))
}

/// Game id from the request, or from the running session.
fn effective_game_id(state: &AppState, game_id: Option<GameId>, session_id: Option<&str>) -> Option<GameId> {
    game_id.or_else(|| session_id.and_then(|s| state.runtime.session(s)).map(|s| s.game_id))
}

#[tauri::command]
pub async fn translation_providers(state: State<'_, AppState>) -> Result<Vec<ProviderInfo>, AppError> {
    Ok(state.translation.providers().await)
}

/// `api_key`: `None`/`null` = keep the stored key, `""` = delete it, other = store
/// it in the Secret Service (Tauri cannot tell a missing argument from `null`).
#[tauri::command]
pub async fn translation_configure_provider(
    state: State<'_, AppState>,
    provider_id: String,
    config: Map<String, Value>,
    api_key: Option<String>,
) -> Result<ProviderInfo, AppError> {
    let key = match api_key {
        None => None,
        Some(k) if k.trim().is_empty() => Some(None),
        Some(k) => Some(Some(k)),
    };
    state.translation.configure_provider(&provider_id, config, key).await
}

#[tauri::command]
pub async fn translation_test_provider(
    state: State<'_, AppState>,
    provider_id: String,
    target_language: String,
) -> Result<TranslationLine, AppError> {
    state.translation.test_provider(&provider_id, &target_language).await
}

#[tauri::command]
pub async fn translation_translate(
    state: State<'_, AppState>,
    request: TranslateRequest,
) -> Result<TranslationLine, AppError> {
    let mut request = request;
    request.game_id = effective_game_id(&state, request.game_id, request.session_id.as_deref());
    let (game, defaults) = configs(&state, request.game_id)?;
    state.translation.translate(request, game.as_ref(), Some(&defaults)).await
}

#[tauri::command]
pub async fn translation_languages(state: State<'_, AppState>) -> Result<Vec<Language>, AppError> {
    Ok(state.translation.languages())
}

#[tauri::command]
pub async fn translation_glossary_list(
    state: State<'_, AppState>,
    game_id: Option<i64>,
) -> Result<Vec<GlossaryEntry>, AppError> {
    state.translation.glossary_list(game_id)
}

#[tauri::command]
pub async fn translation_glossary_upsert(
    state: State<'_, AppState>,
    entry: GlossaryEntry,
) -> Result<GlossaryEntry, AppError> {
    state.translation.glossary_upsert(&entry)
}

#[tauri::command]
pub async fn translation_glossary_delete(state: State<'_, AppState>, id: i64) -> Result<(), AppError> {
    state.translation.glossary_delete(id)
}

#[tauri::command]
pub async fn translation_history(
    state: State<'_, AppState>,
    game_id: Option<i64>,
    limit: Option<u32>,
) -> Result<Vec<TranslationLine>, AppError> {
    state.translation.history(game_id, limit)
}

#[tauri::command]
pub async fn translation_history_clear(state: State<'_, AppState>, game_id: Option<i64>) -> Result<(), AppError> {
    state.translation.history_clear(game_id)
}

#[tauri::command]
pub async fn translation_ocr_backends(state: State<'_, AppState>) -> Result<Vec<BackendInfo>, AppError> {
    Ok(state.translation.ocr_backends().await)
}

#[tauri::command]
pub async fn translation_ocr_capture(
    state: State<'_, AppState>,
    request: OcrCaptureRequest,
) -> Result<TranslationLine, AppError> {
    let mut request = request;
    request.game_id = effective_game_id(&state, request.game_id, request.session_id.as_deref());
    let (game, defaults) = configs(&state, request.game_id)?;
    let exe = match request.game_id {
        Some(id) => Some(state.library.get_game(id)?.exe_path),
        None => None,
    };
    state.translation.ocr_capture(request, game.as_ref(), Some(&defaults), exe.as_deref()).await
}

#[tauri::command]
pub async fn translation_hook_backends(state: State<'_, AppState>) -> Result<Vec<BackendInfo>, AppError> {
    Ok(state.translation.hook_backends())
}

#[tauri::command]
pub async fn translation_session_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<TranslationSessionStatus, AppError> {
    Ok(state.translation.session_status(&session_id).await)
}

/// Tauri implementation of [`OverlayController`]: a transparent, undecorated,
/// always-on-top window labelled `overlay` loading `index.html#/overlay`.
///
/// Wayland limitations (GNOME/Mutter): applications cannot position their own
/// windows and "always on top" is only a hint — a fullscreen game (or a
/// fullscreen gamescope window) normally stays above the overlay. Run the game
/// windowed/borderless and move the overlay by dragging it. Transparency needs
/// a compositor (always present on Wayland).
pub struct TauriOverlay {
    pub app: AppHandle,
}

impl OverlayController for TauriOverlay {
    fn show(&self, config: &OverlayConfig) -> Result<(), AppError> {
        let config = sanitize(config);
        let window = match self.app.get_webview_window(OVERLAY_LABEL) {
            Some(w) => w,
            None => WebviewWindowBuilder::new(&self.app, OVERLAY_LABEL, WebviewUrl::App("index.html#/overlay".into()))
                .title("Yomika – Translation overlay")
                .inner_size(960.0, 220.0)
                .min_inner_size(240.0, 80.0)
                .transparent(true)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(true)
                .focused(false)
                .visible_on_all_workspaces(true)
                .shadow(false)
                .build()
                .map_err(|e| AppError::internal(format!("cannot create the overlay window: {e}")))?,
        };
        window.show().map_err(|e| AppError::internal(format!("cannot show the overlay: {e}")))?;
        // Re-assert the hint: some compositors drop it when the window is re-shown.
        let _ = window.set_always_on_top(true);
        let _ = self.app.emit_to(OVERLAY_LABEL, OVERLAY_CONFIG_EVENT, &config);
        Ok(())
    }

    fn hide(&self) -> Result<(), AppError> {
        if let Some(w) = self.app.get_webview_window(OVERLAY_LABEL) {
            w.hide().map_err(|e| AppError::internal(format!("cannot hide the overlay: {e}")))?;
        }
        Ok(())
    }

    fn is_visible(&self) -> bool {
        self.app.get_webview_window(OVERLAY_LABEL).and_then(|w| w.is_visible().ok()).unwrap_or(false)
    }
}

#[tauri::command]
pub async fn overlay_show(state: State<'_, AppState>, config: Option<OverlayConfig>) -> Result<(), AppError> {
    TauriOverlay { app: state.app.clone() }.show(&config.unwrap_or_default())
}

#[tauri::command]
pub async fn overlay_hide(state: State<'_, AppState>) -> Result<(), AppError> {
    TauriOverlay { app: state.app.clone() }.hide()
}
