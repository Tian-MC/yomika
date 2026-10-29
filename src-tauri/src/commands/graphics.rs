//! Tauri commands for the graphics / diagnostics area. Owned by Agent 5.
//! Names and argument names mirror `src/contract/graphics.ts`.
//!
//! Detection runs helper programs (`--version`, `lspci`, `fc-list`, ...) with
//! timeouts, so every command runs on the blocking thread pool.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;
use vnl_core::{AppError, GraphicsConfig};
use vnl_graphics::{
    DependencyStatus, GraphicsCapabilities, GraphicsPreview, GraphicsService, PrefixComponent, SystemReport,
    UpscalerInfo,
};

use crate::state::AppState;

async fn blocking<T, F>(svc: &Arc<GraphicsService>, f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&GraphicsService) -> Result<T, AppError> + Send + 'static,
{
    let svc = svc.clone();
    tauri::async_runtime::spawn_blocking(move || f(&svc))
        .await
        .map_err(|e| AppError::internal(format!("graphics task failed: {e}")))?
}

#[tauri::command]
pub async fn graphics_capabilities(state: State<'_, AppState>) -> Result<GraphicsCapabilities, AppError> {
    blocking(&state.graphics, |g| Ok(g.capabilities())).await
}

#[tauri::command]
pub async fn graphics_preview(
    state: State<'_, AppState>,
    config: GraphicsConfig,
    runner_id: Option<String>,
) -> Result<GraphicsPreview, AppError> {
    let runner = match runner_id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => Some(
            state
                .runtime
                .list_runners()
                .into_iter()
                .find(|r| r.id == id)
                .ok_or_else(|| AppError::not_found(format!("Runner '{id}' not found")))?,
        ),
        None => None,
    };
    let options = vnl_graphics::GraphicsOptions {
        gamescope_path: state.library.get_app_settings().ok().and_then(|s| s.gamescope_path),
    };
    blocking(&state.graphics, move |g| g.preview_with(&config, runner.as_ref(), &options)).await
}

#[tauri::command]
pub async fn graphics_upscaler_info(state: State<'_, AppState>) -> Result<UpscalerInfo, AppError> {
    blocking(&state.graphics, |g| Ok(g.upscaler_info())).await
}

#[tauri::command]
pub async fn diagnostics_dependencies(state: State<'_, AppState>) -> Result<Vec<DependencyStatus>, AppError> {
    let runners = state.runtime.list_runners();
    blocking(&state.graphics, move |g| Ok(g.dependencies(&runners))).await
}

#[tauri::command]
pub async fn diagnostics_system(state: State<'_, AppState>) -> Result<SystemReport, AppError> {
    blocking(&state.graphics, |g| Ok(g.system_report())).await
}

#[tauri::command]
pub async fn diagnostics_prefix_components(
    state: State<'_, AppState>,
    prefix_path: String,
) -> Result<Vec<PrefixComponent>, AppError> {
    if prefix_path.trim().is_empty() {
        return Err(AppError::invalid("prefixPath must not be empty"));
    }
    let path = PathBuf::from(prefix_path);
    blocking(&state.graphics, move |g| g.prefix_components(&path)).await
}
