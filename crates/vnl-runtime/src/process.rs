//! Process spawning in dedicated process groups, output capture and
//! process-tree signalling via `/proc`.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Child;
use vnl_core::{AppError, CommandSpec, ErrorCode, EventSink, LogEntry, LogLevel};

use crate::command::finalize_for_spawn;

/// Shared with the Tauri prefix-command bridge: hook hosts must not count as
/// the game when deciding whether a Wine prefix is still busy.
pub const TRANSLATION_HELPER_ENV: &str = "VNL_TRANSLATION_HELPER";

pub type LogFile = Arc<Mutex<std::fs::File>>;

/// Spawn `spec` (after the Flatpak transform) as the leader of a NEW process
/// group (pgid == pid), stdin closed, stdout/stderr piped.
pub fn spawn_group(spec: &CommandSpec, flatpak: bool) -> Result<Child, AppError> {
    let spec = finalize_for_spawn(spec, flatpak);
    let mut cmd = tokio::process::Command::from(spec.to_std_command());
    // AppImages may export Python paths under /tmp/.mount_*/usr. If inherited,
    // Proton's Python interpreter looks there for its standard library and
    // fails with "No module named 'encodings'". Clean the child environment
    // only; do not change the launcher's own environment.
    for key in ["PYTHONHOME", "PYTHONPATH", "PYTHONUSERBASE", "PYTHONEXECUTABLE"] {
        cmd.env_remove(key);
    }
    cmd.process_group(0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    cmd.spawn().map_err(|e| {
        let code =
            if e.kind() == std::io::ErrorKind::NotFound { ErrorCode::MissingDependency } else { ErrorCode::Process };
        AppError::new(code, format!("failed to start {}: {e}", spec.program.display())).with_details(spec.preview())
    })
}

/// Tracks prefix tools/installers independently of game sessions. Registration
/// and shutdown share a lock so no helper can appear after the shutdown snapshot.
#[derive(Default)]
pub(crate) struct ManagedGroups {
    state: Mutex<(bool, HashSet<i32>)>,
}

impl ManagedGroups {
    pub fn spawn(self: &Arc<Self>, spec: &CommandSpec, flatpak: bool) -> Result<(Child, ManagedGroup), AppError> {
        let mut state = self.state.lock().unwrap();
        if state.0 {
            return Err(AppError::invalid("The launcher is shutting down"));
        }
        let child = spawn_group(spec, flatpak)?;
        let pgid = child.id().ok_or_else(|| AppError::internal("missing child PID"))? as i32;
        state.1.insert(pgid);
        Ok((child, ManagedGroup { owner: self.clone(), pgid }))
    }

    pub async fn shutdown(&self) {
        let groups = {
            let mut state = self.state.lock().unwrap();
            state.0 = true;
            state.1.iter().copied().collect::<Vec<_>>()
        };
        let mut tasks = tokio::task::JoinSet::new();
        for pgid in groups {
            tasks.spawn(terminate_group(pgid, Duration::from_secs(2)));
        }
        while tasks.join_next().await.is_some() {}
    }
}

pub(crate) struct ManagedGroup {
    owner: Arc<ManagedGroups>,
    pgid: i32,
}

impl ManagedGroup {
    pub async fn finish(self) {
        // Tools can intentionally launch another process and return. Keep
        // ownership until that group exits naturally or shutdown terminates it.
        while group_alive(self.pgid) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // Drop removes the registration and provides a final kill fallback.
    }
}

impl Drop for ManagedGroup {
    fn drop(&mut self) {
        kill_tree(self.pgid, &[]);
        self.owner.state.lock().unwrap().1.remove(&self.pgid);
    }
}

/// Where captured output goes.
#[derive(Clone)]
pub struct OutputTarget {
    pub sink: Arc<dyn EventSink>,
    pub session_id: Option<String>,
    pub file: Option<LogFile>,
}

impl OutputTarget {
    pub fn log(&self, level: LogLevel, source: &str, message: impl Into<String>) {
        let mut entry = LogEntry::new(level, source, message);
        entry.session_id = self.session_id.clone();
        if let Some(file) = &self.file
            && let Ok(mut f) = file.lock()
        {
            let _ = writeln!(f, "{} [{}] {:?} {}", entry.ts_ms, entry.source, entry.level, entry.message);
        }
        self.sink.log(entry);
    }
}

/// Forward each line of `reader` as a log entry. Invalid UTF-8 (e.g.
/// Shift-JIS output) is replaced, never fatal.
pub fn pump<R>(reader: R, source: String, level: LogLevel, target: OutputTarget) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf);
                    let line = line.trim_end_matches(['\n', '\r']);
                    if !line.is_empty() {
                        target.log(level, &source, line);
                    }
                }
            }
        }
    })
}

/// Attach stdout/stderr pumps to a freshly spawned child.
pub fn capture(child: &mut Child, source: &str, target: &OutputTarget) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::new();
    if let Some(out) = child.stdout.take() {
        handles.push(pump(out, format!("{source}.stdout"), LogLevel::Info, target.clone()));
    }
    if let Some(err) = child.stderr.take() {
        handles.push(pump(err, format!("{source}.stderr"), LogLevel::Warn, target.clone()));
    }
    handles
}

/// Same as [`capture`] but stdout and stderr share one source name (`aux.<id>`).
pub fn capture_single(child: &mut Child, source: &str, target: &OutputTarget) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::new();
    if let Some(out) = child.stdout.take() {
        handles.push(pump(out, source.to_string(), LogLevel::Info, target.clone()));
    }
    if let Some(err) = child.stderr.take() {
        handles.push(pump(err, source.to_string(), LogLevel::Warn, target.clone()));
    }
    handles
}

// ------------------------------------------------------------------ /proc

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcStat {
    pub pid: i32,
    pub ppid: i32,
    pub pgrp: i32,
    pub state: char,
}

fn read_stat(pid: i32) -> Option<ProcStat> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm may contain spaces/parentheses: parse after the last ')'.
    let rest = &s[s.rfind(')')? + 1..];
    let mut f = rest.split_whitespace();
    let state = f.next()?.chars().next()?;
    let ppid = f.next()?.parse().ok()?;
    let pgrp = f.next()?.parse().ok()?;
    Some(ProcStat { pid, ppid, pgrp, state })
}

pub fn all_processes() -> Vec<ProcStat> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()))
        .filter_map(read_stat)
        .collect()
}

/// Live (non-zombie) members of a process group.
pub fn group_members(pgid: i32) -> Vec<i32> {
    all_processes().into_iter().filter(|p| p.pgrp == pgid && p.state != 'Z').map(|p| p.pid).collect()
}

/// All live descendants of the given roots (not including the roots).
pub fn descendants(roots: &[i32]) -> Vec<i32> {
    let procs = all_processes();
    let mut out = Vec::new();
    let mut frontier: Vec<i32> = roots.to_vec();
    let mut seen: HashSet<i32> = roots.iter().copied().collect();
    while let Some(parent) = frontier.pop() {
        for p in procs.iter().filter(|p| p.ppid == parent && p.state != 'Z') {
            if seen.insert(p.pid) {
                out.push(p.pid);
                frontier.push(p.pid);
            }
        }
    }
    out
}

/// Every live process in the group plus all their descendants (catches
/// children that moved to their own group/session, e.g. under gamescope).
pub fn tree_of_group(pgid: i32) -> Vec<i32> {
    let mut pids = group_members(pgid);
    let more = descendants(&pids);
    for p in more {
        if !pids.contains(&p) {
            pids.push(p);
        }
    }
    pids
}

pub fn group_alive(pgid: i32) -> bool {
    !group_members(pgid).is_empty()
}

pub fn send_group(pgid: i32, sig: Signal) {
    if pgid > 1 {
        let _ = signal::killpg(Pid::from_raw(pgid), sig);
    }
}

pub fn send_pids(pids: &[i32], sig: Signal) {
    let me = std::process::id() as i32;
    for &pid in pids {
        if pid > 1 && pid != me {
            let _ = signal::kill(Pid::from_raw(pid), sig);
        }
    }
}

/// SIGTERM the group and its descendants, wait up to `grace`, then SIGKILL
/// whatever is left. Returns true if something had to be SIGKILLed.
pub async fn terminate_group(pgid: i32, grace: Duration) -> bool {
    let tree = tree_of_group(pgid);
    send_group(pgid, Signal::SIGTERM);
    send_pids(&tree, Signal::SIGTERM);
    let deadline = tokio::time::Instant::now() + grace;
    while tokio::time::Instant::now() < deadline {
        if !group_alive(pgid) && tree.iter().all(|p| !pid_alive(*p)) {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    kill_tree(pgid, &tree);
    // SIGKILL delivery is asynchronous. Do not report cleanup complete while
    // a child is still runnable (the session monitor may reap it just after us).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        if !group_alive(pgid) && tree.iter().all(|p| !pid_alive(*p)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    true
}

/// SIGKILL the group, the previously captured `tree` and any new descendants.
pub fn kill_tree(pgid: i32, tree: &[i32]) {
    let mut all: Vec<i32> = tree.iter().copied().filter(|p| pid_alive(*p)).collect();
    all.extend(tree_of_group(pgid));
    all.extend(descendants(&all.clone()));
    send_group(pgid, Signal::SIGKILL);
    send_pids(&all, Signal::SIGKILL);
}

pub fn pid_alive(pid: i32) -> bool {
    read_stat(pid).is_some_and(|s| s.state != 'Z')
}

fn proc_env_var(pid: i32, key: &str) -> Option<OsString> {
    let data = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let prefix = format!("{key}=");
    data.split(|b| *b == 0)
        .find(|kv| kv.starts_with(prefix.as_bytes()))
        .map(|kv| OsString::from_vec(kv[prefix.len()..].to_vec()))
}

fn argv0_basename(pid: i32) -> Option<String> {
    let data = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let argv0 = data.split(|b| *b == 0).next()?;
    let s = String::from_utf8_lossy(argv0);
    Some(s.rsplit(['/', '\\']).next().unwrap_or("").to_ascii_lowercase())
}

/// Wine infrastructure processes that stay alive with the wineserver and do
/// not represent a running program.
const WINE_SYSTEM_PROCESSES: &[&str] = &[
    "wineserver",
    "services.exe",
    "explorer.exe",
    "winedevice.exe",
    "plugplay.exe",
    "svchost.exe",
    "rpcss.exe",
    "conhost.exe",
    "tabtip.exe",
    "rundll32.exe",
    "wineboot.exe",
    "mscorsvw.exe",
    "start.exe",
];

/// User processes (not Wine services) whose `WINEPREFIX` env is `wineprefix`,
/// excluding processes in `exclude_groups` (aux programs, companions).
pub fn wine_user_processes(wineprefix: &Path, exclude_groups: &HashSet<i32>) -> Vec<i32> {
    let want = crate::prefixes::canonical(wineprefix);
    let me = std::process::id() as i32;
    all_processes()
        .into_iter()
        .filter(|p| p.state != 'Z' && p.pid != me && !exclude_groups.contains(&p.pgrp))
        .filter(|p| {
            proc_env_var(p.pid, "WINEPREFIX")
                .map(|v| crate::prefixes::canonical(&PathBuf::from(v)) == want)
                .unwrap_or(false)
        })
        .filter(|p| proc_env_var(p.pid, TRANSLATION_HELPER_ENV).as_deref() != Some(std::ffi::OsStr::new("1")))
        .filter(|p| argv0_basename(p.pid).is_some_and(|n| !WINE_SYSTEM_PROCESSES.contains(&n.as_str())))
        .map(|p| p.pid)
        .collect()
}

/// Run a command to completion (own process group, output logged under
/// `source`), killing the group on timeout.
pub async fn run_to_completion(
    spec: &CommandSpec,
    flatpak: bool,
    source: &str,
    target: &OutputTarget,
    timeout: Duration,
) -> Result<std::process::ExitStatus, AppError> {
    let child = spawn_group(spec, flatpak)?;
    wait_to_completion(child, source, target, timeout).await
}

pub(crate) async fn wait_to_completion(
    mut child: Child,
    source: &str,
    target: &OutputTarget,
    timeout: Duration,
) -> Result<std::process::ExitStatus, AppError> {
    let pgid = child.id().map(|p| p as i32);
    let pumps = capture_single(&mut child, source, target);
    let result = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => status.map_err(|e| AppError::new(ErrorCode::Process, e.to_string())),
        Err(_) => {
            if let Some(pgid) = pgid {
                terminate_group(pgid, Duration::from_secs(3)).await;
            }
            let _ = child.wait().await;
            Err(AppError::new(ErrorCode::Process, format!("{} timed out after {}s", source, timeout.as_secs())))
        }
    };
    for p in pumps {
        let _ = tokio::time::timeout(Duration::from_secs(1), p).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use vnl_core::AppEvent;
    use vnl_core::events::RecordingSink;

    #[tokio::test(flavor = "multi_thread")]
    async fn spawned_commands_do_not_inherit_appimage_python_paths() {
        let spec = CommandSpec::new("/usr/bin/env")
            .env("PYTHONHOME", "/tmp/.mount_Yomika/usr")
            .env("PYTHONPATH", "/tmp/.mount_Yomika/usr/share/pyshared");
        let child = spawn_group(&spec, false).unwrap();
        let output = child.wait_with_output().await.unwrap();
        assert!(output.status.success());
        let env = String::from_utf8_lossy(&output.stdout);
        assert!(!env.lines().any(|line| line.starts_with("PYTHONHOME=")), "{env}");
        assert!(!env.lines().any(|line| line.starts_with("PYTHONPATH=")), "{env}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn captures_lossy_output_and_kills_tree() {
        let sink = RecordingSink::new();
        let target = OutputTarget { sink: Arc::new(sink.clone()), session_id: None, file: None };
        let spec = CommandSpec::new("/bin/sh").args(["-c", "printf 'hello\\n\\377bad\\n'; echo err >&2"]);
        let status = run_to_completion(&spec, false, "t", &target, Duration::from_secs(10)).await.unwrap();
        assert!(status.success());
        let msgs: Vec<String> = sink
            .events()
            .into_iter()
            .filter_map(|e| if let AppEvent::Log(l) = e { Some(format!("{}:{}", l.source, l.message)) } else { None })
            .collect();
        assert!(msgs.contains(&"t:hello".to_string()), "{msgs:?}");
        assert!(msgs.contains(&"t:\u{FFFD}bad".to_string()), "{msgs:?}");
        assert!(msgs.contains(&"t:err".to_string()), "{msgs:?}");

        // A child that escapes into its own process group is still found via the tree.
        let spec = CommandSpec::new("/bin/sh").args(["-c", "setsid sleep 30 & sleep 30 & wait"]);
        let mut child = spawn_group(&spec, false).unwrap();
        let pgid = child.id().unwrap() as i32;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let tree = tree_of_group(pgid);
        assert!(tree.len() >= 3, "{tree:?}");
        terminate_group(pgid, Duration::from_secs(2)).await;
        let _ = child.wait().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(tree.iter().all(|p| !pid_alive(*p)), "survivors in {tree:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn timeout_kills() {
        let target = OutputTarget { sink: Arc::new(vnl_core::NullSink), session_id: None, file: None };
        let spec = CommandSpec::new("/bin/sh").args(["-c", "trap '' TERM; sleep 30"]);
        let err = run_to_completion(&spec, false, "t", &target, Duration::from_millis(300)).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Process);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn finds_wine_user_processes_by_env() {
        let tmp = tempfile::tempdir().unwrap();
        let pfx = tmp.path().join("pfx ゲーム");
        std::fs::create_dir_all(&pfx).unwrap();
        let spec = CommandSpec::new("/bin/sleep").arg("30").env("WINEPREFIX", pfx.to_string_lossy());
        let mut child = spawn_group(&spec, false).unwrap();
        let pid = child.id().unwrap() as i32;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(wine_user_processes(&pfx, &HashSet::new()), vec![pid]);
        assert!(wine_user_processes(&pfx, &HashSet::from([pid])).is_empty());
        assert!(wine_user_processes(tmp.path(), &HashSet::new()).is_empty());
        let mut helper = spawn_group(&spec.env(TRANSLATION_HELPER_ENV, "1"), false).unwrap();
        let helper_pid = helper.id().unwrap() as i32;
        assert!(!wine_user_processes(&pfx, &HashSet::new()).contains(&helper_pid));
        send_group(helper_pid, Signal::SIGKILL);
        let _ = helper.wait().await;
        send_group(pid, Signal::SIGKILL);
        let _ = child.wait().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn managed_helpers_stop_on_shutdown_and_on_cancelled_owner() {
        let groups = Arc::new(ManagedGroups::default());
        let spec = CommandSpec::new("/bin/sh").args(["-c", "exec sleep 30"]);
        let (mut child, registration) = groups.spawn(&spec, false).unwrap();
        let pid = child.id().unwrap() as i32;
        groups.shutdown().await;
        let _ = child.wait().await;
        assert!(!pid_alive(pid));
        assert!(groups.spawn(&spec, false).is_err());
        drop(registration);

        let groups = Arc::new(ManagedGroups::default());
        let (mut child, registration) = groups.spawn(&spec, false).unwrap();
        let pid = child.id().unwrap() as i32;
        drop(registration); // cancelled task/future must not detach its process
        let _ = child.wait().await;
        assert!(!pid_alive(pid));
    }
}
