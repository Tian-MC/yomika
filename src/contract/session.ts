// Session orchestration + app-level commands. Owned by the orchestrator.
// Rust side: src-tauri/src/commands/session.rs
import type { CommandSpec, GameId, LogEntry, PrefixInfo, Runner, SessionId, SessionInfo } from "./core";

export interface LaunchPreview {
  runner: Runner;
  prefix: PrefixInfo;
  /** Final command after runtime + gamescope composition. */
  command: CommandSpec;
  preview: string;
  /** Native helper processes (e.g. upscaler) started alongside. */
  companions: CommandSpec[];
  warnings: string[];
}

export interface AppInfo {
  version: string;
  dataDir: string;
  configDir: string;
  cacheDir: string;
  dbPath: string;
}

export interface SessionCommands {
  session_launch: { args: { gameId: GameId }; result: SessionInfo };
  session_stop: { args: { sessionId: SessionId; force: boolean }; result: null };
  session_list: { args: Record<string, never>; result: SessionInfo[] };
  session_start_aux: { args: { sessionId: SessionId; auxId: string }; result: SessionInfo };
  /** Dry-run: resolve runner/prefix/graphics without launching. */
  session_preview: { args: { gameId: GameId }; result: LaunchPreview };
  logs_recent: { args: { limit?: number; sessionId?: SessionId | null }; result: LogEntry[] };
  app_info: { args: Record<string, never>; result: AppInfo };
}
