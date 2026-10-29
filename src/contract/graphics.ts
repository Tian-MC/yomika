// Graphics / upscaling / dependencies / diagnostics contract. Owned by Agent 5.
// Rust side: crates/vnl-graphics + src-tauri/src/commands/graphics.rs
import type { GraphicsConfig, TaskState } from "./core";

export type DisplaySessionType = "wayland" | "x11" | "unknown";

export interface ToolInfo {
  path: string;
  version: string | null;
}

export interface MonitorInfo {
  /** Connector, e.g. "DP-1". */
  name: string;
  width: number | null;
  height: number | null;
  /** Computed from the EDID preferred timing; null when unknown. */
  refreshHz: number | null;
  /** Only true when exactly one monitor is connected (sysfs has no primary flag). */
  primary: boolean;
  /** Monitor model from EDID, e.g. "Odyssey G40B". */
  model?: string | null;
  /** DRM card, e.g. "card1". */
  card?: string | null;
}

export interface GpuInfo {
  card: string;
  pciSlot: string | null;
  vendorId: string | null;
  deviceId: string | null;
  driver: string | null;
  name: string | null;
}

export interface GraphicsCapabilities {
  sessionType: DisplaySessionType;
  xwayland: boolean;
  desktop: string | null;
  gamescope: ToolInfo | null;
  rtUpscaler: ToolInfo | null;
  monitors: MonitorInfo[];
  gpus: string[];
  /** Human-readable notes about limitations (e.g. HDR needs Wayland + gamescope WSI). */
  notes: string[];
  /** Structured GPU data; `vendorId:deviceId` works with gamescope `--prefer-vk-device`. */
  gpuDetails?: GpuInfo[];
  /** VkLayer_FROG_gamescope_wsi manifest found (needed for HDR inside gamescope). */
  gamescopeWsiLayer?: boolean;
  insideFlatpak?: boolean;
}

export interface GraphicsPreview {
  /** gamescope program + args up to and including "--" (empty when gamescope disabled). */
  wrapperArgs: string[];
  /** Env vars added by graphics features (e.g. WINE_FULLSCREEN_FSR). */
  env: Record<string, string>;
  warnings: string[];
  /** Shell-quoted preview of the full command, using a placeholder game. */
  commandPreview?: string;
  /** Shell-quoted previews of companion processes (RT-Upscaler). */
  companions?: string[];
}

export interface UpscalerModel {
  id: string;
  name: string;
  path: string | null;
  scale: number | null;
}

export interface UpscalerInfo {
  available: boolean;
  tool: ToolInfo | null;
  models: UpscalerModel[];
  presets: string[];
  /** Known limitations of the backend in the current environment. */
  limitations: string[];
  /** Backend id, "rt-upscaler". */
  backend?: string;
  /** Where models were discovered (inside the installed Python package). */
  modelDir?: string | null;
  /** RT-Upscaler config.yaml (presets = its `profiles:`). */
  configPath?: string | null;
}

export type DependencyCategory = "runtime" | "graphics" | "translation" | "ocr" | "packaging" | "other";

export interface InstallHint {
  /** "arch", "debian", "fedora", "flatpak", "generic". */
  distro: string;
  command: string;
}

export interface DependencyStatus {
  id: string;
  name: string;
  category: DependencyCategory;
  /** Required for the core launcher (vs. optional feature). */
  required: boolean;
  found: boolean;
  version: string | null;
  path: string | null;
  purpose: string;
  /** Data only: the UI shows/copies these; the app never runs them. */
  installHints: InstallHint[];
  /** Extra detection details (portal backends, matching fonts, Proton builds...). */
  details?: string | null;
}

export interface SystemReport {
  os: string | null;
  kernel: string | null;
  desktop: string | null;
  sessionType: DisplaySessionType;
  insideFlatpak: boolean;
  cpu: string | null;
  memoryMb: number | null;
  gpus: string[];
  vulkan: string | null;
  /** os-release ID / ID_LIKE, to pick install hints. */
  osId?: string | null;
  osIdLike?: string | null;
}

export interface PrefixComponent {
  id: string;
  name: string;
  /** e.g. "fonts", "codecs", "runtime". */
  category: string;
  winetricksVerbs: string[];
  /** null when the prefix is not initialised yet. */
  installed: boolean | null;
  description: string;
  /** What the detection was based on ("winetricks.log (cjkfonts)", "font msgothic.ttc"...). */
  detectedBy?: string | null;
}

export type GamescopeSource = "game" | "custom" | "system" | "managed";

export interface GamescopeBinary {
  path: string;
  version: string | null;
  source: GamescopeSource;
  /** Values accepted by -F, from `--help`; empty when unknown. */
  filters: string[];
  supportsAnime4k: boolean;
}

export interface BuildRequirement {
  id: string;
  name: string;
  kind: "tool" | "library";
  found: boolean;
  detail: string | null;
}

export interface BuildRequirements {
  items: BuildRequirement[];
  missing: string[];
  /** Package-manager commands (shown, never run by the app). */
  hints: InstallHint[];
  ok: boolean;
}

export interface GamescopeManifest {
  repo: string;
  branch: string;
  commit: string;
  builtAt: number;
  version: string | null;
  filters: string[];
}

export interface ManagedGamescope {
  installed: boolean;
  dir: string;
  binary: GamescopeBinary | null;
  manifest: GamescopeManifest | null;
}

export interface GamescopeBuildStatus {
  running: boolean;
  taskId: string | null;
  state: TaskState | null;
  message: string | null;
  startedAt: number | null;
  finishedAt: number | null;
  logPath: string | null;
}

export interface GamescopeStatus {
  /** Every usable binary: application custom path, system, managed build. */
  binaries: GamescopeBinary[];
  anime4kAvailable: boolean;
  managed: ManagedGamescope;
  requirements: BuildRequirements;
  build: GamescopeBuildStatus;
  repoUrl: string;
  branch: string;
  customPath: string | null;
}

/** Task id of the gamescope build in `vnl://task` events; its output is logged with source "gamescope-build". */
export const GAMESCOPE_BUILD_TASK = "gamescope-build";
export const GAMESCOPE_BUILD_LOG_SOURCE = "gamescope-build";

export interface GraphicsCommands {
  graphics_capabilities: { args: Record<string, never>; result: GraphicsCapabilities };
  graphics_preview: { args: { config: GraphicsConfig; runnerId?: string | null }; result: GraphicsPreview };
  graphics_upscaler_info: { args: Record<string, never>; result: UpscalerInfo };
  gamescope_status: { args: Record<string, never>; result: GamescopeStatus };
  /** Validate a custom gamescope executable (absolute path). */
  gamescope_inspect_path: { args: { path: string }; result: GamescopeBinary };
  /** Optional, user-initiated build of the gamescope-anime4k fork. Returns the task id. */
  gamescope_build_start: { args: Record<string, never>; result: string };
  gamescope_build_cancel: { args: Record<string, never>; result: boolean };
  /** Deletes the managed build only (private directory). */
  gamescope_managed_remove: { args: Record<string, never>; result: null };
  diagnostics_dependencies: { args: Record<string, never>; result: DependencyStatus[] };
  diagnostics_system: { args: Record<string, never>; result: SystemReport };
  /** Fonts/codecs useful for Japanese VNs and whether they appear installed in the prefix. */
  diagnostics_prefix_components: { args: { prefixPath: string }; result: PrefixComponent[] };
}
