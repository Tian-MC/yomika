//! Prefix creation, prefix tools (winecfg, regedit, explorer, winetricks,
//! kill all) and installers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use vnl_core::{
    AppError, AppEvent, CommandSpec, ErrorCode, LogLevel, PrefixArch, PrefixChoice, PrefixInfo, PrefixKind,
    ProtonLauncher, Runner, RunnerKind, RuntimeConfig, TaskProgress, TaskState,
};

use crate::command::{self, Invocation, ProtonVerb};
use crate::process::{self, OutputTarget};
use crate::{
    CreatePrefixRequest, InstallerRequest, InstallerRun, PrefixTool, PrefixToolRequest, RuntimeService, prefixes,
    runners,
};

const INIT_TIMEOUT: Duration = Duration::from_secs(600);

/// Config for helper invocations: direct Proton (no umu), no forced locale.
fn tool_config(japanese: bool) -> RuntimeConfig {
    let mut c = RuntimeConfig::default();
    c.proton.launcher = ProtonLauncher::Direct;
    c.locale.japanese_locale = japanese;
    c
}

fn require_wine_runner(runner: &Runner) -> Result<(), AppError> {
    if runner.kind == RunnerKind::Native {
        return Err(AppError::invalid("The native runner cannot be used with a Wine prefix"));
    }
    Ok(())
}

impl RuntimeService {
    fn target(&self) -> OutputTarget {
        OutputTarget { sink: self.events.clone(), session_id: None, file: None }
    }

    fn task(&self, task_id: &str, label: &str, state: TaskState, message: Option<String>) {
        self.events.emit(AppEvent::Task(TaskProgress {
            task_id: task_id.to_string(),
            label: label.to_string(),
            state,
            progress: None,
            message,
        }));
    }

    /// Build a command for a helper program (wineboot, winecfg, installers).
    fn helper_command(
        &self,
        exe: &Path,
        args: &[String],
        runner: &Runner,
        prefix: &PrefixInfo,
        verb: ProtonVerb,
        japanese: bool,
    ) -> Result<CommandSpec, AppError> {
        let cwd = exe.parent().filter(|p| p.is_absolute()).map(Path::to_path_buf);
        let inv = Invocation { exe, args, cwd, verb, install_dir: None };
        let built = command::build(&self.host, &inv, &tool_config(japanese), runner, prefix)?;
        for w in built.warnings {
            self.warn(w);
        }
        Ok(built.spec)
    }

    /// Initialise a prefix with `wineboot -u` and wait for its wineserver.
    async fn init_prefix(&self, runner: &Runner, prefix: &PrefixInfo) -> Result<(), AppError> {
        self.prepare_prefix(runner, prefix)?;
        let spec =
            self.helper_command(Path::new("wineboot"), &["-u".into()], runner, prefix, ProtonVerb::Run, false)?;
        let target = self.target();
        let status = process::run_to_completion(&spec, self.flatpak, "prefix.init", &target, INIT_TIMEOUT).await?;
        if let Some(ws) = command::wineserver_command(runner, prefix, "-w") {
            let _ = process::run_to_completion(&ws, self.flatpak, "prefix.init", &target, INIT_TIMEOUT).await;
        }
        if !prefixes::is_initialized(prefix) {
            return Err(AppError::new(ErrorCode::Process, "prefix initialisation failed (no drive_c)")
                .with_details(format!("wineboot exited with {status}; command: {}", spec.preview())));
        }
        Ok(())
    }

    /// Create and initialise a new shared, managed prefix. Refuses to use a
    /// non-empty directory.
    pub async fn create_prefix(&self, request: CreatePrefixRequest) -> Result<PrefixInfo, AppError> {
        let name = request.name.trim();
        if name.is_empty() {
            return Err(AppError::invalid("Prefix name must not be empty"));
        }
        let runner = self.find_runner(&request.runner_id)?;
        require_wine_runner(&runner)?;
        if runner.kind == RunnerKind::Proton && request.arch == PrefixArch::Win32 {
            return Err(AppError::new(ErrorCode::Unsupported, "Proton prefixes are always 64-bit (WoW64)")
                .with_details("Use a Wine runner to create a 32-bit prefix."));
        }
        let id = self.prefixes.unique_id(name, &self.paths.prefixes_dir());
        let path = self.paths.prefixes_dir().join(&id);
        self.create_prefix_at(&path, &id, name, request.arch, &runner).await
    }

    /// Create a prefix at an explicit path (must be missing or empty).
    pub(crate) async fn create_prefix_at(
        &self,
        path: &Path,
        id: &str,
        name: &str,
        arch: PrefixArch,
        runner: &Runner,
    ) -> Result<PrefixInfo, AppError> {
        if !prefixes::is_missing_or_empty(path)? {
            return Err(AppError::invalid(format!(
                "Refusing to create a prefix in non-empty directory {}",
                path.display()
            ))
            .with_details("Use \"register prefix\" to add an existing prefix."));
        }
        std::fs::create_dir_all(path.join("pfx"))?;
        let info = self.prefixes.upsert(PrefixInfo {
            id: id.to_string(),
            name: name.to_string(),
            path: path.to_path_buf(),
            arch,
            kind: PrefixKind::Shared,
            runner_id: Some(runner.id.clone()),
            created_at: vnl_core::now_secs(),
            managed: true,
            initialized: false,
        })?;
        let task_id = format!("prefix-{}", util_id());
        let label = format!("Creating prefix {name}");
        self.task(&task_id, &label, TaskState::Running, None);
        match self.init_prefix(runner, &info).await {
            Ok(()) => {
                self.task(&task_id, &label, TaskState::Done, None);
                self.get_prefix(id)
            }
            Err(e) => {
                self.task(&task_id, &label, TaskState::Failed, Some(e.message.clone()));
                Err(e)
            }
        }
    }

    /// Run winecfg / regedit / explorer / winetricks (detached) or kill all
    /// processes of the prefix (waits for `wineserver -k`).
    pub async fn run_prefix_tool(&self, request: PrefixToolRequest) -> Result<(), AppError> {
        let prefix = self.get_prefix(&request.prefix_id)?;
        let runner = self.find_runner(&request.runner_id)?;
        require_wine_runner(&runner)?;
        let target = self.target();
        let tool_name = format!("{:?}", request.tool).to_lowercase();
        let source = format!("tool.{tool_name}");

        let spec = match request.tool {
            PrefixTool::KillAll => {
                let spec = command::wineserver_command(&runner, &prefix, "-k").ok_or_else(|| {
                    AppError::new(ErrorCode::MissingDependency, format!("no wineserver found for {}", runner.name))
                })?;
                process::run_to_completion(&spec, self.flatpak, &source, &target, Duration::from_secs(30)).await?;
                return Ok(());
            }
            PrefixTool::Winecfg | PrefixTool::Regedit | PrefixTool::Explorer => {
                self.prepare_prefix(&runner, &prefix)?;
                let exe = PathBuf::from(&tool_name);
                // runinprefix skips Proton's prefix setup; use `run` for a fresh prefix.
                let verb = if prefix.initialized { ProtonVerb::RunInPrefix } else { ProtonVerb::Run };
                self.helper_command(&exe, &[], &runner, &prefix, verb, false)?
            }
            PrefixTool::Winetricks => {
                self.prepare_prefix(&runner, &prefix)?;
                winetricks_command(&self.host, &runner, &prefix, request.verbs.as_deref().unwrap_or(&[]))?
            }
        };

        let mut child = process::spawn_group(&spec, self.flatpak)?;
        let pumps = process::capture_single(&mut child, &source, &target);
        target.log(LogLevel::Info, &source, format!("started: {}", spec.preview()));
        let is_tricks = request.tool == PrefixTool::Winetricks;
        let task_id = format!("tool-{}", util_id());
        let label = format!("winetricks {}", request.verbs.clone().unwrap_or_default().join(" "));
        if is_tricks {
            self.task(&task_id, &label, TaskState::Running, None);
        }
        let events = self.events.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            for p in pumps {
                let _ = tokio::time::timeout(Duration::from_secs(1), p).await;
            }
            let ok = status.as_ref().is_ok_and(|s| s.success());
            let msg = match &status {
                Ok(s) => format!("exited with {s}"),
                Err(e) => format!("wait failed: {e}"),
            };
            target.log(if ok { LogLevel::Info } else { LogLevel::Warn }, &source, msg.clone());
            if is_tricks {
                events.emit(AppEvent::Task(TaskProgress {
                    task_id,
                    label,
                    state: if ok { TaskState::Done } else { TaskState::Failed },
                    progress: None,
                    message: Some(msg),
                }));
            }
        });
        Ok(())
    }

    /// Run an installer in the chosen prefix as a background task.
    pub async fn run_installer(self: &Arc<Self>, request: InstallerRequest) -> Result<InstallerRun, AppError> {
        if !request.installer_path.is_file() {
            return Err(AppError::not_found(format!("installer {} not found", request.installer_path.display())));
        }
        let runner = self.find_runner(&request.runner_id)?;
        require_wine_runner(&runner)?;
        let prefix = match &request.prefix {
            PrefixChoice::Dedicated => {
                let name = if request.name_hint.trim().is_empty() {
                    request
                        .installer_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "installer".into())
                } else {
                    request.name_hint.trim().to_string()
                };
                let id = self.prefixes.unique_id(&name, &self.paths.prefixes_dir());
                let path = self.paths.prefixes_dir().join(&id);
                std::fs::create_dir_all(path.join("pfx"))?;
                self.prefixes.upsert(PrefixInfo {
                    id,
                    name,
                    path,
                    arch: PrefixArch::Win64,
                    kind: PrefixKind::Shared,
                    runner_id: Some(runner.id.clone()),
                    created_at: vnl_core::now_secs(),
                    managed: true,
                    initialized: false,
                })?
            }
            PrefixChoice::Shared { id } => self.get_prefix(id)?,
            PrefixChoice::Custom { path } => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                match self.prefixes.find_by_path(path) {
                    Some(p) => p,
                    None => self.register_prefix(crate::RegisterPrefixRequest { name, path: path.clone() })?,
                }
            }
        };
        self.prepare_prefix(&runner, &prefix)?;
        let spec = self.helper_command(&request.installer_path, &[], &runner, &prefix, ProtonVerb::Run, true)?;
        let wait = command::wineserver_command(&runner, &prefix, "-w");
        let task_id = format!("installer-{}", util_id());
        let label = format!(
            "Installing {}",
            request.installer_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        );
        let run = InstallerRun { task_id: task_id.clone(), drive_c: prefixes::drive_c(&prefix), prefix };

        let service = self.clone();
        self.task(&task_id, &label, TaskState::Running, Some(spec.preview()));
        tokio::spawn(async move {
            let target = service.target();
            // Installers can run for a long time (and may launch the game at the end).
            let long = Duration::from_secs(6 * 3600);
            let result = process::run_to_completion(&spec, service.flatpak, "installer", &target, long).await;
            if let Some(ws) = wait {
                let _ = process::run_to_completion(&ws, service.flatpak, "installer", &target, long).await;
            }
            match result {
                Ok(status) if status.success() => service.task(&task_id, &label, TaskState::Done, None),
                Ok(status) => {
                    service.task(&task_id, &label, TaskState::Failed, Some(format!("installer exited with {status}")))
                }
                Err(e) => service.task(&task_id, &label, TaskState::Failed, Some(e.message)),
            }
        });
        Ok(run)
    }
}

fn util_id() -> String {
    crate::util::short_id()
}

/// `winetricks` pointed at the runner's own wine/wineserver binaries.
pub(crate) fn winetricks_command(
    host: &command::HostContext,
    runner: &Runner,
    prefix: &PrefixInfo,
    verbs: &[String],
) -> Result<CommandSpec, AppError> {
    let winetricks = host.winetricks.clone().ok_or_else(|| {
        AppError::new(ErrorCode::MissingDependency, "winetricks is not installed")
            .with_details("Install winetricks from your distribution's packages.")
    })?;
    for v in verbs {
        if v.is_empty() || v.starts_with('-') || v.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(AppError::invalid(format!("invalid winetricks verb {v:?}")));
        }
    }
    let wine = runners::wine_binary(runner)
        .ok_or_else(|| AppError::new(ErrorCode::MissingDependency, format!("{} has no wine binary", runner.name)))?;
    let mut spec = CommandSpec::new(winetricks);
    if !verbs.is_empty() {
        spec = spec.arg("--unattended").args(verbs.iter().cloned());
    }
    spec = spec
        .env("WINEPREFIX", prefixes::wine_prefix_dir(prefix).to_string_lossy())
        .env("WINE", wine.to_string_lossy())
        .env("WINEDEBUG", "-all");
    if let Some(ws) = runners::wineserver_binary(runner) {
        spec = spec.env("WINESERVER", ws.to_string_lossy());
    }
    Ok(spec)
}
