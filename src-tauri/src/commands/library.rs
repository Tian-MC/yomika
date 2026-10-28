//! Tauri commands for the Agent 2 (library) area. Owned by Agent 2 (library).
//! Stubs: replace `serde_json::Value` with real types from the subsystem crate,
//! keeping the command names and argument names from src/contract/*.ts.

#![allow(unused_variables)]

use serde_json::Value;
use tauri::State;
use vnl_core::AppError;

use crate::state::AppState;

#[tauri::command]
pub async fn library_list_games(state: State<'_, AppState>, query: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("library_list_games"))
}

#[tauri::command]
pub async fn library_get_game(state: State<'_, AppState>, id: i64) -> Result<Value, AppError> {
    Err(AppError::not_implemented("library_get_game"))
}

#[tauri::command]
pub async fn library_scan_path(state: State<'_, AppState>, path: String) -> Result<Value, AppError> {
    Err(AppError::not_implemented("library_scan_path"))
}

#[tauri::command]
pub async fn library_import_game(state: State<'_, AppState>, request: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("library_import_game"))
}

#[tauri::command]
pub async fn library_update_game(state: State<'_, AppState>, game: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("library_update_game"))
}

#[tauri::command]
pub async fn library_remove_game(state: State<'_, AppState>, id: i64) -> Result<Value, AppError> {
    Err(AppError::not_implemented("library_remove_game"))
}

#[tauri::command]
pub async fn library_get_settings(state: State<'_, AppState>, game_id: i64) -> Result<Value, AppError> {
    Err(AppError::not_implemented("library_get_settings"))
}

#[tauri::command]
pub async fn library_save_settings(state: State<'_, AppState>, game_id: i64, settings: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("library_save_settings"))
}

#[tauri::command]
pub async fn library_play_sessions(state: State<'_, AppState>, game_id: i64) -> Result<Value, AppError> {
    Err(AppError::not_implemented("library_play_sessions"))
}

#[tauri::command]
pub async fn settings_get_app(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("settings_get_app"))
}

#[tauri::command]
pub async fn settings_save_app(state: State<'_, AppState>, settings: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("settings_save_app"))
}

#[tauri::command]
pub async fn vndb_search(state: State<'_, AppState>, query: String) -> Result<Value, AppError> {
    Err(AppError::not_implemented("vndb_search"))
}

#[tauri::command]
pub async fn vndb_apply(state: State<'_, AppState>, game_id: i64, vndb_id: String) -> Result<Value, AppError> {
    Err(AppError::not_implemented("vndb_apply"))
}

#[tauri::command]
pub async fn saves_detect(state: State<'_, AppState>, game_id: i64) -> Result<Value, AppError> {
    Err(AppError::not_implemented("saves_detect"))
}

#[tauri::command]
pub async fn saves_set_locations(state: State<'_, AppState>, game_id: i64, paths: Vec<String>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("saves_set_locations"))
}

#[tauri::command]
pub async fn saves_list_backups(state: State<'_, AppState>, game_id: i64) -> Result<Value, AppError> {
    Err(AppError::not_implemented("saves_list_backups"))
}

#[tauri::command]
pub async fn saves_create_backup(state: State<'_, AppState>, game_id: i64, label: Option<String>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("saves_create_backup"))
}

#[tauri::command]
pub async fn saves_restore_backup(state: State<'_, AppState>, backup_id: i64, confirm: bool) -> Result<Value, AppError> {
    Err(AppError::not_implemented("saves_restore_backup"))
}
