//! Tauri commands for the library area (games, settings, playtime, VNDB,
//! saves). Owned by Agent 2 (library). Command and argument names mirror
//! `src/contract/library.ts`.
//!
//! DB calls are short and run inline; filesystem scans, backups and restores
//! run on the blocking pool; VNDB calls are async.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;
use vnl_core::{AppError, AppSettings, Game, GameId, GameQuery, GameSettings, PrefixChoice, PrefixKind};
use vnl_library::{
    ImportRequest, ImportScan, Library, PlaySession, RestoreResult, SaveBackup, SaveLocation, VndbSearchResult,
};

use crate::state::AppState;

/// Run blocking library work (filesystem scans, archives) off the async runtime.
async fn blocking<T, F>(library: &Arc<Library>, f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&Library) -> Result<T, AppError> + Send + 'static,
{
    let library = library.clone();
    tauri::async_runtime::spawn_blocking(move || f(&library))
        .await
        .map_err(|e| AppError::internal("background task failed").with_details(e.to_string()))?
}

/// `drive_c` of the game's prefix, if it can be determined without side
/// effects. Uses `list_prefixes` (read-only) instead of `resolve_prefix`,
/// which may create a dedicated prefix. Any runtime error (including
/// NotImplemented) simply disables prefix-based save detection.
fn prefix_drive_c(state: &AppState, game_id: GameId) -> Option<PathBuf> {
    let settings = state.library.get_settings(game_id).ok()?;
    let prefix_path = match &settings.runtime.prefix {
        PrefixChoice::Custom { path } => path.clone(),
        choice => {
            let prefixes = match state.runtime.list_prefixes() {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!(game_id, error = %e, "prefix list unavailable; skipping prefix save detection");
                    return None;
                }
            };
            prefixes
                .into_iter()
                .find(|p| match choice {
                    PrefixChoice::Shared { id } => &p.id == id,
                    PrefixChoice::Dedicated => p.kind == PrefixKind::Dedicated { game_id },
                    PrefixChoice::Custom { .. } => false,
                })?
                .path
        }
    };
    // Wine: <prefix>/drive_c; Proton compat data: <prefix>/pfx/drive_c.
    [prefix_path.join("drive_c"), prefix_path.join("pfx").join("drive_c")].into_iter().find(|p| p.is_dir())
}

#[tauri::command]
pub async fn library_list_games(state: State<'_, AppState>, query: GameQuery) -> Result<Vec<Game>, AppError> {
    state.library.list_games(&query)
}

#[tauri::command]
pub async fn library_get_game(state: State<'_, AppState>, id: GameId) -> Result<Game, AppError> {
    state.library.get_game(id)
}

#[tauri::command]
pub async fn library_scan_path(state: State<'_, AppState>, path: String) -> Result<ImportScan, AppError> {
    blocking(&state.library, move |lib| lib.scan_path(std::path::Path::new(&path))).await
}

#[tauri::command]
pub async fn library_import_game(state: State<'_, AppState>, request: ImportRequest) -> Result<Game, AppError> {
    state.library.import_game(request)
}

#[tauri::command]
pub async fn library_update_game(state: State<'_, AppState>, game: Game) -> Result<Game, AppError> {
    state.library.update_game(&game)
}

#[tauri::command]
pub async fn library_remove_game(state: State<'_, AppState>, id: GameId) -> Result<(), AppError> {
    state.library.remove_game(id)
}

#[tauri::command]
pub async fn library_get_settings(state: State<'_, AppState>, game_id: GameId) -> Result<GameSettings, AppError> {
    state.library.get_settings(game_id)
}

#[tauri::command]
pub async fn library_save_settings(
    state: State<'_, AppState>,
    game_id: GameId,
    settings: GameSettings,
) -> Result<GameSettings, AppError> {
    state.library.save_settings(game_id, &settings)
}

#[tauri::command]
pub async fn library_play_sessions(state: State<'_, AppState>, game_id: GameId) -> Result<Vec<PlaySession>, AppError> {
    state.library.play_sessions(game_id)
}

#[tauri::command]
pub async fn settings_get_app(state: State<'_, AppState>) -> Result<AppSettings, AppError> {
    state.library.get_app_settings()
}

#[tauri::command]
pub async fn settings_save_app(state: State<'_, AppState>, settings: AppSettings) -> Result<AppSettings, AppError> {
    state.library.save_app_settings(&settings)
}

#[tauri::command]
pub async fn vndb_search(state: State<'_, AppState>, query: String) -> Result<Vec<VndbSearchResult>, AppError> {
    state.library.vndb_search(&query).await
}

#[tauri::command]
pub async fn vndb_apply(state: State<'_, AppState>, game_id: GameId, vndb_id: String) -> Result<Game, AppError> {
    state.library.vndb_apply(game_id, &vndb_id).await
}

#[tauri::command]
pub async fn saves_detect(state: State<'_, AppState>, game_id: GameId) -> Result<Vec<SaveLocation>, AppError> {
    let drive_c = prefix_drive_c(&state, game_id);
    blocking(&state.library, move |lib| lib.detect_saves(game_id, drive_c.as_deref())).await
}

/// Replace the manual save locations, then return manual + detected locations.
#[tauri::command]
pub async fn saves_set_locations(
    state: State<'_, AppState>,
    game_id: GameId,
    paths: Vec<String>,
) -> Result<Vec<SaveLocation>, AppError> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let drive_c = prefix_drive_c(&state, game_id);
    blocking(&state.library, move |lib| {
        lib.set_save_locations(game_id, &paths)?;
        lib.detect_saves(game_id, drive_c.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn saves_list_backups(state: State<'_, AppState>, game_id: GameId) -> Result<Vec<SaveBackup>, AppError> {
    state.library.list_backups(game_id)
}

#[tauri::command]
pub async fn saves_create_backup(
    state: State<'_, AppState>,
    game_id: GameId,
    label: Option<String>,
) -> Result<SaveBackup, AppError> {
    let drive_c = prefix_drive_c(&state, game_id);
    blocking(&state.library, move |lib| lib.create_backup(game_id, label, drive_c.as_deref())).await
}

#[tauri::command]
pub async fn saves_restore_backup(
    state: State<'_, AppState>,
    backup_id: i64,
    confirm: bool,
) -> Result<RestoreResult, AppError> {
    blocking(&state.library, move |lib| lib.restore_backup(backup_id, confirm)).await
}
