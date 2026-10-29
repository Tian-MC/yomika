//! Gamescope binaries and the optional guided build of the Anime4K fork.
//! Owned by the orchestrator. Nothing here runs automatically: the build
//! starts only when the UI calls `gamescope_build_start`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;
use vnl_core::AppError;
use vnl_graphics::gamescope_bin::{self, GamescopeBinary, GamescopeSource};
use vnl_graphics::gamescope_build::{self, BuildOptions, BuildStatus, ManagedBuild, Requirements};
use vnl_graphics::probe::Probe;

use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamescopeStatus {
    /// Every usable binary (application custom path, system, managed).
    pub binaries: Vec<GamescopeBinary>,
    /// A usable binary that supports `-F anime4k` exists.
    pub anime4k_available: bool,
    pub managed: ManagedBuild,
    pub requirements: Requirements,
    pub build: BuildStatus,
    pub repo_url: String,
    pub branch: String,
    /// The configured application-wide custom path, if any, even when invalid.
    pub custom_path: Option<PathBuf>,
}

#[tauri::command]
pub async fn gamescope_status(state: State<'_, AppState>) -> Result<GamescopeStatus, AppError> {
    let custom_path = state.library.get_app_settings()?.gamescope_path;
    let paths = state.paths.clone();
    let builder = state.gamescope_builder.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let probe = Probe::system();
        let binaries = gamescope_bin::discover(&probe, &paths, None, custom_path.as_deref());
        GamescopeStatus {
            anime4k_available: binaries.iter().any(|b| b.supports_anime4k),
            binaries,
            managed: builder.managed(),
            requirements: builder.requirements(),
            build: builder.status(),
            repo_url: gamescope_build::REPO_URL.into(),
            branch: gamescope_build::BRANCH.into(),
            custom_path,
        }
    })
    .await
    .map_err(|e| AppError::internal("background task failed").with_details(e.to_string()))
}

/// Check that `path` is a gamescope binary and report what it supports.
#[tauri::command]
pub async fn gamescope_inspect_path(path: String) -> Result<GamescopeBinary, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(&path);
        if !path.is_absolute() {
            return Err(AppError::invalid("Choose the gamescope executable with its full path"));
        }
        gamescope_bin::inspect(&Probe::system(), Path::new(&path), GamescopeSource::Custom).ok_or_else(|| {
            AppError::invalid(format!(
                "{} is not an executable gamescope (`--help` did not look like gamescope)",
                path.display()
            ))
        })
    })
    .await
    .map_err(|e| AppError::internal("background task failed").with_details(e.to_string()))?
}

/// Start the build of the gamescope-anime4k fork. Returns the task id whose
/// progress arrives as `vnl://task` events; output as `gamescope-build` logs.
#[tauri::command]
pub async fn gamescope_build_start(state: State<'_, AppState>) -> Result<String, AppError> {
    let app = state.app.clone();
    let builder = state.gamescope_builder.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let state = app.state::<AppState>();
        if state.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::invalid("The launcher is shutting down"));
        }
        let result = builder.start(BuildOptions::default());
        if state.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            builder.cancel();
        }
        result
    })
    .await
    .map_err(|e| AppError::internal("background task failed").with_details(e.to_string()))?
}

#[tauri::command]
pub async fn gamescope_build_cancel(state: State<'_, AppState>) -> Result<bool, AppError> {
    Ok(state.gamescope_builder.cancel())
}

/// Delete the managed build from the launcher's private directory.
#[tauri::command]
pub async fn gamescope_managed_remove(state: State<'_, AppState>) -> Result<(), AppError> {
    state.gamescope_builder.remove_managed()
}
