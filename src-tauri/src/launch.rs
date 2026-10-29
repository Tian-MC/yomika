//! Tauri-independent launch pipeline: library -> runtime -> graphics.
//! Kept separate from `commands/session.rs` so it can be tested end to end
//! without a Tauri runtime.

use std::path::Path;

use vnl_core::{
    AppError, AppSettings, AuxProgram, CommandSpec, Game, GameId, GameSettings, PrefixInfo, Runner, RuntimeConfig,
    SessionInfo,
};
use vnl_graphics::GraphicsService;
use vnl_library::Library;
use vnl_runtime::{LaunchRequest, RuntimeService};

pub struct ResolvedLaunch {
    pub game: Game,
    pub settings: GameSettings,
    pub app_settings: AppSettings,
    pub runner: Runner,
    pub prefix: PrefixInfo,
    pub command: CommandSpec,
    pub companions: Vec<CommandSpec>,
    pub aux: Vec<(AuxProgram, CommandSpec)>,
    pub warnings: Vec<String>,
}

impl ResolvedLaunch {
    pub fn to_request(&self) -> LaunchRequest {
        LaunchRequest {
            game_id: self.game.id,
            runner: self.runner.clone(),
            prefix: self.prefix.clone(),
            config: self.settings.runtime.clone(),
            command: self.command.clone(),
            companions: self.companions.clone(),
            aux: self.aux.clone(),
        }
    }
}

pub fn resolve_launch(
    library: &Library,
    runtime: &RuntimeService,
    graphics: &GraphicsService,
    game_id: GameId,
) -> Result<ResolvedLaunch, AppError> {
    let game = library.get_game(game_id)?;
    let settings = library.get_settings(game_id)?;
    let app_settings = library.get_app_settings()?;

    let runner = runtime.resolve_runner(settings.runtime.runner.as_ref(), app_settings.default_runner.as_ref())?;
    let prefix = runtime.resolve_prefix(&game, &settings.runtime.prefix, &runner)?;
    let base = runtime.build_game_command(&game, &settings.runtime, &runner, &prefix)?;
    let plan = graphics.apply(&settings.graphics, base, &runner)?;

    let mut aux = Vec::with_capacity(settings.runtime.auxiliary.len());
    for program in &settings.runtime.auxiliary {
        let spec = build_aux_command(runtime, program, &settings.runtime, &runner, &prefix)?;
        aux.push((program.clone(), spec));
    }

    Ok(ResolvedLaunch {
        game,
        settings,
        app_settings,
        runner,
        prefix,
        command: plan.command,
        companions: plan.companions,
        aux,
        warnings: plan.warnings,
    })
}

fn build_aux_command(
    runtime: &RuntimeService,
    program: &AuxProgram,
    config: &RuntimeConfig,
    runner: &Runner,
    prefix: &PrefixInfo,
) -> Result<CommandSpec, AppError> {
    let mut spec = if program.windows {
        runtime.build_prefix_command(
            &program.exe_path,
            &program.args,
            program.working_dir.as_deref(),
            config,
            runner,
            prefix,
        )?
    } else {
        let mut spec = CommandSpec::new(&program.exe_path).args(program.args.clone());
        spec.cwd = program.working_dir.clone().or_else(|| program.exe_path.parent().map(Path::to_path_buf));
        spec
    };
    spec.env.extend(program.env.clone());
    Ok(spec)
}

/// Persist playtime for a finished session. Sessions that failed before
/// running for a full second are ignored by the library.
pub fn record_session_end(library: &Library, info: &SessionInfo) -> Result<(), AppError> {
    let ended_at = info.ended_at.unwrap_or_else(vnl_core::now_secs);
    library.record_play_session(info.game_id, info.started_at, ended_at)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::time::Duration;

    use vnl_core::events::RecordingSink;
    use vnl_core::{AppEvent, AppPaths, RunnerRef, SessionState};
    use vnl_library::ImportRequest;

    use super::*;

    struct Env {
        _tmp: tempfile::TempDir,
        library: Library,
        runtime: RuntimeService,
        graphics: GraphicsService,
        sink: RecordingSink,
        game: Game,
    }

    /// A "game" that is a native shell script in a folder with spaces and
    /// Japanese characters, launched through the native runner.
    fn setup(script_body: &str) -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(tmp.path());
        paths.ensure().unwrap();
        let game_dir = tmp.path().join("My Games/ゲーム テスト");
        std::fs::create_dir_all(&game_dir).unwrap();
        let exe = game_dir.join("起動.sh");
        std::fs::write(&exe, format!("#!/bin/sh\n{script_body}\n")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();

        let library = Library::open(&paths).unwrap();
        let sink = RecordingSink::new();
        let runtime = RuntimeService::new(&paths, Arc::new(sink.clone())).unwrap();
        let graphics = GraphicsService::new(&paths);

        let game = library
            .import_game(ImportRequest {
                title: "テストゲーム".into(),
                exe_path: exe,
                install_dir: Some(game_dir),
                working_dir: None,
                launch_args: vec!["--flag".into(), "value with space".into()],
                vndb_id: None,
                settings: None,
            })
            .unwrap();
        let mut settings = library.get_settings(game.id).unwrap();
        settings.runtime.runner = Some(RunnerRef { id: vnl_runtime::NATIVE_RUNNER_ID.into() });
        library.save_settings(game.id, &settings).unwrap();

        Env { _tmp: tmp, library, runtime, graphics, sink, game }
    }

    #[tokio::test]
    async fn native_game_launches_and_records_playtime() {
        let env = setup(r#"echo "args:$1|$2"; echo "cwd:$(basename "$PWD")"; sleep 1.2"#);

        let resolved = resolve_launch(&env.library, &env.runtime, &env.graphics, env.game.id).unwrap();
        assert!(resolved.command.program.ends_with("起動.sh"), "{:?}", resolved.command);
        assert!(resolved.warnings.is_empty(), "{:?}", resolved.warnings);

        let session = env.runtime.start_session(resolved.to_request()).await.unwrap();
        let finished = env.runtime.wait_session(&session.id, Duration::from_secs(20)).await.expect("session finished");
        assert_eq!(finished.state, SessionState::Exited);
        assert_eq!(finished.exit_code, Some(0));

        record_session_end(&env.library, &finished).unwrap();
        let sessions = env.library.play_sessions(env.game.id).unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].duration_seconds >= 1);
        let game = env.library.get_game(env.game.id).unwrap();
        assert!(game.playtime_seconds >= 1);
        assert!(game.last_played_at.is_some());

        let logs: Vec<String> = env
            .sink
            .events()
            .into_iter()
            .filter_map(|e| match e {
                AppEvent::Log(l) if l.session_id.as_deref() == Some(session.id.as_str()) => Some(l.message),
                _ => None,
            })
            .collect();
        assert!(logs.iter().any(|m| m.contains("args:--flag|value with space")), "{logs:?}");
        assert!(logs.iter().any(|m| m.contains("cwd:ゲーム テスト")), "{logs:?}");
    }

    #[tokio::test]
    async fn stop_terminates_running_game() {
        let env = setup("sleep 60");
        let resolved = resolve_launch(&env.library, &env.runtime, &env.graphics, env.game.id).unwrap();
        let session = env.runtime.start_session(resolved.to_request()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        env.runtime.stop_session(&session.id, false).await.unwrap();
        let finished = env.runtime.wait_session(&session.id, Duration::from_secs(20)).await.expect("stopped");
        assert!(finished.state.is_finished());
    }

    #[test]
    fn gamescope_settings_wrap_the_runtime_command() {
        if which_gamescope().is_none() {
            eprintln!("gamescope not installed; skipping");
            return;
        }
        let env = setup("true");
        let mut settings = env.library.get_settings(env.game.id).unwrap();
        settings.graphics.gamescope.enabled = true;
        settings.graphics.gamescope.game_width = Some(1280);
        settings.graphics.gamescope.game_height = Some(720);
        settings.graphics.gamescope.output_width = Some(1920);
        settings.graphics.gamescope.output_height = Some(1080);
        settings.graphics.gamescope.scaler = vnl_core::GamescopeScaler::Integer;
        settings.graphics.gamescope.filter = vnl_core::GamescopeFilter::Fsr;
        env.library.save_settings(env.game.id, &settings).unwrap();

        let resolved = resolve_launch(&env.library, &env.runtime, &env.graphics, env.game.id).unwrap();
        let cmd = &resolved.command;
        assert!(cmd.program.ends_with("gamescope"), "{cmd:?}");
        let sep = cmd.args.iter().position(|a| a == "--").expect("-- separator");
        let gs = &cmd.args[..sep];
        for pair in [["-w", "1280"], ["-h", "720"], ["-W", "1920"], ["-H", "1080"], ["-S", "integer"], ["-F", "fsr"]] {
            assert!(gs.windows(2).any(|w| w == pair), "missing {pair:?} in {gs:?}");
        }
        assert!(cmd.args[sep + 1].ends_with("起動.sh"));
        assert_eq!(&cmd.args[sep + 2..], ["--flag", "value with space"]);
    }

    fn which_gamescope() -> Option<std::path::PathBuf> {
        std::env::var_os("PATH")?
            .to_str()?
            .split(':')
            .map(|d| Path::new(d).join("gamescope"))
            .find(|p| p.is_file())
    }
}
