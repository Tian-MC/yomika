// Graphics / upscaling / dependencies / diagnostics contract. Owned by Agent 5.
// Rust side: crates/vnl-graphics + src-tauri/src/commands/graphics.rs
import type { GraphicsConfig } from "./core";

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
  refreshHz: number | null;
  primary: boolean;
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
}

export interface GraphicsPreview {
  /** argv prefix that will wrap the game command (empty when gamescope disabled). */
  wrapperArgs: string[];
  env: Record<string, string>;
  warnings: string[];
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
  installHints: InstallHint[];
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
}

export interface PrefixComponent {
  id: string;
  name: string;
  /** e.g. "fonts", "codecs", "runtime". */
  category: string;
  winetricksVerbs: string[];
  installed: boolean | null;
  description: string;
}

export interface GraphicsCommands {
  graphics_capabilities: { args: Record<string, never>; result: GraphicsCapabilities };
  graphics_preview: { args: { config: GraphicsConfig; runnerId?: string | null }; result: GraphicsPreview };
  graphics_upscaler_info: { args: Record<string, never>; result: UpscalerInfo };
  diagnostics_dependencies: { args: Record<string, never>; result: DependencyStatus[] };
  diagnostics_system: { args: Record<string, never>; result: SystemReport };
  /** Fonts/codecs useful for Japanese VNs and whether they appear installed in the prefix. */
  diagnostics_prefix_components: { args: { prefixPath: string }; result: PrefixComponent[] };
}
