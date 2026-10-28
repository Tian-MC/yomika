//! Runtime subsystem: runner discovery, prefixes, command building and
//! process sessions. Owned by Agent 1.
//!
//! CONTRACT (used by src-tauri/src/commands/session.rs — keep signatures stable,
//! you may add methods/fields freely):
//! - `RuntimeService::new`
//! - `list_runners`, `resolve_runner`
//! - `list_prefixes`, `resolve_prefix`
//! - `build_game_command`, `build_prefix_command`
//! - `start_session`, `stop_session`, `start_aux`, `sessions`, `session`

pub mod command;
pub mod prefixes;
pub mod process;
pub mod runners;
mod session;
mod tools;
pub mod util;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use vnl_core::{
    AppError, AppPaths, AuxProgram, CommandSpec, ErrorCode, EventSink, Game, GameId, LogEntry, LogLevel, PrefixArch,
    PrefixChoice, PrefixInfo, PrefixKind, Runner, RunnerKind, RunnerRef, RuntimeConfig, SessionInfo,
};

pub use command::{HostContext, ProtonVerb};
pub use runners::{CustomRunner, DiscoveryRoots, NATIVE_RUNNER_ID, SYSTEM_WINE_ID};

use crate::prefixes::PrefixRegistry;
use crate::session::{SessionManager, SessionSetup};

/// Everything needed to start a game session. Built by the integration layer.
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub game_id: GameId,
    pub runner: Runner,
    pub prefix: PrefixInfo,
    pub config: RuntimeConfig,
    /// Final game command (possibly already wrapped by gamescope).
    pub command: CommandSpec,
    /// Native helper processes started after the game (e.g. upscaler).
    /// Stopped when the session ends.
    pub companions: Vec<CommandSpec>,
    /// Auxiliary programs with their pre-built commands.
    pub aux: Vec<(AuxProgram, CommandSpec)>,
}

// ------------------------------------------------------- command requests

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePrefixRequest {
    pub name: String,
    #[serde(default)]
    pub arch: PrefixArch,
    pub runner_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPrefixRequest {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrefixTool {
    Winecfg,
    Regedit,
    Explorer,
    Winetricks,
    KillAll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixToolRequest {
    pub prefix_id: String,
    pub runner_id: String,
    pub tool: PrefixTool,
    /// winetricks verbs, e.g. `["cjkfonts"]`.
    #[serde(default)]
    pub verbs: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallerRequest {
    pub installer_path: PathBuf,
    pub runner_id: String,
    pub prefix: PrefixChoice,
    /// Used to name a new dedicated prefix.
    pub name_hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallerRun {
    /// Progress is reported via `vnl://task` events with this id.
    pub task_id: String,
    pub prefix: PrefixInfo,
    /// `drive_c` of the prefix.
    pub drive_c: PathBuf,
}

// ------------------------------------------------------------- service

/// Construction options; [`RuntimeOptions::detect`] inspects the real system.
#[derive(Debug, Clone)]
pub struct RuntimeOptions {
    pub roots: DiscoveryRoots,
    pub host: HostContext,
    /// Spawn through `flatpak-spawn --host` (auto-detected via `/.flatpak-info`).
    pub flatpak: bool,
    /// SIGTERM -> SIGKILL grace period.
    pub stop_grace: Duration,
}

impl RuntimeOptions {
    pub fn detect(paths: &AppPaths) -> Self {
        Self {
            roots: DiscoveryRoots::from_system(paths),
            host: HostContext::detect(paths),
            flatpak: command::running_in_flatpak(),
            stop_grace: Duration::from_secs(5),
        }
    }
}

pub struct RuntimeService {
    paths: AppPaths,
    events: Arc<dyn EventSink>,
    roots: DiscoveryRoots,
    host: HostContext,
    flatpak: bool,
    runners: RwLock<Option<Vec<Runner>>>,
    custom_lock: Mutex<()>,
    prefixes: PrefixRegistry,
    sessions: SessionManager,
}

impl RuntimeService {
    pub fn new(paths: &AppPaths, events: Arc<dyn EventSink>) -> Result<Self, AppError> {
        Self::with_options(paths, events, RuntimeOptions::detect(paths))
    }

    pub fn with_options(paths: &AppPaths, events: Arc<dyn EventSink>, options: RuntimeOptions) -> Result<Self, AppError> {
        let (prefixes, warning) = PrefixRegistry::load(&paths.data_dir.join("prefixes.json"))?;
        if let Some(w) = warning {
            events.log(LogEntry::new(LogLevel::Warn, "runtime", w));
        }
        let sessions = SessionManager::new(events.clone(), paths.logs_dir(), options.flatpak, options.stop_grace);
        Ok(Self {
            paths: paths.clone(),
            events,
            roots: options.roots,
            host: options.host,
            flatpak: options.flatpak,
            runners: RwLock::new(None),
            custom_lock: Mutex::new(()),
            prefixes,
            sessions,
        })
    }

    fn warn(&self, message: impl Into<String>) {
        self.events.log(LogEntry::new(LogLevel::Warn, "runtime", message));
    }

    // ------------------------------------------------------------ runners

    /// Cached runner list (discovered on first use).
    pub fn list_runners(&self) -> Vec<Runner> {
        if let Some(list) = self.runners.read().unwrap().as_ref() {
            return list.clone();
        }
        self.refresh_runners()
    }

    /// Re-scan all runner locations.
    pub fn refresh_runners(&self) -> Vec<Runner> {
        let list = runners::discover(&self.roots);
        *self.runners.write().unwrap() = Some(list.clone());
        list
    }

    pub fn find_runner(&self, id: &str) -> Result<Runner, AppError> {
        self.list_runners()
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| AppError::not_found(format!("runner {id} not found")))
    }

    /// Add a user runner (Proton dir, Wine build dir or wine binary); persisted in `config_dir/runners.json`.
    pub fn add_custom_runner(&self, path: &Path, name: Option<&str>) -> Result<Runner, AppError> {
        let file = self.roots.custom_file.clone().unwrap_or_else(|| runners::custom_runners_file(&self.paths));
        let entry = {
            let _guard = self.custom_lock.lock().unwrap();
            let mut existing = runners::load_custom_runners(&file)?;
            let canonical = prefixes::canonical(path);
            if let Some(found) = existing.iter().find(|r| {
                prefixes::canonical(&r.path) == canonical || prefixes::canonical(&r.path) == canonical.join("bin/wine")
            }) {
                found.clone()
            } else {
                let entry = runners::inspect_custom_runner(path, name, &existing)?;
                existing.push(entry.clone());
                runners::save_custom_runners(&file, &existing)?;
                entry
            }
        };
        let list = self.refresh_runners();
        // A custom path that duplicates a discovered runner is listed under the discovered id.
        list.iter()
            .find(|r| r.id == entry.id)
            .or_else(|| list.iter().find(|r| prefixes::canonical(&r.path) == prefixes::canonical(&entry.path)))
            .cloned()
            .ok_or_else(|| AppError::internal("custom runner vanished after saving"))
    }

    /// Pick `preferred`, else `fallback`, else the best available runner.
    pub fn resolve_runner(&self, preferred: Option<&RunnerRef>, fallback: Option<&RunnerRef>) -> Result<Runner, AppError> {
        let list = self.list_runners();
        for wanted in [preferred, fallback].into_iter().flatten() {
            if let Some(r) = list.iter().find(|r| r.id == wanted.id) {
                return Ok(r.clone());
            }
            self.warn(format!("runner {} is not available any more; falling back", wanted.id));
        }
        runners::best_runner(&list).cloned().ok_or_else(|| {
            AppError::new(ErrorCode::MissingDependency, "No Wine or Proton runner found")
                .with_details("Install wine or a Proton build (e.g. GE-Proton in ~/.steam/root/compatibilitytools.d).")
        })
    }

    // ----------------------------------------------------------- prefixes

    pub fn list_prefixes(&self) -> Result<Vec<PrefixInfo>, AppError> {
        Ok(self.prefixes.list())
    }

    pub fn get_prefix(&self, id: &str) -> Result<PrefixInfo, AppError> {
        self.prefixes.get(id).ok_or_else(|| AppError::not_found(format!("prefix {id} not found")))
    }

    /// Resolve (and register, for dedicated prefixes) the prefix for a game.
    /// Must not create/initialise anything destructive; Wine/Proton initialise
    /// an empty prefix on first run.
    pub fn resolve_prefix(&self, game: &Game, choice: &PrefixChoice, runner: &Runner) -> Result<PrefixInfo, AppError> {
        match choice {
            PrefixChoice::Dedicated => {
                if let Some(p) = self.prefixes.find_dedicated(game.id) {
                    return Ok(p);
                }
                let id = format!("game-{}", game.id);
                let path = self.paths.prefixes_dir().join(&id);
                let arch = prefixes::detect_arch(&path.join("pfx")).unwrap_or_default();
                self.prefixes.upsert(PrefixInfo {
                    id,
                    name: game.title.clone(),
                    path,
                    arch,
                    kind: PrefixKind::Dedicated { game_id: game.id },
                    runner_id: (runner.kind != RunnerKind::Native).then(|| runner.id.clone()),
                    created_at: vnl_core::now_secs(),
                    managed: true,
                    initialized: false,
                })
            }
            PrefixChoice::Shared { id } => self.get_prefix(id),
            PrefixChoice::Custom { path } => {
                if let Some(p) = self.prefixes.find_by_path(path) {
                    return Ok(p);
                }
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "prefix".into());
                self.register_prefix(RegisterPrefixRequest { name, path: path.clone() })
            }
        }
    }

    /// Register an existing directory as an External prefix. Never modifies it.
    pub fn register_prefix(&self, request: RegisterPrefixRequest) -> Result<PrefixInfo, AppError> {
        if !request.path.is_dir() {
            return Err(AppError::not_found(format!("prefix directory {} does not exist", request.path.display())));
        }
        let path = prefixes::canonical(&request.path);
        if let Some(existing) = self.prefixes.find_by_path(&path) {
            return Ok(existing);
        }
        let name = if request.name.trim().is_empty() {
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            request.name.trim().to_string()
        };
        let mut info = PrefixInfo {
            id: format!("ext-{}", util::short_id()),
            name,
            path,
            arch: PrefixArch::Win64,
            kind: PrefixKind::External,
            runner_id: None,
            created_at: vnl_core::now_secs(),
            managed: false,
            initialized: false,
        };
        info.arch = prefixes::detect_arch(&prefixes::wine_prefix_dir(&info)).unwrap_or_default();
        self.prefixes.upsert(info)
    }

    /// Create the managed directories a prefix needs before the first spawn
    /// (`<path>/pfx` for managed prefixes, the dummy Steam client dir).
    /// Never touches unmanaged prefixes.
    pub fn prepare_prefix(&self, runner: &Runner, prefix: &PrefixInfo) -> Result<(), AppError> {
        if prefix.managed {
            std::fs::create_dir_all(prefix.path.join("pfx"))?;
        }
        if runner.kind == RunnerKind::Proton && self.host.steam_root.is_none() {
            std::fs::create_dir_all(&self.host.fallback_steam_dir)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ commands

    pub fn build_game_command(
        &self,
        game: &Game,
        config: &RuntimeConfig,
        runner: &Runner,
        prefix: &PrefixInfo,
    ) -> Result<CommandSpec, AppError> {
        let built = command::build_game(&self.host, game, config, runner, prefix)?;
        for w in built.warnings {
            self.warn(w);
        }
        Ok(built.spec)
    }

    /// Command running another Windows exe in the same prefix/runner as the
    /// game, safe to start while the game is running (Proton: runinprefix).
    pub fn build_prefix_command(
        &self,
        exe: &Path,
        args: &[String],
        cwd: Option<&Path>,
        config: &RuntimeConfig,
        runner: &Runner,
        prefix: &PrefixInfo,
    ) -> Result<CommandSpec, AppError> {
        let built = command::build_in_prefix(&self.host, exe, args, cwd, config, runner, prefix)?;
        for w in built.warnings {
            self.warn(w);
        }
        Ok(built.spec)
    }

    // ------------------------------------------------------------ sessions

    pub async fn start_session(&self, request: LaunchRequest) -> Result<SessionInfo, AppError> {
        self.prepare_prefix(&request.runner, &request.prefix)?;
        let (wineserver_wait, wineserver_kill, wineprefix) = if request.runner.kind == RunnerKind::Native {
            (None, None, None)
        } else {
            (
                command::wineserver_command(&request.runner, &request.prefix, "-w"),
                command::wineserver_command(&request.runner, &request.prefix, "-k"),
                Some(prefixes::wine_prefix_dir(&request.prefix)),
            )
        };
        self.sessions.start(SessionSetup { request, wineserver_wait, wineserver_kill, wineprefix })
    }

    pub async fn stop_session(&self, session_id: &str, force: bool) -> Result<(), AppError> {
        self.sessions.stop(session_id, force).await
    }

    pub async fn start_aux(&self, session_id: &str, aux_id: &str) -> Result<SessionInfo, AppError> {
        self.sessions.start_aux(session_id, aux_id)
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        self.sessions.list()
    }

    pub fn session(&self, session_id: &str) -> Option<SessionInfo> {
        self.sessions.get(session_id).map(|s| s.snapshot())
    }

    /// Per-session log file in `logs_dir`.
    pub fn session_log_path(&self, session_id: &str) -> Option<PathBuf> {
        self.sessions.log_path(session_id)
    }

    /// Wait until a session is Exited/Failed or `timeout` elapses; returns its latest info.
    pub async fn wait_session(&self, session_id: &str, timeout: Duration) -> Option<SessionInfo> {
        self.sessions.wait_finished(session_id, timeout).await
    }
}

#[cfg(test)]
mod tests;
