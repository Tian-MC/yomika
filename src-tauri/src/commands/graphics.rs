//! Tauri commands for the Agent 5 (graphics) area. Owned by Agent 5 (graphics).
//! Stubs: replace `serde_json::Value` with real types from the subsystem crate,
//! keeping the command names and argument names from src/contract/*.ts.

#![allow(unused_variables)]

use serde_json::Value;
use tauri::State;
use vnl_core::AppError;

use crate::state::AppState;

#[tauri::command]
pub async fn graphics_capabilities(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("graphics_capabilities"))
}

#[tauri::command]
pub async fn graphics_preview(state: State<'_, AppState>, config: Value, runner_id: Option<String>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("graphics_preview"))
}

#[tauri::command]
pub async fn graphics_upscaler_info(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("graphics_upscaler_info"))
}

#[tauri::command]
pub async fn diagnostics_dependencies(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("diagnostics_dependencies"))
}

#[tauri::command]
pub async fn diagnostics_system(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("diagnostics_system"))
}

#[tauri::command]
pub async fn diagnostics_prefix_components(state: State<'_, AppState>, prefix_path: String) -> Result<Value, AppError> {
    Err(AppError::not_implemented("diagnostics_prefix_components"))
}
