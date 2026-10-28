//! Tauri commands for the Agent 1 (runtime) area. Owned by Agent 1 (runtime).
//! Stubs: replace `serde_json::Value` with real types from the subsystem crate,
//! keeping the command names and argument names from src/contract/*.ts.

#![allow(unused_variables)]

use serde_json::Value;
use tauri::State;
use vnl_core::AppError;

use crate::state::AppState;

#[tauri::command]
pub async fn runtime_list_runners(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("runtime_list_runners"))
}

#[tauri::command]
pub async fn runtime_refresh_runners(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("runtime_refresh_runners"))
}

#[tauri::command]
pub async fn runtime_add_custom_runner(state: State<'_, AppState>, path: String, name: Option<String>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("runtime_add_custom_runner"))
}

#[tauri::command]
pub async fn runtime_list_prefixes(state: State<'_, AppState>) -> Result<Value, AppError> {
    Err(AppError::not_implemented("runtime_list_prefixes"))
}

#[tauri::command]
pub async fn runtime_create_prefix(state: State<'_, AppState>, request: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("runtime_create_prefix"))
}

#[tauri::command]
pub async fn runtime_register_prefix(state: State<'_, AppState>, request: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("runtime_register_prefix"))
}

#[tauri::command]
pub async fn runtime_prefix_tool(state: State<'_, AppState>, request: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("runtime_prefix_tool"))
}

#[tauri::command]
pub async fn runtime_run_installer(state: State<'_, AppState>, request: Value) -> Result<Value, AppError> {
    Err(AppError::not_implemented("runtime_run_installer"))
}
