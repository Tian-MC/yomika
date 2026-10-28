# Architecture contracts & file ownership

## Layout

```
crates/vnl-core       shared types, AppPaths, AppEvent/EventSink, CommandSpec   (orchestrator)
crates/vnl-runtime    runners, prefixes, command building, process sessions      (agent 1)
crates/vnl-library    SQLite, games, settings, playtime, VNDB, saves             (agent 2)
crates/vnl-translate  text sources, hook/OCR adapters, providers, pipeline       (agent 4)
crates/vnl-graphics   gamescope, Wine FSR, upscaler, display + dependency checks (agent 5)
src-tauri/            Tauri shell; commands/<area>.rs per agent; session.rs glue  (orchestrator)
src/contract/*.ts     TS mirror of Rust types + typed command map                 (per area, see headers)
src/ (rest)           React UI                                                    (agent 3)
.github/ packaging/ docs/ scripts/ lint configs                                  (agent 6)
```

## Rules

1. Subsystem crates depend on `vnl-core` only — never on each other.
   Cross-subsystem wiring lives in `src-tauri/src/commands/session.rs`.
2. Public service methods return `Result<T, vnl_core::AppError>`.
3. Subsystems never depend on Tauri; they emit `AppEvent`s via `Arc<dyn EventSink>`.
4. Processes are built as `CommandSpec` (argv + env + cwd). Never use a shell.
5. Every settings struct is `#[serde(default)]`; Rust and TS types change together.
6. Tauri command args are camelCase in JS, snake_case in Rust (Tauri converts).
7. Errors: commands reject with `AppError { code, message, details }`.
8. Tests use `AppPaths::under(tempdir)` or `VNL_HOME`; never the real user dirs.
9. Never delete/reset an existing Wine prefix, never overwrite saves without a
   backup and explicit `confirm: true`.
10. No dialogue text leaves the machine unless the user configured an online provider.

## Launch pipeline (session_launch)

```
library.get_game/get_settings/get_app_settings
  -> runtime.resolve_runner -> runtime.resolve_prefix -> runtime.build_game_command
  -> graphics.apply (gamescope wrap, WINE_FULLSCREEN_FSR env, upscaler companions)
  -> runtime.build_prefix_command for each Windows AuxProgram (PROTON_VERB=runinprefix)
  -> runtime.start_session  ==> AppEvent::Session ... Exited
  -> [optional] translation.start_session(ctx with PrefixCommandBuilder)
on Exited/Failed: library.record_play_session + translation.stop_session
```

## Tauri events

`vnl://session` SessionInfo · `vnl://log` LogEntry · `vnl://translation` TranslationLine · `vnl://task` TaskProgress
