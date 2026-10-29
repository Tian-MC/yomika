//! `LunaHostAdapter`: optional external hook host running inside the game's
//! Wine/Proton prefix.
//!
//! Supported user-provided binaries (never downloaded automatically), placed
//! in `<tools_dir>/lunahook/` (any sub-folder):
//! - `LunaHostCLI32.exe` / `LunaHostCLI64.exe` — the console host shipped by
//!   the former HIllya51/LunaHook releases (3.x). Needs `LunaHost32/64.dll` and
//!   `LunaHook32/64.dll` next to it. (Current LunaTranslator releases no longer
//!   ship a CLI: only `LunaHost64.dll` with a C API used by its Python UI.)
//! - `TextractorCLI.exe` from Textractor's `x86/` or `x64/` folder (same
//!   protocol, needs `texthook.dll`).
//!
//! Flow: wait `attachDelayMs` → find the game's Windows PID with
//! `tasklist.exe` in the prefix → start the CLI matching the game's PE
//! architecture via `PrefixCommandBuilder` (runinprefix under Proton) →
//! `attach -P<pid>` (+ optional hook code) → parse UTF-16 output.
//!
//! STATUS: implemented and unit-tested against a fake CLI that replays the
//! documented protocol; **not tested against a real game** (none available
//! on the development machine). Whether DLL injection works depends on the
//! Wine/Proton build and the game.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use vnl_core::{AppError, ErrorCode, PrefixCommandBuilder, TextOrigin};

use super::HookAdapter;
use super::pe::{PeArch, pe_arch};
use super::process::{PidMatch, find_game, linux_pids_for_exe, list_windows_processes};
use super::protocol::{
    HostEvent, HostMessageKind, HostParser, StreamDecoder, classify_console, encode_utf16le, hook_code_matches,
    validate_hook_code,
};
use crate::source::{SourceStatus, StatusCell, TextEvent, TextSender, TextSource};
use crate::types::{BackendInfo, TextSourceState};

pub const BACKEND_ID: &str = "lunahost";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliFlavor {
    LunaHost,
    Textractor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCli {
    pub path: PathBuf,
    pub arch: PeArch,
    pub flavor: CliFlavor,
}

impl HostCli {
    /// DLLs the host loads from its own folder.
    pub fn missing_dlls(&self) -> Vec<String> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let bits = if self.arch == PeArch::X64 { "64" } else { "32" };
        let wanted: Vec<String> = match self.flavor {
            CliFlavor::LunaHost => vec![format!("LunaHost{bits}.dll"), format!("LunaHook{bits}.dll")],
            CliFlavor::Textractor => vec!["texthook.dll".into()],
        };
        let present: Vec<String> = std::fs::read_dir(dir)
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_lowercase()).collect())
            .unwrap_or_default();
        wanted.into_iter().filter(|w| !present.contains(&w.to_lowercase())).collect()
    }
}

pub fn lunahook_dir(tools_dir: &Path) -> PathBuf {
    tools_dir.join("lunahook")
}

/// Finds host CLIs under `dir` (depth ≤ 3) and reads their PE architecture.
pub fn locate_clis(dir: &Path) -> Vec<HostCli> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<HostCli>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            if path.is_dir() {
                if depth < 3 {
                    walk(&path, depth + 1, out);
                }
                continue;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            let flavor = match name.as_str() {
                "lunahostcli32.exe" | "lunahostcli64.exe" | "lunahostcli.exe" => CliFlavor::LunaHost,
                "textractorcli.exe" => CliFlavor::Textractor,
                _ => continue,
            };
            if let Ok(Some(arch)) = pe_arch(&path) {
                out.push(HostCli { path, arch, flavor });
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out
}

pub fn pick_cli(clis: &[HostCli], game: PeArch) -> Option<&HostCli> {
    // A 64-bit game needs the x64 host; a 32-bit game the x86 host.
    let want = if game == PeArch::X64 { PeArch::X64 } else { PeArch::X86 };
    clis.iter()
        .find(|c| c.arch == want && c.flavor == CliFlavor::LunaHost)
        .or_else(|| clis.iter().find(|c| c.arch == want))
}

pub fn install_hint(tools_dir: &Path) -> String {
    format!(
        "Place LunaHostCLI32.exe/LunaHostCLI64.exe (with LunaHost*.dll and LunaHook*.dll) from a LunaHook 3.x release, \
         or Textractor's x86/x64 TextractorCLI.exe (with texthook.dll), in {}. Yomika never downloads hook hosts. \
         Alternatively use the 'clipboard' hook backend with any hooker that copies text to the clipboard.",
        lunahook_dir(tools_dir).display()
    )
}

pub fn backend_info(tools_dir: &Path) -> BackendInfo {
    let dir = lunahook_dir(tools_dir);
    let clis = locate_clis(&dir);
    let mut diagnostics = Vec::new();
    if clis.is_empty() {
        diagnostics.push(format!("No hook host CLI found in {}.", dir.display()));
        diagnostics.push(install_hint(tools_dir));
    }
    for c in &clis {
        let missing = c.missing_dlls();
        diagnostics.push(format!(
            "{:?} host {} ({}){}",
            c.flavor,
            c.path.display(),
            c.arch.label(),
            if missing.is_empty() { String::new() } else { format!(" — missing next to it: {}", missing.join(", ")) }
        ));
    }
    if !clis.iter().any(|c| c.arch == PeArch::X86) && !clis.is_empty() {
        diagnostics.push("No 32-bit host: most visual novels are 32-bit and need the x86 host.".into());
    }
    diagnostics.push(
        "Runs inside the game's prefix; the game's Windows PID is found with Wine's tasklist.exe. \
         Implemented but untested against a real game — injection depends on Wine/Proton and the engine."
            .into(),
    );
    BackendInfo {
        id: BACKEND_ID.into(),
        name: "LunaHook / Textractor CLI host".into(),
        available: !clis.is_empty(),
        diagnostics,
    }
}

#[derive(Debug, Clone)]
pub struct LunaHostOptions {
    pub attach_delay: Duration,
    /// How long to wait for the game process to show up in tasklist.
    pub process_timeout: Duration,
    pub process_poll: Duration,
    /// Warn when no game text arrived within this time after attaching.
    pub no_text_warning: Duration,
    /// Idle time after which a pending sentence is emitted.
    pub flush_after: Duration,
}

impl Default for LunaHostOptions {
    fn default() -> Self {
        Self {
            attach_delay: Duration::ZERO,
            process_timeout: Duration::from_secs(90),
            process_poll: Duration::from_secs(2),
            no_text_warning: Duration::from_secs(30),
            flush_after: Duration::from_millis(150),
        }
    }
}

struct Running {
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
    pgid: Arc<Mutex<Option<i32>>>,
}

pub struct LunaHostAdapter {
    tools_dir: PathBuf,
    game_exe: PathBuf,
    prefix: Arc<dyn PrefixCommandBuilder>,
    hook_code: Option<String>,
    opts: LunaHostOptions,
    status: StatusCell,
    running: Mutex<Option<Running>>,
}

impl LunaHostAdapter {
    pub fn new(
        tools_dir: PathBuf,
        game_exe: PathBuf,
        prefix: Arc<dyn PrefixCommandBuilder>,
        hook_code: Option<String>,
        opts: LunaHostOptions,
    ) -> Self {
        Self {
            tools_dir,
            game_exe,
            prefix,
            hook_code: hook_code.filter(|c| !c.trim().is_empty()),
            opts,
            status: StatusCell::default(),
            running: Mutex::new(None),
        }
    }

    /// Synchronous checks: CLI present, architecture, hook code.
    pub fn preflight(&self) -> Result<(HostCli, Option<String>), AppError> {
        let game_arch = match pe_arch(&self.game_exe) {
            Ok(Some(a)) => a,
            Ok(None) => {
                self.status
                    .push(format!("{} is not a Windows PE executable; assuming 32-bit.", self.game_exe.display()));
                PeArch::X86
            }
            Err(e) => {
                self.status.push(format!("Cannot read {}: {e}; assuming 32-bit.", self.game_exe.display()));
                PeArch::X86
            }
        };
        self.status.push(format!("Game executable is {}.", game_arch.label()));
        let clis = locate_clis(&lunahook_dir(&self.tools_dir));
        if clis.is_empty() {
            return Err(AppError::new(ErrorCode::MissingDependency, "Hook host CLI not found")
                .with_details(install_hint(&self.tools_dir)));
        }
        let cli = pick_cli(&clis, game_arch).cloned().ok_or_else(|| {
            let have: Vec<String> = clis.iter().map(|c| format!("{} ({})", c.path.display(), c.arch.label())).collect();
            AppError::new(
                ErrorCode::MissingDependency,
                format!(
                    "Architecture mismatch: the game is {} but only these hook hosts were found: {}",
                    game_arch.label(),
                    have.join(", ")
                ),
            )
            .with_details(install_hint(&self.tools_dir))
        })?;
        let missing = cli.missing_dlls();
        if !missing.is_empty() {
            self.status.push(format!(
                "Warning: {} next to {} — the host will probably fail.",
                missing.join(", "),
                cli.path.display()
            ));
        }
        let code = match &self.hook_code {
            Some(c) => Some(validate_hook_code(c).map_err(|e| AppError::invalid(format!("Invalid hook code: {e}")))?),
            None => None,
        };
        Ok((cli, code))
    }
}

fn kill_group(pgid: i32) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    let _ = killpg(Pid::from_raw(pgid), Signal::SIGTERM);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        let _ = killpg(Pid::from_raw(pgid), Signal::SIGKILL);
    });
}

struct Ctx {
    exe_name: String,
    prefix: Arc<dyn PrefixCommandBuilder>,
    cli: HostCli,
    hook_code: Option<String>,
    opts: LunaHostOptions,
    status: StatusCell,
    tx: TextSender,
    pgid: Arc<Mutex<Option<i32>>>,
}

async fn find_pid(ctx: &Ctx, stop: &mut oneshot::Receiver<()>) -> Option<u32> {
    let deadline = Instant::now() + ctx.opts.process_timeout;
    let mut last_err = None;
    let mut last_candidates = Vec::new();
    loop {
        match list_windows_processes(ctx.prefix.as_ref()).await {
            Ok(procs) => match find_game(&procs, &ctx.exe_name) {
                PidMatch::Exact(p) => {
                    ctx.status
                        .push(format!("Found {} as Windows PID {} (tasklist in the game prefix).", p.image, p.pid));
                    return Some(p.pid);
                }
                PidMatch::SoleCandidate(p) => {
                    ctx.status.push(format!(
                        "No process named {} in tasklist; using the only non-system process {} (PID {}).",
                        ctx.exe_name, p.image, p.pid
                    ));
                    return Some(p.pid);
                }
                PidMatch::NotFound { candidates } => last_candidates = candidates,
            },
            Err(e) => last_err = Some(e.message),
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(ctx.opts.process_poll) => {}
            _ = &mut *stop => return None,
        }
    }
    let mut msg =
        format!("Game process {} not found in the prefix after {:?}.", ctx.exe_name, ctx.opts.process_timeout);
    if let Some(e) = last_err {
        msg.push_str(&format!(" tasklist error: {e}."));
    }
    if !last_candidates.is_empty() {
        let names: Vec<String> = last_candidates.iter().map(|p| format!("{} ({})", p.image, p.pid)).collect();
        msg.push_str(&format!(" Running Windows processes: {}.", names.join(", ")));
    }
    let linux = linux_pids_for_exe(&ctx.exe_name);
    if !linux.is_empty() {
        msg.push_str(&format!(
            " It IS running as Linux PID(s) {linux:?}: it may use a different prefix/wineserver than the hook host."
        ));
    }
    ctx.status.fail(msg);
    None
}

async fn run_host(ctx: Ctx, mut stop: oneshot::Receiver<()>, attach_delay: Duration) {
    if !attach_delay.is_zero() {
        tokio::select! {
            _ = tokio::time::sleep(attach_delay) => {}
            _ = &mut stop => return,
        }
    }
    let Some(pid) = find_pid(&ctx, &mut stop).await else { return };

    let cwd = ctx.cli.path.parent().map(Path::to_path_buf);
    let spec = match ctx.prefix.build(&ctx.cli.path, &[], cwd.as_deref()) {
        Ok(s) => s,
        Err(e) => return ctx.status.fail(format!("Cannot build the host command: {}", e.message)),
    };
    let mut cmd = tokio::process::Command::from(spec.to_std_command());
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ctx.status.fail(format!("Cannot start {}: {e}", spec.preview())),
    };
    if let Some(id) = child.id() {
        *ctx.pgid.lock().unwrap() = Some(id as i32);
    }
    ctx.status.push(format!("Started hook host: {}", spec.preview()));
    let mut stdin = child.stdin.take().expect("piped stdin");
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let stderr_tail = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let tail = stderr_tail.clone();
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf).await;
            let text = String::from_utf8_lossy(&buf);
            let mut t = tail.lock().unwrap();
            t.extend(text.lines().rev().take(5).map(str::to_string));
            t.reverse();
        });
    }

    let mut commands = vec![format!("attach -P{pid}\n")];
    if let Some(code) = &ctx.hook_code {
        commands.push(format!("{code} -P{pid}\n"));
    }
    for c in &commands {
        if let Err(e) = stdin.write_all(&encode_utf16le(c)).await {
            ctx.status.fail(format!("Cannot talk to the hook host: {e}"));
            let _ = child.kill().await;
            return;
        }
    }
    let _ = stdin.flush().await;
    ctx.status.push(format!(
        "Sent: attach -P{pid}{}",
        ctx.hook_code.as_ref().map(|c| format!(", hook code {c}")).unwrap_or_default()
    ));

    let mut decoder = StreamDecoder::default();
    let mut parser = HostParser::default();
    let mut threads: BTreeMap<String, u64> = BTreeMap::new();
    let mut got_text = false;
    let mut connected = false;
    let mut warned = false;
    let attached_at = Instant::now();
    let mut buf = vec![0u8; 8192];
    let mut stopped = false;

    let handle_event = |ev: HostEvent,
                        threads: &mut BTreeMap<String, u64>,
                        got_text: &mut bool,
                        connected: &mut bool|
     -> Option<TextEvent> {
        match ev {
            HostEvent::Sentence(s) if s.is_console() => {
                let kind = classify_console(&s.text);
                ctx.status.push(format!("Host: {}", s.text.trim()));
                match kind {
                    HostMessageKind::PipeConnected => {
                        *connected = true;
                        ctx.status.set_state(TextSourceState::Active);
                    }
                    HostMessageKind::ArchitectureMismatch => ctx.status.fail(format!(
                        "Architecture mismatch reported by the host ({} host used).",
                        ctx.cli.arch.label()
                    )),
                    HostMessageKind::InjectFailed => ctx
                        .status
                        .fail("Injection failed. Wine/Proton may block it for this game, or the prefix differs."),
                    HostMessageKind::HookFailed | HostMessageKind::InvalidCode => {
                        ctx.status.push("The hook code was rejected; check it or remove it to use automatic hooks.")
                    }
                    _ => {}
                }
                None
            }
            HostEvent::Sentence(s) if s.is_host_clipboard() => None,
            HostEvent::Sentence(s) => {
                let key = format!("{} {}", s.name, s.hook_code);
                let count = threads.entry(key.clone()).or_insert(0);
                *count += 1;
                if *count == 1 {
                    ctx.status.push(format!("Text thread: {key}"));
                }
                if let Some(code) = &ctx.hook_code
                    && !hook_code_matches(code, &s.hook_code)
                {
                    return None;
                }
                if !*got_text {
                    *got_text = true;
                    ctx.status.set_state(TextSourceState::Active);
                    ctx.status.push("Receiving game text.");
                }
                Some(TextEvent { origin: TextOrigin::Hook, text: s.text, detail: Some(key) })
            }
            HostEvent::Usage => None,
            HostEvent::Noise(n) => {
                tracing::debug!(line = %n, "hook host output");
                None
            }
        }
    };

    loop {
        tokio::select! {
            _ = &mut stop => { stopped = true; break; }
            r = tokio::time::timeout(ctx.opts.flush_after, stdout.read(&mut buf)) => match r {
                Err(_) => {
                    if let Some(ev) = parser.flush()
                        && let Some(t) = handle_event(ev, &mut threads, &mut got_text, &mut connected)
                        && ctx.tx.send(t).await.is_err() { stopped = true; break; }
                    if !got_text && !warned && attached_at.elapsed() >= ctx.opts.no_text_warning {
                        warned = true;
                        ctx.status.push(format!(
                            "No text received {}s after attaching{}. Advance the dialogue; if nothing appears the engine \
                             may need a hook code (H-code), or injection silently failed under Wine.",
                            ctx.opts.no_text_warning.as_secs(),
                            if connected { "" } else { " (the host never reported a pipe connection)" }
                        ));
                    }
                    if ctx.status.get().state == TextSourceState::Failed { break; }
                }
                Ok(Ok(0)) | Ok(Err(_)) => break,
                Ok(Ok(n)) => {
                    let mut out = Vec::new();
                    for line in decoder.push(&buf[..n]) {
                        for ev in parser.feed_line(&line) {
                            if let Some(t) = handle_event(ev, &mut threads, &mut got_text, &mut connected) { out.push(t); }
                        }
                    }
                    for t in out {
                        if ctx.tx.send(t).await.is_err() { stopped = true; }
                    }
                    if stopped { break; }
                }
            }
        }
    }

    if let Some(rest) = decoder.finish() {
        for ev in parser.feed_line(&rest) {
            if let Some(t) = handle_event(ev, &mut threads, &mut got_text, &mut connected) {
                let _ = ctx.tx.send(t).await;
            }
        }
    }
    if let Some(ev) = parser.flush()
        && let Some(t) = handle_event(ev, &mut threads, &mut got_text, &mut connected)
    {
        let _ = ctx.tx.send(t).await;
    }

    let mut exit_status = None;
    if !stopped && ctx.status.get().state != TextSourceState::Failed {
        // stdout closed: wait for the host to exit, unless we are asked to stop.
        tokio::select! {
            _ = &mut stop => stopped = true,
            s = child.wait() => exit_status = s.ok(),
        }
    }
    if stopped || ctx.status.get().state == TextSourceState::Failed {
        // Detach politely so the game unloads the hook, then terminate the host.
        let _ = stdin.write_all(&encode_utf16le(&format!("detach -P{pid}\n"))).await;
        let _ = stdin.flush().await;
        drop(stdin);
        if tokio::time::timeout(Duration::from_millis(500), child.wait()).await.is_err() {
            if let Some(pg) = *ctx.pgid.lock().unwrap() {
                kill_group(pg);
            }
            let _ = child.kill().await;
        }
    } else {
        // Host ended on its own.
        let status = exit_status;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let tail = stderr_tail.lock().unwrap().join(" | ");
        ctx.status.fail(format!(
            "Hook host exited unexpectedly ({}){}",
            status.map(|s| s.to_string()).unwrap_or_else(|| "unknown status".into()),
            if tail.is_empty() { String::new() } else { format!(": {tail}") }
        ));
    }
    *ctx.pgid.lock().unwrap() = None;
}

#[async_trait]
impl TextSource for LunaHostAdapter {
    fn id(&self) -> &str {
        BACKEND_ID
    }

    fn origin(&self) -> TextOrigin {
        TextOrigin::Hook
    }

    async fn start(&self, tx: TextSender) -> Result<(), AppError> {
        self.stop().await;
        self.status.set_state(TextSourceState::Starting);
        let (cli, hook_code) = match self.preflight() {
            Ok(v) => v,
            Err(e) => {
                self.status.fail(format!(
                    "{}{}",
                    e.message,
                    e.details.as_ref().map(|d| format!(" — {d}")).unwrap_or_default()
                ));
                return Err(e);
            }
        };
        self.status.push(format!("Using {:?} host {} ({}).", cli.flavor, cli.path.display(), cli.arch.label()));
        let exe_name = self
            .game_exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| AppError::invalid("game executable path has no file name"))?;
        let (stop_tx, stop_rx) = oneshot::channel();
        let pgid = Arc::new(Mutex::new(None));
        let ctx = Ctx {
            exe_name,
            prefix: self.prefix.clone(),
            cli,
            hook_code,
            opts: self.opts.clone(),
            status: self.status.clone(),
            tx,
            pgid: pgid.clone(),
        };
        let task = tokio::spawn(run_host(ctx, stop_rx, self.opts.attach_delay));
        *self.running.lock().unwrap() = Some(Running { stop: stop_tx, task, pgid });
        Ok(())
    }

    async fn stop(&self) {
        let running = self.running.lock().unwrap().take();
        if let Some(r) = running {
            let _ = r.stop.send(());
            let mut task = r.task;
            if tokio::time::timeout(Duration::from_secs(3), &mut task).await.is_err() {
                task.abort();
                if let Some(pg) = *r.pgid.lock().unwrap() {
                    kill_group(pg);
                }
            }
            if self.status.get().state != TextSourceState::Failed {
                self.status.set_state(TextSourceState::Inactive);
            }
        }
    }

    fn status(&self) -> SourceStatus {
        self.status.get()
    }
}

impl HookAdapter for LunaHostAdapter {
    fn backend_id(&self) -> &str {
        BACKEND_ID
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::pe::fake_pe;
    use vnl_core::CommandSpec;

    /// Maps `tasklist.exe` and the CLI path to shell scripts (tests only).
    struct FakePrefix {
        dir: PathBuf,
    }

    impl PrefixCommandBuilder for FakePrefix {
        fn build(&self, exe: &Path, args: &[String], _cwd: Option<&Path>) -> Result<CommandSpec, AppError> {
            let name = exe.file_name().unwrap().to_string_lossy().to_lowercase();
            let script = if name == "tasklist.exe" { "tasklist.sh" } else { "host.sh" };
            Ok(CommandSpec::new("/bin/sh").arg(self.dir.join(script).to_string_lossy()).args(args.to_vec()))
        }
        fn prefix_path(&self) -> PathBuf {
            self.dir.join("pfx")
        }
    }

    fn setup(
        host_output: &str,
        tasklist: &str,
        game_machine: u16,
        cli_name: &str,
        cli_machine: u16,
    ) -> (tempfile::TempDir, LunaHostAdapter) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let tools = dir.join("tools");
        let host_dir = lunahook_dir(&tools).join("x86");
        std::fs::create_dir_all(&host_dir).unwrap();
        std::fs::write(host_dir.join(cli_name), fake_pe(cli_machine)).unwrap();
        for dll in ["LunaHost32.dll", "LunaHook32.dll", "LunaHost64.dll", "LunaHook64.dll"] {
            std::fs::write(host_dir.join(dll), b"").unwrap();
        }
        let game = dir.join("Game.exe");
        std::fs::write(&game, fake_pe(game_machine)).unwrap();
        std::fs::write(dir.join("tasklist.out"), tasklist).unwrap();
        std::fs::write(dir.join("host.out"), encode_utf16le(host_output)).unwrap();
        std::fs::write(dir.join("tasklist.sh"), format!("cat '{}'\n", dir.join("tasklist.out").display())).unwrap();
        std::fs::write(
            dir.join("host.sh"),
            format!(
                "cat '{out}'\ncat > '{stdin}'\n",
                out = dir.join("host.out").display(),
                stdin = dir.join("stdin.bin").display()
            ),
        )
        .unwrap();
        let opts = LunaHostOptions {
            process_timeout: Duration::from_millis(600),
            process_poll: Duration::from_millis(100),
            no_text_warning: Duration::from_millis(300),
            ..Default::default()
        };
        let adapter = LunaHostAdapter::new(tools, game, Arc::new(FakePrefix { dir: dir.clone() }), None, opts);
        (tmp, adapter)
    }

    const TASKLIST: &str =
        "\"services.exe\",\"48\",\"Console\",\"1\",\"1 K\"\n\"Game.exe\",\"356\",\"Console\",\"1\",\"9 K\"\n";

    fn host_output() -> String {
        [
            "Usage: {'attach'|'detach'|hookcode} -Pprocessid",
            "[0:0:FFFFFFFFFFFFFFFF:FFFFFFFFFFFFFFFF:FFFFFFFFFFFFFFFF:Console:HB0@0] Textractor: pipe connected",
            "[1:0:0:FFFFFFFFFFFFFFFF:FFFFFFFFFFFFFFFF:Clipboard:HB0@0] copied by host",
            "[17:164:67A78E:0:19F95C:System43:HSN4:-14@27A78E:Game.exe] わずかな灯りに照らされた道の真ん中で、",
            "大柄の男が、ずぶ濡れで立っていた。",
            "[18:164:67A790:0:0:GetGlyphOutlineA:HA8@0:gdi32.dll] 次の行",
            "",
        ]
        .join("\r\n")
    }

    #[tokio::test]
    async fn attaches_and_parses_output() {
        let (tmp, adapter) = setup(&host_output(), TASKLIST, 0x14C, "LunaHostCLI32.exe", 0x14C);
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        adapter.start(tx).await.unwrap();
        let first = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
        assert_eq!(first.origin, TextOrigin::Hook);
        assert_eq!(first.text, "わずかな灯りに照らされた道の真ん中で、\n大柄の男が、ずぶ濡れで立っていた。");
        assert!(first.detail.unwrap().contains("HSN4"));
        let second = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
        assert_eq!(second.text, "次の行");
        let st = adapter.status();
        assert_eq!(st.state, TextSourceState::Active);
        assert!(st.diagnostics.iter().any(|d| d.contains("Windows PID 356")), "{:?}", st.diagnostics);
        adapter.stop().await;
        let stdin = std::fs::read(tmp.path().join("stdin.bin")).unwrap();
        let sent =
            String::from_utf16_lossy(&stdin.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>());
        assert!(sent.starts_with("attach -P356\n"), "{sent:?}");
        assert!(sent.contains("detach -P356\n"), "{sent:?}");
        assert_eq!(adapter.status().state, TextSourceState::Inactive);
    }

    #[tokio::test]
    async fn hook_code_filters_threads_and_is_sent() {
        let (tmp, mut adapter) = setup(&host_output(), TASKLIST, 0x14C, "LunaHostCLI32.exe", 0x14C);
        adapter.hook_code = Some("/HA8@0".into());
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        adapter.start(tx).await.unwrap();
        let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
        assert_eq!(ev.text, "次の行");
        adapter.stop().await;
        let stdin = std::fs::read(tmp.path().join("stdin.bin")).unwrap();
        let sent =
            String::from_utf16_lossy(&stdin.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>());
        assert!(sent.contains("/HA8@0 -P356\n"), "{sent:?}");
    }

    #[tokio::test]
    async fn missing_cli_and_arch_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("g.exe");
        std::fs::write(&game, fake_pe(0x14C)).unwrap();
        let adapter = LunaHostAdapter::new(
            tmp.path().join("tools"),
            game,
            Arc::new(FakePrefix { dir: tmp.path().into() }),
            None,
            LunaHostOptions::default(),
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let err = adapter.start(tx).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::MissingDependency);
        assert_eq!(adapter.status().state, TextSourceState::Failed);

        // 32-bit game, only a 64-bit host.
        let (_tmp, adapter) = setup("", TASKLIST, 0x14C, "LunaHostCLI64.exe", 0x8664);
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let err = adapter.start(tx).await.unwrap_err();
        assert!(err.message.contains("Architecture mismatch"), "{}", err.message);
    }

    #[tokio::test]
    async fn invalid_hook_code_rejected_before_start() {
        let (_tmp, mut adapter) = setup("", TASKLIST, 0x14C, "LunaHostCLI32.exe", 0x14C);
        adapter.hook_code = Some("attach -P1".into());
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let err = adapter.start(tx).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn process_not_found_diagnostics() {
        let (_tmp, adapter) = setup(
            "",
            "\"services.exe\",\"48\",\"Console\",\"1\",\"1 K\"\n\"a.exe\",\"1\",\"C\",\"1\",\"1\"\n\"b.exe\",\"2\",\"C\",\"1\",\"1\"\n",
            0x14C,
            "LunaHostCLI32.exe",
            0x14C,
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        adapter.start(tx).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let st = adapter.status();
        assert_eq!(st.state, TextSourceState::Failed);
        let joined = st.diagnostics.join("\n");
        assert!(joined.contains("Game.exe not found"), "{joined}");
        assert!(joined.contains("a.exe (1)"), "{joined}");
        adapter.stop().await;
    }

    #[tokio::test]
    async fn host_reports_injection_failure_and_no_text() {
        let out =
            "[0:0:FFFFFFFFFFFFFFFF:FFFFFFFFFFFFFFFF:FFFFFFFFFFFFFFFF:Console:HB0@0] Textractor: couldn't inject\r\n";
        let (_tmp, adapter) = setup(out, TASKLIST, 0x14C, "TextractorCLI.exe", 0x14C);
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        adapter.start(tx).await.unwrap();
        tokio::time::sleep(Duration::from_millis(800)).await;
        let st = adapter.status();
        assert_eq!(st.state, TextSourceState::Failed);
        assert!(st.diagnostics.iter().any(|d| d.contains("Injection failed")), "{:?}", st.diagnostics);
        adapter.stop().await;
    }

    #[test]
    fn locate_and_pick() {
        let tmp = tempfile::tempdir().unwrap();
        let d = lunahook_dir(tmp.path());
        std::fs::create_dir_all(d.join("x64")).unwrap();
        std::fs::write(d.join("LunaHostCLI32.exe"), fake_pe(0x14C)).unwrap();
        std::fs::write(d.join("x64/TextractorCLI.exe"), fake_pe(0x8664)).unwrap();
        std::fs::write(d.join("other.exe"), fake_pe(0x14C)).unwrap();
        let clis = locate_clis(&d);
        assert_eq!(clis.len(), 2);
        assert_eq!(pick_cli(&clis, PeArch::X86).unwrap().flavor, CliFlavor::LunaHost);
        assert_eq!(pick_cli(&clis, PeArch::X64).unwrap().flavor, CliFlavor::Textractor);
        let info = backend_info(tmp.path());
        assert!(info.available);
        assert!(info.diagnostics.iter().any(|d| d.contains("missing next to it")));
        assert!(!backend_info(&tmp.path().join("nothing")).available);
    }
}
