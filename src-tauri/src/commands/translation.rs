//! Tauri commands for the Agent 4 (translation) area. Owned by Agent 4 (translation).
//! Stubs: replace `serde_json::Value` with real types from the subsystem crate,
//! keeping the command names and argument names from src/contract/*.ts.

#![allow(unused_variables)]

use serde_json::Value;
use tauri::State;
use vnl_core::AppError;

use crate::state::AppState;

#[tauri::command]
pub async fn translation_providers(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_providers"))
}

#[tauri::command]
pub async fn translation_configure_provider(state: State<'_, AppState>, provider_id: String, config: Value, api_key: Option<Option<String>>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_configure_provider"))
}

#[tauri::command]
pub async fn translation_test_provider(state: State<'_, AppState>, provider_id: String, target_language: String) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_test_provider"))
}

#[tauri::command]
pub async fn translation_translate(state: State<'_, AppState>, request: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_translate"))
}

#[tauri::command]
pub async fn translation_languages(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_languages"))
}

#[tauri::command]
pub async fn translation_glossary_list(state: State<'_, AppState>, game_id: Option<i64>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_glossary_list"))
}

#[tauri::command]
pub async fn translation_glossary_upsert(state: State<'_, AppState>, entry: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_glossary_upsert"))
}

#[tauri::command]
pub async fn translation_glossary_delete(state: State<'_, AppState>, id: i64) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_glossary_delete"))
}

#[tauri::command]
pub async fn translation_history(state: State<'_, AppState>, game_id: Option<i64>, limit: Option<u32>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_history"))
}

#[tauri::command]
pub async fn translation_history_clear(state: State<'_, AppState>, game_id: Option<i64>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_history_clear"))
}

#[tauri::command]
pub async fn translation_ocr_backends(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_ocr_backends"))
}

#[tauri::command]
pub async fn translation_ocr_capture(state: State<'_, AppState>, request: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_ocr_capture"))
}

#[tauri::command]
pub async fn translation_hook_backends(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_hook_backends"))
}

#[tauri::command]
pub async fn translation_session_status(state: State<'_, AppState>, session_id: String) -> Result<Value, AppError> {
    Err(AppError::not_implemented("translation_session_status"))
}

#[tauri::command]
pub async fn overlay_show(state: State<'_, AppState>, config: Option<Value>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("overlay_show"))
}

#[tauri::command]
pub async fn overlay_hide(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("overlay_hide"))
}
