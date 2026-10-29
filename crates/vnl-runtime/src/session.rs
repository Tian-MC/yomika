//! Game sessions: spawning, output capture, aux programs, companions,
//! waiting for the wineserver and stopping whole process trees.
//!
//! Every process (game, each aux program, each companion) is started as the
//! leader of its own process group, so the game's group (gamescope -> proton
//! -> wine ...) can be signalled as a whole without touching aux programs,
//! and aux programs can exit without affecting the game.

use std::collections::{HashMap, HashSet};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nix::sys::signal::Signal;
use tokio::sync::watch;
use vnl_core::{
    AppError, AppEvent, AuxProcessInfo, AuxProgram, CommandSpec, EventSink, LogLevel, SessionInfo, SessionState,
};

use crate::LaunchRequest;
use crate::process::{self, OutputTarget};

/// Everything `SessionManager::start` needs beyond the launch request.
pub(crate) struct SessionSetup {
    pub request: LaunchRequest,
    /// `wineserver -w` for the prefix (used by `wait_for_wineserver`).
    pub wineserver_wait: Option<CommandSpec>,
    /// `wineserver -k` for the prefix (used by forced stops).
    pub wineserver_kill: Option<CommandSpec>,
    /// WINEPREFIX dir, used to find leftover Wine processes via /proc.
    pub wineprefix: Option<PathBuf>,
}

pub(crate) struct SessionManager {
    sink: Arc<dyn EventSink>,
    logs_dir: PathBuf,
    flatpak: bool,
    grace: Duration,
    sessions: Mutex<Vec<Arc<Session>>>,
}

const MAX_FINISHED_KEPT: usize = 50;

impl SessionManager {
    pub fn new(sink: Arc<dyn EventSink>, logs_dir: PathBuf, flatpak: bool, grace: Duration) -> Self {
        Self { sink, logs_dir, flatpak, grace, sessions: Mutex::new(Vec::new()) }
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions.lock().unwrap().iter().map(|s| s.snapshot()).collect()
    }

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().iter().find(|s| s.id == id).cloned()
    }

    fn get_or_err(&self, id: &str) -> Result<Arc<Session>, AppError> {
        self.get(id).ok_or_else(|| AppError::not_found(format!("session {id} not found")))
    }

    pub async fn stop(&self, id: &str, force: bool) -> Result<(), AppError> {
        self.get_or_err(id)?.stop(force).await
    }

    pub fn start_aux(&self, id: &str, aux_id: &str) -> Result<SessionInfo, AppError> {
        self.get_or_err(id)?.start_aux(aux_id)
    }

    pub fn log_path(&self, id: &str) -> Option<PathBuf> {
        self.get(id).and_then(|s| s.log_path.clone())
    }

    /// Wait until the session reached Exited/Failed (tests, shutdown).
    pub async fn wait_finished(&self, id: &str, timeout: Duration) -> Option<SessionInfo> {
        let session = self.get(id)?;
        let mut rx = session.finished.subscribe();
        let _ = tokio::time::timeout(timeout, wait_true(&mut rx)).await;
        Some(session.snapshot())
    }

    pub fn start(&self, setup: SessionSetup) -> Result<SessionInfo, AppError> {
        let SessionSetup { request, wineserver_wait, wineserver_kill, wineprefix } = setup;
        let id = uuid::Uuid::new_v4().to_string();
        let started_at = vnl_core::now_secs();
        let info = SessionInfo {
            id: id.clone(),
            game_id: request.game_id,
            state: SessionState::Starting,
            started_at,
            ended_at: None,
            pid: None,
            exit_code: None,
            error: None,
            runner_id: Some(request.runner.id.clone()),
            prefix_path: Some(request.prefix.path.clone()),
            aux: request
                .aux
                .iter()
                .map(|(p, _)| AuxProcessInfo {
                    aux_id: p.id.clone(),
                    name: p.name.clone(),
                    pid: None,
                    running: false,
                    exit_code: None,
                })
                .collect(),
            command_preview: request.command.preview(),
        };

        let (log_file, log_path) = match open_log(&self.logs_dir, request.game_id, started_at, &id) {
            Ok((f, p)) => (Some(Arc::new(Mutex::new(f))), Some(p)),
            Err(e) => {
                self.sink.log(
                    vnl_core::LogEntry::new(LogLevel::Warn, "runtime", format!("cannot create session log: {e}"))
                        .session(id.clone()),
                );
                (None, None)
            }
        };
        let target = OutputTarget { sink: self.sink.clone(), session_id: Some(id.clone()), file: log_file };
        let session = Arc::new(Session {
            id: id.clone(),
            info: Mutex::new(info),
            target,
            flatpak: self.flatpak,
            grace: self.grace,
            aux_defs: request.aux.clone(),
            aux_groups: Mutex::new(HashMap::new()),
            companion_groups: Mutex::new(Vec::new()),
            game_pgid: Mutex::new(None),
            finished: watch::channel(false).0,
            game_exited: watch::channel(false).0,
            stop: watch::channel(false).0,
            wait_for_wineserver: request.config.wait_for_wineserver,
            wineserver_wait,
            wineserver_kill,
            wineprefix,
            log_path,
        });
        {
            let mut list = self.sessions.lock().unwrap();
            let finished = list.iter().filter(|s| s.is_finished()).count();
            if finished >= MAX_FINISHED_KEPT {
                if let Some(pos) = list.iter().position(|s| s.is_finished()) {
                    list.remove(pos);
                }
            }
            list.push(session.clone());
        }

        session.emit();
        session.log(LogLevel::Info, "runtime", format!("launching: {}", request.command.preview()));

        let mut child = match process::spawn_group(&request.command, self.flatpak) {
            Ok(child) => child,
            Err(e) => {
                session.log(LogLevel::Error, "runtime", e.to_string());
                session.update(|i| {
                    i.state = SessionState::Failed;
                    i.ended_at = Some(vnl_core::now_secs());
                    i.error = Some(e.message.clone());
                });
                session.finished.send_replace(true);
                return Err(e);
            }
        };
        let pid = child.id();
        *session.game_pgid.lock().unwrap() = pid.map(|p| p as i32);
        let pumps = process::capture(&mut child, "game", &session.target);
        let info = session.update(|i| {
            i.state = SessionState::Running;
            i.pid = pid;
        });

        for (n, spec) in request.companions.iter().enumerate() {
            session.spawn_companion(n, spec);
        }
        for (program, _) in request.aux.iter().filter(|(p, _)| p.autostart) {
            let session = session.clone();
            let aux_id = program.id.clone();
            let delay = Duration::from_millis(program.delay_ms);
            tokio::spawn(async move {
                let mut stop = session.stop.subscribe();
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = wait_true(&mut stop) => return,
                }
                if session.is_finished() || *session.game_exited.borrow() {
                    return;
                }
                if let Err(e) = session.start_aux(&aux_id) {
                    session.log(LogLevel::Warn, "runtime", format!("autostart of aux {aux_id} failed: {e}"));
                }
            });
        }

        tokio::spawn(session.clone().monitor(child, pumps));
        Ok(info)
    }
}

fn open_log(dir: &std::path::Path, game_id: i64, ts: i64, id: &str) -> std::io::Result<(std::fs::File, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("session-{game_id}-{ts}-{}.log", &id[..8]));
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    Ok((file, path))
}

async fn wait_true(rx: &mut watch::Receiver<bool>) {
    loop {
        let v = *rx.borrow_and_update();
        if v {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

pub(crate) struct Session {
    id: String,
    info: Mutex<SessionInfo>,
    target: OutputTarget,
    flatpak: bool,
    grace: Duration,
    aux_defs: Vec<(AuxProgram, CommandSpec)>,
    /// aux id -> process group of the running instance
    aux_groups: Mutex<HashMap<String, i32>>,
    companion_groups: Mutex<Vec<i32>>,
    game_pgid: Mutex<Option<i32>>,
    finished: watch::Sender<bool>,
    game_exited: watch::Sender<bool>,
    stop: watch::Sender<bool>,
    wait_for_wineserver: bool,
    wineserver_wait: Option<CommandSpec>,
    wineserver_kill: Option<CommandSpec>,
    wineprefix: Option<PathBuf>,
    log_path: Option<PathBuf>,
}

impl Session {
    pub fn snapshot(&self) -> SessionInfo {
        self.info.lock().unwrap().clone()
    }

    fn is_finished(&self) -> bool {
        *self.finished.borrow()
    }

    fn emit(&self) {
        let info = self.snapshot();
        self.target.sink.emit(AppEvent::Session(info));
    }

    /// Mutate the info and emit a session event with the new state.
    fn update(&self, f: impl FnOnce(&mut SessionInfo)) -> SessionInfo {
        let info = {
            let mut guard = self.info.lock().unwrap();
            f(&mut guard);
            guard.clone()
        };
        self.target.sink.emit(AppEvent::Session(info.clone()));
        info
    }

    fn log(&self, level: LogLevel, source: &str, msg: impl Into<String>) {
        self.target.log(level, source, msg);
    }

    fn excluded_groups(&self) -> HashSet<i32> {
        let mut set: HashSet<i32> = self.aux_groups.lock().unwrap().values().copied().collect();
        set.extend(self.companion_groups.lock().unwrap().iter().copied());
        set
    }

    fn spawn_companion(self: &Arc<Self>, n: usize, spec: &CommandSpec) {
        let name = spec.program.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| n.to_string());
        let source = format!("companion.{name}");
        let mut child = match process::spawn_group(spec, self.flatpak) {
            Ok(c) => c,
            Err(e) => {
                self.log(LogLevel::Warn, &source, format!("companion failed to start: {e}"));
                return;
            }
        };
        let Some(pgid) = child.id().map(|p| p as i32) else { return };
        self.companion_groups.lock().unwrap().push(pgid);
        let pumps = process::capture_single(&mut child, &source, &self.target);
        self.log(LogLevel::Info, &source, format!("started (pid {pgid}): {}", spec.preview()));
        let session = self.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            for p in pumps {
                let _ = tokio::time::timeout(Duration::from_secs(1), p).await;
            }
            session.companion_groups.lock().unwrap().retain(|g| *g != pgid);
            session.log(LogLevel::Info, &source, format!("exited: {}", describe(&status)));
        });
    }

    pub fn start_aux(self: &Arc<Self>, aux_id: &str) -> Result<SessionInfo, AppError> {
        if self.is_finished() {
            return Err(AppError::invalid("The game session has already ended"));
        }
        let (program, spec) = self
            .aux_defs
            .iter()
            .find(|(p, _)| p.id == aux_id)
            .cloned()
            .ok_or_else(|| AppError::not_found(format!("auxiliary program {aux_id} not found")))?;
        let source = format!("aux.{aux_id}");
        let (mut child, pgid) = {
            let mut groups = self.aux_groups.lock().unwrap();
            if groups.contains_key(aux_id) {
                return Err(AppError::invalid(format!("{} is already running", program.name)));
            }
            let child = process::spawn_group(&spec, self.flatpak).inspect_err(|e| {
                self.log(LogLevel::Warn, &source, format!("failed to start: {e}"));
            })?;
            let pgid = child.id().map(|p| p as i32).unwrap_or(0);
            groups.insert(aux_id.to_string(), pgid);
            (child, pgid)
        };
        let pumps = process::capture_single(&mut child, &source, &self.target);
        self.log(LogLevel::Info, &source, format!("started (pid {pgid}): {}", spec.preview()));
        let info = self.update(|i| {
            if let Some(a) = i.aux.iter_mut().find(|a| a.aux_id == aux_id) {
                a.pid = Some(pgid as u32);
                a.running = true;
                a.exit_code = None;
            }
        });
        let session = self.clone();
        let aux_id = aux_id.to_string();
        tokio::spawn(async move {
            let status = child.wait().await;
            for p in pumps {
                let _ = tokio::time::timeout(Duration::from_secs(1), p).await;
            }
            {
                let mut groups = session.aux_groups.lock().unwrap();
                if groups.get(&aux_id) == Some(&pgid) {
                    groups.remove(&aux_id);
                }
            }
            session.log(LogLevel::Info, &source, format!("exited: {}", describe(&status)));
            let code = status.as_ref().ok().and_then(|s| s.code());
            session.update(|i| {
                if let Some(a) = i.aux.iter_mut().find(|a| a.aux_id == aux_id) {
                    a.running = false;
                    a.exit_code = code;
                }
            });
        });
        Ok(info)
    }

    async fn monitor(self: Arc<Self>, mut child: tokio::process::Child, pumps: Vec<tokio::task::JoinHandle<()>>) {
        let status = child.wait().await;
        self.game_exited.send_replace(true);
        for p in pumps {
            // A grandchild may keep the pipe open; don't block on it.
            let _ = tokio::time::timeout(Duration::from_millis(500), p).await;
        }
        self.log(LogLevel::Info, "runtime", format!("game process exited: {}", describe(&status)));

        if self.wait_for_wineserver && !*self.stop.borrow() {
            self.log(LogLevel::Info, "runtime", "waiting for the prefix's wine processes to exit");
            self.wait_wine_idle().await;
        }

        self.cleanup().await;

        let stopped = *self.stop.borrow();
        let (state, exit_code, error) = match &status {
            Ok(s) => {
                let err = s.signal().filter(|_| !stopped).map(|sig| format!("terminated by signal {sig}"));
                (SessionState::Exited, s.code(), err)
            }
            Err(e) => (SessionState::Failed, None, Some(format!("failed to wait for the game: {e}"))),
        };
        self.update(|i| {
            i.state = state;
            i.ended_at = Some(vnl_core::now_secs());
            i.exit_code = exit_code;
            i.error = error;
        });
        self.finished.send_replace(true);
    }

    /// Wait until either `wineserver -w` returns or no non-system Wine process
    /// using this prefix is left (aux programs and companions excluded, so a
    /// running text hooker does not keep the session alive).
    async fn wait_wine_idle(&self) {
        let mut ws = self.wineserver_wait.as_ref().and_then(|spec| process::spawn_group(spec, self.flatpak).ok());
        let _ws_pumps = ws.as_mut().map(|c| process::capture_single(c, "wineserver", &self.target));
        let mut stop = self.stop.subscribe();
        let can_scan = !self.flatpak && self.wineprefix.is_some();
        let mut idle_polls = 0;
        loop {
            if ws.is_none() && !can_scan {
                break;
            }
            tokio::select! {
                _ = async {
                    match ws.as_mut() {
                        Some(c) => { let _ = c.wait().await; }
                        None => std::future::pending::<()>().await,
                    }
                } => break,
                _ = wait_true(&mut stop) => break,
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    if let (true, Some(pfx)) = (can_scan, &self.wineprefix) {
                        if process::wine_user_processes(pfx, &self.excluded_groups()).is_empty() {
                            idle_polls += 1;
                            if idle_polls >= 3 {
                                break;
                            }
                        } else {
                            idle_polls = 0;
                        }
                    }
                }
            }
        }
        if let Some(mut c) = ws
            && c.try_wait().ok().flatten().is_none()
        {
            let _ = c.start_kill();
            let _ = c.wait().await;
        }
    }

    /// Stop companions and `stop_with_game` aux programs.
    async fn cleanup(&self) {
        let mut groups: Vec<i32> = std::mem::take(&mut *self.companion_groups.lock().unwrap());
        {
            let aux = self.aux_groups.lock().unwrap();
            for (id, pgid) in aux.iter() {
                let stop_with_game =
                    self.aux_defs.iter().find(|(p, _)| &p.id == id).is_none_or(|(p, _)| p.stop_with_game);
                if stop_with_game {
                    groups.push(*pgid);
                }
            }
        }
        let handles: Vec<_> =
            groups.into_iter().map(|g| tokio::spawn(process::terminate_group(g, self.grace))).collect();
        for h in handles {
            let _ = h.await;
        }
    }

    pub async fn stop(self: &Arc<Self>, force: bool) -> Result<(), AppError> {
        if self.is_finished() {
            return Ok(());
        }
        self.stop.send_replace(true);
        self.update(|i| {
            if !i.state.is_finished() {
                i.state = SessionState::Stopping;
            }
        });
        self.log(LogLevel::Info, "runtime", if force { "force-stopping session" } else { "stopping session" });

        let pgid = *self.game_pgid.lock().unwrap();
        if let Some(pgid) = pgid {
            let tree = process::tree_of_group(pgid);
            process::send_group(pgid, Signal::SIGTERM);
            process::send_pids(&tree, Signal::SIGTERM);
            let deadline = tokio::time::Instant::now() + self.grace;
            loop {
                let done = *self.game_exited.borrow()
                    && !process::group_alive(pgid)
                    && tree.iter().all(|p| !process::pid_alive(*p));
                if done {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    self.log(LogLevel::Warn, "runtime", "game did not exit after SIGTERM; sending SIGKILL");
                    process::kill_tree(pgid, &tree);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        // Launcher-style games: the real game may have escaped the process tree.
        if self.wait_for_wineserver
            && !self.flatpak
            && let Some(pfx) = &self.wineprefix
        {
            let leftovers = process::wine_user_processes(pfx, &self.excluded_groups());
            if !leftovers.is_empty() {
                process::send_pids(&leftovers, Signal::SIGTERM);
                let deadline = tokio::time::Instant::now() + self.grace;
                while tokio::time::Instant::now() < deadline && leftovers.iter().any(|p| process::pid_alive(*p)) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                let alive: Vec<i32> = leftovers.iter().copied().filter(|p| process::pid_alive(*p)).collect();
                process::send_pids(&alive, Signal::SIGKILL);
            }
        }

        if force && let Some(spec) = &self.wineserver_kill {
            match process::run_to_completion(spec, self.flatpak, "wineserver", &self.target, Duration::from_secs(15))
                .await
            {
                Ok(_) => self.log(LogLevel::Info, "runtime", "wineserver -k done"),
                Err(e) => self.log(LogLevel::Warn, "runtime", format!("wineserver -k failed: {e}")),
            }
        }

        let mut rx = self.finished.subscribe();
        let wait = self.grace * 2 + Duration::from_secs(5);
        if tokio::time::timeout(wait, wait_true(&mut rx)).await.is_err() {
            return Err(AppError::new(vnl_core::ErrorCode::Process, "session did not finish after stopping"));
        }
        Ok(())
    }
}

fn describe(status: &std::io::Result<std::process::ExitStatus>) -> String {
    match status {
        Ok(s) => match (s.code(), s.signal()) {
            (Some(c), _) => format!("exit code {c}"),
            (None, Some(sig)) => format!("signal {sig}"),
            _ => "unknown status".into(),
        },
        Err(e) => format!("wait failed: {e}"),
    }
}
