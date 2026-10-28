// Runner / prefix / process contract. Owned by Agent 1 (runtime).
// Rust side: crates/vnl-runtime + src-tauri/src/commands/runtime.rs
import type { PrefixArch, PrefixChoice, PrefixInfo, Runner } from "./core";

export interface CreatePrefixRequest {
  name: string;
  arch: PrefixArch;
  runnerId: string;
}

export interface RegisterPrefixRequest {
  name: string;
  path: string;
}

export type PrefixTool = "winecfg" | "regedit" | "explorer" | "winetricks" | "killAll";

export interface PrefixToolRequest {
  prefixId: string;
  runnerId: string;
  tool: PrefixTool;
  /** winetricks verbs, e.g. ["cjkfonts"]. */
  verbs?: string[];
}

export interface InstallerRequest {
  installerPath: string;
  runnerId: string;
  prefix: PrefixChoice;
  /** Used to name a new dedicated prefix. */
  nameHint: string;
}

export interface InstallerRun {
  /** Progress is reported via `vnl://task` events with this id. */
  taskId: string;
  prefix: PrefixInfo;
  /** `drive_c` of the prefix; scan it with library_scan_path after completion. */
  driveC: string;
}

export interface RuntimeCommands {
  runtime_list_runners: { args: Record<string, never>; result: Runner[] };
  runtime_refresh_runners: { args: Record<string, never>; result: Runner[] };
  runtime_add_custom_runner: { args: { path: string; name?: string | null }; result: Runner };
  runtime_list_prefixes: { args: Record<string, never>; result: PrefixInfo[] };
  runtime_create_prefix: { args: { request: CreatePrefixRequest }; result: PrefixInfo };
  /** Register an existing prefix. Never modified destructively. */
  runtime_register_prefix: { args: { request: RegisterPrefixRequest }; result: PrefixInfo };
  runtime_prefix_tool: { args: { request: PrefixToolRequest }; result: null };
  runtime_run_installer: { args: { request: InstallerRequest }; result: InstallerRun };
}
