//! Service-level tests: prefixes, runner resolution and session lifecycle
//! with real short-lived processes (native runner). Real Wine/Proton
//! integration tests are `#[ignore]`d; run with `cargo test -p vnl-runtime -- --ignored`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use vnl_core::events::RecordingSink;
use vnl_core::{
    AppEvent, AppPaths, AuxProgram, CommandSpec, ErrorCode, Game, GameMetadata, PlayStatus, PrefixArch, PrefixChoice,
    PrefixInfo, PrefixKind, RunnerKind, RunnerRef, RuntimeConfig, SessionInfo, SessionState,
};

use super::*;

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    paths: AppPaths,
    sink: RecordingSink,
    svc: Arc<RuntimeService>,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let paths = AppPaths::under(&root);
    paths.ensure().unwrap();
    let sink = RecordingSink::new();
    let options = RuntimeOptions {
        roots: DiscoveryRoots { custom_file: Some(paths.config_dir.join("runners.json")), ..Default::default() },
        host: HostContext {
            fallback_steam_dir: paths.data_dir.join("steam-compat-client"),
            logs_dir: paths.logs_dir(),
            ..Default::default()
        },
        flatpak: false,
        stop_grace: Duration::from_secs(1),
    };
    let svc = Arc::new(RuntimeService::with_options(&paths, Arc::new(sink.clone()), options).unwrap());
    Fixture { _tmp: tmp, root, paths, sink, svc }
}

fn game(id: i64, exe: &str) -> Game {
    Game {
        id,
        title: format!("Game {id}"),
        original_title: None,
        exe_path: exe.into(),
        install_dir: Path::new(exe).parent().unwrap().to_path_buf(),
        working_dir: None,
        launch_args: vec![],
        status: PlayStatus::Unplayed,
        favorite: false,
        playtime_seconds: 0,
        last_played_at: None,
        added_at: 0,
        metadata: GameMetadata::default(),
        notes: None,
    }
}

fn dummy_prefix(root: &Path) -> PrefixInfo {
    PrefixInfo {
        id: "none".into(),
        name: "none".into(),
        path: root.join("no-prefix"),
        arch: PrefixArch::Win64,
        kind: PrefixKind::External,
        runner_id: None,
        created_at: 0,
        managed: false,
        initialized: false,
    }
}

fn sh(script: &str) -> CommandSpec {
    CommandSpec::new("/bin/sh").args(["-c", script])
}

fn native_request(f: &Fixture, script: &str) -> LaunchRequest {
    LaunchRequest {
        game_id: 1,
        runner: runners::native_runner(),
        prefix: dummy_prefix(&f.root),
        config: RuntimeConfig::default(),
        command: sh(script),
        companions: vec![],
        aux: vec![],
    }
}

fn aux(id: &str, autostart: bool, delay_ms: u64, stop_with_game: bool) -> AuxProgram {
    AuxProgram {
        id: id.into(),
        name: format!("Aux {id}"),
        exe_path: "/bin/sh".into(),
        windows: false,
        autostart,
        delay_ms,
        stop_with_game,
        ..Default::default()
    }
}

fn session_events(sink: &RecordingSink, id: &str) -> Vec<SessionInfo> {
    sink.events()
        .into_iter()
        .filter_map(|e| match e {
            AppEvent::Session(s) if s.id == id => Some(s),
            _ => None,
        })
        .collect()
}

fn states(sink: &RecordingSink, id: &str) -> Vec<SessionState> {
    let mut out: Vec<SessionState> = Vec::new();
    for s in session_events(sink, id) {
        if out.last() != Some(&s.state) {
            out.push(s.state);
        }
    }
    out
}

fn logs(sink: &RecordingSink, source: &str) -> Vec<String> {
    sink.events()
        .into_iter()
        .filter_map(|e| match e {
            AppEvent::Log(l) if l.source == source => Some(l.message),
            _ => None,
        })
        .collect()
}

fn pid_alive(pid: i32) -> bool {
    process::pid_alive(pid)
}

async fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    f()
}

async fn read_pid(file: &Path) -> i32 {
    assert!(wait_until(Duration::from_secs(5), || std::fs::read_to_string(file).is_ok_and(|s| s.ends_with('\n'))).await);
    std::fs::read_to_string(file).unwrap().trim().parse().unwrap()
}

// ------------------------------------------------------------- prefixes

#[test]
fn dedicated_prefix_is_registered_lazily_without_creating_dirs() {
    let f = fixture();
    let runner = runners::native_runner();
    let g = game(42, "/games/My Game/ゲーム.exe");
    let p = f.svc.resolve_prefix(&g, &PrefixChoice::Dedicated, &runner).unwrap();
    assert_eq!(p.id, "game-42");
    assert_eq!(p.path, f.paths.prefixes_dir().join("game-42"));
    assert_eq!(p.kind, PrefixKind::Dedicated { game_id: 42 });
    assert!(p.managed && !p.initialized);
    assert!(!p.path.exists(), "resolve_prefix must not create anything");
    // idempotent + persisted
    let again = f.svc.resolve_prefix(&g, &PrefixChoice::Dedicated, &runner).unwrap();
    assert_eq!(again.created_at, p.created_at);
    let json = std::fs::read_to_string(f.paths.data_dir.join("prefixes.json")).unwrap();
    assert!(json.contains("game-42"));
    assert_eq!(f.svc.list_prefixes().unwrap().len(), 1);

    let err = f.svc.resolve_prefix(&g, &PrefixChoice::Shared { id: "nope".into() }, &runner).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn custom_and_registered_prefixes_are_external_and_untouched() {
    let f = fixture();
    let dir = f.root.join("my prefixes/ゲーム pfx");
    std::fs::create_dir_all(dir.join("drive_c")).unwrap();
    std::fs::write(dir.join("system.reg"), "WINE REGISTRY Version 2\n#arch=win32\n").unwrap();
    let before: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();

    let runner = runners::native_runner();
    let p = f.svc.resolve_prefix(&game(1, "/g/a.exe"), &PrefixChoice::Custom { path: dir.clone() }, &runner).unwrap();
    assert_eq!(p.kind, PrefixKind::External);
    assert!(!p.managed && p.initialized);
    assert_eq!(p.arch, PrefixArch::Win32);
    assert_eq!(prefixes::wine_prefix_dir(&p), std::fs::canonicalize(&dir).unwrap());
    let again = f.svc.register_prefix(RegisterPrefixRequest { name: "x".into(), path: dir.clone() }).unwrap();
    assert_eq!(again.id, p.id);
    f.svc.prepare_prefix(&runner, &p).unwrap();
    let after: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(before, after);

    let shared = f.svc.resolve_prefix(&game(2, "/g/b.exe"), &PrefixChoice::Shared { id: p.id.clone() }, &runner).unwrap();
    assert_eq!(shared.id, p.id);

    let err = f.svc.register_prefix(RegisterPrefixRequest { name: "x".into(), path: f.root.join("missing") }).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn create_prefix_refuses_non_empty_dirs_and_bad_runners() {
    let f = fixture();
    let dir = f.root.join("occupied");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keep.txt"), "user data").unwrap();
    let wine = Runner {
        id: "wine:test".into(),
        name: "test".into(),
        kind: RunnerKind::Wine,
        source: vnl_core::RunnerSource::Custom,
        path: "/nonexistent/wine".into(),
        version: None,
    };
    let err = f.svc.create_prefix_at(&dir, "occupied", "Occupied", PrefixArch::Win64, &wine).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert_eq!(std::fs::read_to_string(dir.join("keep.txt")).unwrap(), "user data");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    assert!(f.svc.list_prefixes().unwrap().is_empty());

    let err = f
        .svc
        .create_prefix(CreatePrefixRequest { name: "x".into(), arch: PrefixArch::Win64, runner_id: "native".into() })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    let err = f
        .svc
        .create_prefix(CreatePrefixRequest { name: "x".into(), arch: PrefixArch::Win64, runner_id: "nope".into() })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn runner_resolution_and_custom_runners() {
    let f = fixture();
    // Only the native runner exists: no best runner.
    let err = f.svc.resolve_runner(None, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::MissingDependency);
    assert_eq!(f.svc.resolve_runner(Some(&RunnerRef { id: "native".into() }), None).unwrap().id, "native");

    let proton = f.root.join("protons/GE-Proton11-11");
    std::fs::create_dir_all(&proton).unwrap();
    std::fs::write(proton.join("proton"), "").unwrap();
    std::fs::write(proton.join("version"), "1 GE-Proton11-11\n").unwrap();
    let r = f.svc.add_custom_runner(&proton, Some("My GE")).unwrap();
    assert_eq!(r.id, "custom:my-ge");
    assert_eq!(r.kind, RunnerKind::Proton);
    assert_eq!(r.version.as_deref(), Some("GE-Proton11-11"));
    // adding the same path again returns the same runner
    assert_eq!(f.svc.add_custom_runner(&proton, None).unwrap().id, r.id);
    assert!(f.paths.config_dir.join("runners.json").is_file());

    let missing = RunnerRef { id: "proton:gone".into() };
    assert_eq!(f.svc.resolve_runner(Some(&missing), None).unwrap().id, r.id);
    assert_eq!(f.svc.resolve_runner(Some(&missing), Some(&RunnerRef { id: "native".into() })).unwrap().id, "native");
}

// ------------------------------------------------------------- sessions

#[tokio::test(flavor = "multi_thread")]
async fn session_exit_code_output_events_and_log_file() {
    let f = fixture();
    let info = f.svc.start_session(native_request(&f, "echo out ゲーム; echo err >&2; exit 3")).await.unwrap();
    assert_eq!(info.state, SessionState::Running);
    assert!(info.pid.is_some());
    let done = f.svc.wait_session(&info.id, Duration::from_secs(10)).await.unwrap();
    assert_eq!(done.state, SessionState::Exited);
    assert_eq!(done.exit_code, Some(3));
    assert!(done.error.is_none());
    assert!(done.ended_at.unwrap() >= done.started_at);
    assert_eq!(states(&f.sink, &info.id), vec![SessionState::Starting, SessionState::Running, SessionState::Exited]);
    let evs = session_events(&f.sink, &info.id);
    assert!(evs.iter().all(|e| e.started_at == done.started_at));
    assert_eq!(evs.last().unwrap(), &done);

    assert_eq!(logs(&f.sink, "game.stdout"), vec!["out ゲーム"]);
    assert_eq!(logs(&f.sink, "game.stderr"), vec!["err"]);
    let log_path = f.svc.session_log_path(&info.id).unwrap();
    assert!(log_path.starts_with(f.paths.logs_dir()));
    let content = std::fs::read_to_string(log_path).unwrap();
    assert!(content.contains("out ゲーム") && content.contains("[game.stderr]"), "{content}");
    assert_eq!(f.svc.session(&info.id).unwrap(), done);
    assert_eq!(f.svc.sessions().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn session_spawn_failure_emits_failed() {
    let f = fixture();
    let mut req = native_request(&f, "");
    req.command = CommandSpec::new("/nonexistent/My Game/ゲーム.exe");
    let err = f.svc.start_session(req).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::MissingDependency);
    let s = &f.svc.sessions()[0];
    assert_eq!(s.state, SessionState::Failed);
    assert!(s.ended_at.is_some() && s.error.is_some());
    assert_eq!(states(&f.sink, &s.id), vec![SessionState::Starting, SessionState::Failed]);
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_sends_term_to_whole_group() {
    let f = fixture();
    let pids = f.root.join("pids");
    let script = format!(
        "sleep 30 & echo $! >> '{p}'; setsid sleep 30 & echo $! >> '{p}'; echo end >> '{p}'; wait",
        p = pids.display()
    );
    let info = f.svc.start_session(native_request(&f, &script)).await.unwrap();
    assert!(wait_until(Duration::from_secs(5), || std::fs::read_to_string(&pids).is_ok_and(|s| s.contains("end"))).await);
    let children: Vec<i32> =
        std::fs::read_to_string(&pids).unwrap().lines().filter_map(|l| l.trim().parse().ok()).collect();
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|p| pid_alive(*p)));

    let start = Instant::now();
    f.svc.stop_session(&info.id, false).await.unwrap();
    assert!(start.elapsed() < Duration::from_millis(900), "TERM should suffice: {:?}", start.elapsed());
    let s = f.svc.session(&info.id).unwrap();
    assert_eq!(s.state, SessionState::Exited);
    assert!(s.error.is_none(), "user stop is not an error: {:?}", s.error);
    assert_eq!(states(&f.sink, &info.id).last(), Some(&SessionState::Exited));
    assert!(states(&f.sink, &info.id).contains(&SessionState::Stopping));
    assert!(wait_until(Duration::from_secs(2), || children.iter().all(|p| !pid_alive(*p))).await, "children survived");
    // stopping again is a no-op
    f.svc.stop_session(&info.id, false).await.unwrap();
    assert_eq!(f.svc.stop_session("nope", false).await.unwrap_err().code, ErrorCode::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_escalates_to_kill() {
    let f = fixture();
    let marker = f.root.join("ready");
    let script = format!("trap '' TERM; touch '{}'; while :; do sleep 0.1; done", marker.display());
    let info = f.svc.start_session(native_request(&f, &script)).await.unwrap();
    assert!(wait_until(Duration::from_secs(5), || marker.exists()).await);
    let pid = info.pid.unwrap() as i32;
    let start = Instant::now();
    f.svc.stop_session(&info.id, true).await.unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(900), "should wait the grace period: {elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    assert_eq!(f.svc.session(&info.id).unwrap().state, SessionState::Exited);
    assert!(!pid_alive(pid));
    assert!(process::group_members(pid).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn aux_programs_are_independent_of_the_game() {
    let f = fixture();
    let dir = &f.root;
    let mut req = native_request(&f, "sleep 30");
    let a_pid = dir.join("a.pid");
    let b_pid = dir.join("b.pid");
    let mk = |file: &Path| sh(&format!("echo $$ > '{}'; exec sleep 30", file.display()));
    req.aux = vec![
        (aux("a", false, 0, true), mk(&a_pid)),
        (aux("b", false, 0, false), mk(&b_pid)),
        (aux("quick", true, 100, true), sh("echo hello from aux; exit 4")),
    ];
    let info = f.svc.start_session(req).await.unwrap();
    assert_eq!(info.aux.len(), 3);

    // autostarted aux exits on its own; the game keeps running
    let id = info.id.clone();
    let svc = f.svc.clone();
    assert!(
        wait_until(Duration::from_secs(5), || {
            svc.session(&id).unwrap().aux.iter().any(|a| a.aux_id == "quick" && !a.running && a.exit_code == Some(4))
        })
        .await
    );
    assert_eq!(logs(&f.sink, "aux.quick"), vec!["hello from aux"]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(f.svc.session(&id).unwrap().state, SessionState::Running);

    f.svc.start_aux(&id, "a").await.unwrap();
    let s = f.svc.start_aux(&id, "b").await.unwrap();
    assert!(s.aux.iter().filter(|a| a.running).count() == 2);
    assert_eq!(f.svc.start_aux(&id, "a").await.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(f.svc.start_aux(&id, "zzz").await.unwrap_err().code, ErrorCode::NotFound);
    let a = read_pid(&a_pid).await;
    let b = read_pid(&b_pid).await;

    // killing an aux program does not affect the game
    process::send_group(a, nix::sys::signal::Signal::SIGKILL);
    assert!(
        wait_until(Duration::from_secs(3), || !svc.session(&id).unwrap().aux.iter().any(|x| x.aux_id == "a" && x.running))
            .await
    );
    assert_eq!(f.svc.session(&id).unwrap().state, SessionState::Running);
    // restart it
    f.svc.start_aux(&id, "a").await.unwrap();
    std::fs::remove_file(&a_pid).ok();
    let a = read_pid(&a_pid).await;

    // ending the game stops `stop_with_game` aux only
    f.svc.stop_session(&id, false).await.unwrap();
    assert!(wait_until(Duration::from_secs(3), || !pid_alive(a)).await, "stop_with_game aux survived");
    assert!(pid_alive(b), "aux without stop_with_game must keep running");
    let s = f.svc.session(&id).unwrap();
    assert!(s.aux.iter().any(|x| x.aux_id == "b" && x.running));
    assert_eq!(f.svc.start_aux(&id, "a").await.unwrap_err().code, ErrorCode::InvalidInput);
    process::send_group(b, nix::sys::signal::Signal::SIGKILL);
    assert!(wait_until(Duration::from_secs(3), || !f.svc.session(&id).unwrap().aux.iter().any(|x| x.running)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn companions_stop_with_the_session() {
    let f = fixture();
    let c_pid = f.root.join("c.pid");
    let mut req = native_request(&f, "sleep 1");
    req.companions = vec![sh(&format!("echo $$ > '{}'; exec sleep 30", c_pid.display()))];
    let info = f.svc.start_session(req).await.unwrap();
    let c = read_pid(&c_pid).await;
    assert!(pid_alive(c));
    let done = f.svc.wait_session(&info.id, Duration::from_secs(10)).await.unwrap();
    assert_eq!(done.state, SessionState::Exited);
    assert_eq!(done.exit_code, Some(0));
    assert!(!pid_alive(c), "companion survived the session");
}

#[tokio::test(flavor = "multi_thread")]
async fn wait_for_wineserver_waits_for_spawned_wine_processes_but_not_aux() {
    let f = fixture();
    let pfx = f.root.join("pfx dir/pfx");
    std::fs::create_dir_all(&pfx).unwrap();
    let mut req = native_request(&f, "setsid sh -c 'sleep 2' & exit 0");
    req.command.env.insert("WINEPREFIX".into(), pfx.to_string_lossy().into_owned());
    req.config.wait_for_wineserver = true;
    let mut aux_cmd = sh("exec sleep 30");
    aux_cmd.env.insert("WINEPREFIX".into(), pfx.to_string_lossy().into_owned());
    req.aux = vec![(aux("hooker", true, 0, true), aux_cmd)];
    let start = Instant::now();
    let info = f
        .svc
        .sessions
        .start(session::SessionSetup {
            request: req,
            wineserver_wait: None,
            wineserver_kill: None,
            wineprefix: Some(pfx.clone()),
        })
        .unwrap();
    let done = f.svc.wait_session(&info.id, Duration::from_secs(15)).await.unwrap();
    let elapsed = start.elapsed();
    assert_eq!(done.state, SessionState::Exited);
    assert!(elapsed >= Duration::from_secs(2), "ended before the spawned process: {elapsed:?}");
    assert!(elapsed < Duration::from_secs(8), "aux kept the session alive: {elapsed:?}");
    assert!(done.aux.iter().all(|a| !a.running) || {
        // aux monitor may lag a little behind the final event
        wait_until(Duration::from_secs(3), || f.svc.session(&info.id).unwrap().aux.iter().all(|a| !a.running)).await
    });
}

#[test]
fn request_types_serialize_like_the_ts_contract() {
    let req: PrefixToolRequest =
        serde_json::from_str(r#"{"prefixId":"p","runnerId":"r","tool":"killAll"}"#).unwrap();
    assert_eq!(req.tool, PrefixTool::KillAll);
    assert!(req.verbs.is_none());
    let req: InstallerRequest = serde_json::from_str(
        r#"{"installerPath":"/x/セットアップ.exe","runnerId":"r","prefix":{"type":"custom","path":"/p"},"nameHint":"n"}"#,
    )
    .unwrap();
    assert_eq!(req.prefix, PrefixChoice::Custom { path: "/p".into() });
    let run = InstallerRun { task_id: "t".into(), prefix: dummy_prefix(Path::new("/r")), drive_c: "/c".into() };
    let v = serde_json::to_value(&run).unwrap();
    assert!(v.get("taskId").is_some() && v.get("driveC").is_some());
    let c: CreatePrefixRequest = serde_json::from_str(r#"{"name":"n","arch":"win32","runnerId":"r"}"#).unwrap();
    assert_eq!(c.arch, PrefixArch::Win32);
}

#[test]
fn winetricks_uses_runner_binaries() {
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join("GE");
    std::fs::create_dir_all(pdir.join("files/bin")).unwrap();
    std::fs::write(pdir.join("files/bin/wine"), "").unwrap();
    std::fs::write(pdir.join("files/bin/wineserver"), "").unwrap();
    let runner = Runner {
        id: "proton:GE".into(),
        name: "GE".into(),
        kind: RunnerKind::Proton,
        source: vnl_core::RunnerSource::Steam,
        path: pdir.clone(),
        version: None,
    };
    let mut prefix = dummy_prefix(tmp.path());
    prefix.managed = true;
    let host = HostContext { winetricks: Some("/usr/bin/winetricks".into()), ..Default::default() };
    let spec = tools::winetricks_command(&host, &runner, &prefix, &["cjkfonts".into()]).unwrap();
    assert_eq!(spec.args, vec!["--unattended", "cjkfonts"]);
    assert_eq!(PathBuf::from(&spec.env["WINE"]), pdir.join("files/bin/wine"));
    assert_eq!(PathBuf::from(&spec.env["WINESERVER"]), pdir.join("files/bin/wineserver"));
    assert_eq!(PathBuf::from(&spec.env["WINEPREFIX"]), prefix.path.join("pfx"));
    assert!(tools::winetricks_command(&host, &runner, &prefix, &["-x".into()]).is_err());
    let err = tools::winetricks_command(&HostContext::default(), &runner, &prefix, &[]).unwrap_err();
    assert_eq!(err.code, ErrorCode::MissingDependency);
}

// ------------------------------------------------- real Wine / Proton runs

fn real_service() -> (tempfile::TempDir, AppPaths, RecordingSink, Arc<RuntimeService>) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(&tmp.path().join("vnl home ゲーム"));
    paths.ensure().unwrap();
    let sink = RecordingSink::new();
    let mut options = RuntimeOptions::detect(&paths);
    options.roots.custom_file = None;
    let svc = Arc::new(RuntimeService::with_options(&paths, Arc::new(sink.clone()), options).unwrap());
    (tmp, paths, sink, svc)
}

fn cmd_game(dir: &Path) -> Game {
    let mut g = game(7, "/unused/x.exe");
    g.exe_path = "cmd".into();
    g.launch_args = vec!["/c".into(), "echo".into(), "hello".into()];
    g.install_dir = dir.to_path_buf();
    g.working_dir = Some(dir.to_path_buf());
    g
}

fn quiet_config() -> RuntimeConfig {
    let mut c = RuntimeConfig::default();
    c.proton.launcher = vnl_core::ProtonLauncher::Direct;
    c.env = BTreeMap::new();
    c
}

async fn real_run(runner_id: &str) {
    let (tmp, paths, sink, svc) = real_service();
    let runner = svc.find_runner(runner_id).expect("runner installed");
    let dir = paths.prefixes_dir().join("test prefix");
    let prefix = svc.create_prefix_at(&dir, "test", "Test ゲーム", PrefixArch::Win64, &runner).await.unwrap();
    assert!(prefix.initialized, "drive_c missing");
    let g = cmd_game(tmp.path());
    let config = quiet_config();
    let command = svc.build_game_command(&g, &config, &runner, &prefix).unwrap();
    eprintln!("command: {}", command.preview());
    let info = svc
        .start_session(LaunchRequest {
            game_id: 7,
            runner: runner.clone(),
            prefix: prefix.clone(),
            config,
            command,
            companions: vec![],
            aux: vec![],
        })
        .await
        .unwrap();
    let done = svc.wait_session(&info.id, Duration::from_secs(180)).await.unwrap();
    let out = logs(&sink, "game.stdout");
    eprintln!("final: {done:?}\nstdout: {out:?}");
    assert_eq!(done.state, SessionState::Exited);
    assert_eq!(done.exit_code, Some(0));
    assert!(out.iter().any(|l| l.trim() == "hello"), "{out:?}");
    if let Some(ws) = command::wineserver_command(&runner, &prefix, "-k") {
        let _ = ws.to_std_command().status();
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs system wine; initialises a prefix in a tempdir"]
async fn real_system_wine_session() {
    real_run(SYSTEM_WINE_ID).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs GE-Proton11-11 in compatibilitytools.d; initialises a prefix in a tempdir"]
async fn real_ge_proton_session() {
    real_run("proton:GE-Proton11-11-x86_64").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs GE-Proton11-11; runs an aux program via runinprefix while the game runs"]
async fn real_proton_aux_runinprefix() {
    let (tmp, paths, sink, svc) = real_service();
    let runner = svc.find_runner("proton:GE-Proton11-11-x86_64").expect("GE-Proton installed");
    let dir = paths.prefixes_dir().join("aux prefix");
    let prefix = svc.create_prefix_at(&dir, "aux", "Aux", PrefixArch::Win64, &runner).await.unwrap();
    let mut g = cmd_game(tmp.path());
    g.launch_args = ["/c", "ping", "-n", "8", "127.0.0.1"].iter().map(|s| s.to_string()).collect();
    let config = quiet_config();
    let command = svc.build_game_command(&g, &config, &runner, &prefix).unwrap();
    let aux_cmd = svc
        .build_prefix_command(Path::new("cmd"), &["/c".into(), "echo".into(), "aux".into()], None, &config, &runner, &prefix)
        .unwrap();
    eprintln!("aux: {}", aux_cmd.preview());
    assert_eq!(aux_cmd.args[0], "runinprefix");
    let info = svc
        .start_session(LaunchRequest {
            game_id: 7,
            runner: runner.clone(),
            prefix: prefix.clone(),
            config,
            command,
            companions: vec![],
            aux: vec![(aux("echo", false, 0, true), aux_cmd)],
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(svc.session(&info.id).unwrap().state, SessionState::Running);
    svc.start_aux(&info.id, "echo").await.unwrap();
    let id = info.id.clone();
    let s2 = svc.clone();
    assert!(
        wait_until(Duration::from_secs(60), || s2.session(&id).unwrap().aux.iter().any(|a| !a.running && a.exit_code.is_some()))
            .await
    );
    let s = svc.session(&id).unwrap();
    eprintln!("after aux: {s:?}\naux log: {:?}", logs(&sink, "aux.echo"));
    assert_eq!(s.aux[0].exit_code, Some(0));
    assert!(logs(&sink, "aux.echo").iter().any(|l| l.trim() == "aux"));
    assert_eq!(s.state, SessionState::Running, "game must keep running after the aux program exits");
    let done = svc.wait_session(&id, Duration::from_secs(120)).await.unwrap();
    eprintln!("final: {done:?}");
    assert_eq!(done.state, SessionState::Exited);
    assert_eq!(done.exit_code, Some(0));
    if let Some(ws) = command::wineserver_command(&runner, &prefix, "-k") {
        let _ = ws.to_std_command().status();
    }
}
