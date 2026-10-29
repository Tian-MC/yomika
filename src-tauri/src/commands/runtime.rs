//! Tauri commands for the runtime area (runners, prefixes, prefix tools,
//! installers). Owned by Agent 1 (runtime). Mirrors src/contract/runtime.ts.

use std::path::PathBuf;

use tauri::State;
use vnl_core::{AppError, PrefixInfo, Runner};
use vnl_runtime::{CreatePrefixRequest, InstallerRequest, InstallerRun, PrefixToolRequest, RegisterPrefixRequest};

use crate::state::AppState;

#[tauri::command]
pub async fn runtime_list_runners(state: State<'_, AppState>) -> Result<Vec<Runner>, AppError> {
    let runtime = state.runtime.clone();
    // First call runs `wine --version`; keep it off the async executor.
    tauri::async_runtime::spawn_blocking(move || runtime.list_runners())
        .await
        .map_err(|e| AppError::internal(e.to_string()))
}

#[tauri::command]
pub async fn runtime_refresh_runners(state: State<'_, AppState>) -> Result<Vec<Runner>, AppError> {
    let runtime = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || runtime.refresh_runners())
        .await
        .map_err(|e| AppError::internal(e.to_string()))
}

#[tauri::command]
pub async fn runtime_add_custom_runner(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<Runner, AppError> {
    let runtime = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || runtime.add_custom_runner(&PathBuf::from(path), name.as_deref()))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn runtime_list_prefixes(state: State<'_, AppState>) -> Result<Vec<PrefixInfo>, AppError> {
    state.runtime.list_prefixes()
}

/// Creates and initialises (wineboot) a new managed prefix. May take a minute.
#[tauri::command]
pub async fn runtime_create_prefix(
    state: State<'_, AppState>,
    request: CreatePrefixRequest,
) -> Result<PrefixInfo, AppError> {
    state.runtime.create_prefix(request).await
}

/// Registers an existing prefix. Never modified destructively.
#[tauri::command]
pub async fn runtime_register_prefix(
    state: State<'_, AppState>,
    request: RegisterPrefixRequest,
) -> Result<PrefixInfo, AppError> {
    state.runtime.register_prefix(request)
}

#[tauri::command]
pub async fn runtime_prefix_tool(state: State<'_, AppState>, request: PrefixToolRequest) -> Result<(), AppError> {
    state.runtime.run_prefix_tool(request).await
}

#[tauri::command]
pub async fn runtime_run_installer(
    state: State<'_, AppState>,
    request: InstallerRequest,
) -> Result<InstallerRun, AppError> {
    state.runtime.run_installer(request).await
}
