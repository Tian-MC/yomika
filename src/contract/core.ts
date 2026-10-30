// Mirror of crates/vnl-core. Owned by the orchestrator: change Rust and TS together.
// Conventions: camelCase fields, Option<T> => T | null, PathBuf => string,
// timestamps are Unix seconds unless the field ends in `Ms`.

export type GameId = number;
export type SessionId = string;

export type ErrorCode =
  | "notFound"
  | "invalidInput"
  | "io"
  | "database"
  | "network"
  | "missingDependency"
  | "process"
  | "confirmationRequired"
  | "notImplemented"
  | "unsupported"
  | "internal";

/** Shape of every rejected `invoke()` promise. */
export interface AppError {
  code: ErrorCode;
  message: string;
  details: string | null;
}

// ------------------------------------------------------------------ game

export type PlayStatus = "unplayed" | "playing" | "completed" | "onHold" | "dropped" | "planned";

export interface GameMetadata {
  vndbId: string | null;
  description: string | null;
  developers: string[];
  releaseDate: string | null;
  tags: string[];
  aliases: string[];
  rating: number | null;
  lengthMinutes: number | null;
  coverPath: string | null;
  coverUrl: string | null;
  coverSexual: number | null;
  fetchedAt: number | null;
}

export interface Game {
  id: GameId;
  title: string;
  originalTitle: string | null;
  exePath: string;
  installDir: string;
  workingDir: string | null;
  launchArgs: string[];
  status: PlayStatus;
  favorite: boolean;
  playtimeSeconds: number;
  lastPlayedAt: number | null;
  addedAt: number;
  metadata: GameMetadata;
  notes: string | null;
}

export type GameSort = "title" | "lastPlayed" | "playtime" | "addedAt" | "releaseDate";

export interface GameQuery {
  search?: string | null;
  status?: PlayStatus | null;
  favoriteOnly?: boolean;
  tag?: string | null;
  sort?: GameSort;
  descending?: boolean;
}

// --------------------------------------------------------------- runtime

export type RunnerKind = "wine" | "proton" | "native";
export type RunnerSource = "system" | "steam" | "managed" | "custom";

export interface Runner {
  id: string;
  name: string;
  kind: RunnerKind;
  source: RunnerSource;
  path: string;
  version: string | null;
}

export type PrefixArch = "win64" | "win32";

export type PrefixKind = { type: "dedicated"; gameId: GameId } | { type: "shared" } | { type: "external" };

export interface PrefixInfo {
  id: string;
  name: string;
  path: string;
  arch: PrefixArch;
  kind: PrefixKind;
  runnerId: string | null;
  createdAt: number;
  managed: boolean;
  initialized: boolean;
}

export type SessionState = "starting" | "running" | "stopping" | "exited" | "failed";

export interface AuxProcessInfo {
  auxId: string;
  name: string;
  pid: number | null;
  running: boolean;
  exitCode: number | null;
}

export interface SessionInfo {
  id: SessionId;
  gameId: GameId;
  state: SessionState;
  startedAt: number;
  endedAt: number | null;
  pid: number | null;
  exitCode: number | null;
  error: string | null;
  runnerId: string | null;
  prefixPath: string | null;
  aux: AuxProcessInfo[];
  commandPreview: string;
}

export interface CommandSpec {
  program: string;
  args: string[];
  env: Record<string, string>;
  envRemove: string[];
  cwd: string | null;
}

// -------------------------------------------------------------- settings

export interface RunnerRef {
  id: string;
}

export type PrefixChoice = { type: "dedicated" } | { type: "shared"; id: string } | { type: "custom"; path: string };

export type ProtonLauncher = "auto" | "umu" | "direct";

export interface LocaleConfig {
  japaneseLocale: boolean;
  locale: string | null;
  timezone: string | null;
}

export interface ProtonOptions {
  launcher: ProtonLauncher;
  useWined3d: boolean;
  noEsync: boolean;
  noFsync: boolean;
  enableLog: boolean;
  umuGameId: string | null;
}

export interface AuxProgram {
  id: string;
  name: string;
  exePath: string;
  args: string[];
  workingDir: string | null;
  windows: boolean;
  autostart: boolean;
  delayMs: number;
  stopWithGame: boolean;
  env: Record<string, string>;
}

export interface RuntimeConfig {
  runner: RunnerRef | null;
  prefix: PrefixChoice;
  locale: LocaleConfig;
  env: Record<string, string>;
  dllOverrides: Record<string, string>;
  proton: ProtonOptions;
  auxiliary: AuxProgram[];
  waitForWineserver: boolean;
}

export type WindowMode = "windowed" | "fullscreen" | "borderless";
export type GamescopeScaler = "auto" | "integer" | "fit" | "fill" | "stretch";
export type GamescopeFilter = "linear" | "nearest" | "fsr" | "nis" | "pixel" | "sgsr" | "anime4k";

export interface GamescopeConfig {
  enabled: boolean;
  gameWidth: number | null;
  gameHeight: number | null;
  outputWidth: number | null;
  outputHeight: number | null;
  windowMode: WindowMode;
  scaler: GamescopeScaler;
  filter: GamescopeFilter;
  sharpness: number | null;
  fpsLimit: number | null;
  fpsLimitUnfocused: number | null;
  preferOutput: string | null;
  forceGrabCursor: boolean;
  hdr: boolean;
  /** Opt-in (default false): send one Alt+Enter so a windowed game switches to its own fullscreen mode. Normally gamescope scales the window; set the game resolution instead. */
  autoFullscreen: boolean;
  /** Gamescope binary for this game; null = automatic. */
  binaryPath: string | null;
  extraArgs: string[];
}

/** Proton-GE WINE_FULLSCREEN_FSR — distinct from gamescope upscaling. */
export interface WineFsrConfig {
  enabled: boolean;
  strength: number | null;
}

export interface UpscalerConfig {
  enabled: boolean;
  backend: string | null;
  model: string | null;
  preset: string | null;
  scale: number | null;
  /** Title of the game window to attach to (RT-Upscaler -t). */
  targetTitle: string | null;
  extraArgs: string[];
}

export interface GraphicsConfig {
  gamescope: GamescopeConfig;
  wineFsr: WineFsrConfig;
  upscaler: UpscalerConfig;
}

export type ExtractionMode = "disabled" | "hook" | "ocr" | "hookAndOcr";

export interface HookThreadSelection {
  name: string;
  hookCode: string;
  context: string;
  context2: string;
}

export interface HookCodeEntry {
  code: string;
  label?: string | null;
  enabled: boolean;
}

export interface HookFilters {
  hiddenNames: string[];
  minChars: number;
  requireCjk: boolean;
}

export interface HookExtraction {
  codepage?: number | null;
  flushDelayMs?: number | null;
  mergeWindowMs: number;
  repeatGapMs: number;
}

export interface HookConfig {
  thread: HookThreadSelection | null;
  backend: string | null;
  hookCode: string | null;
  attachDelayMs: number;
  hookCodes?: HookCodeEntry[];
  threads?: HookThreadSelection[];
  preferred?: HookThreadSelection | null;
  filters?: HookFilters;
  extraction?: HookExtraction;
}

export interface CaptureRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface OcrConfig {
  backend: string | null;
  region: CaptureRegion | null;
  intervalMs: number;
}

export interface OverlayConfig {
  enabled: boolean;
  fontSize: number;
  opacity: number;
  showOriginal: boolean;
}

export interface TranslationConfig {
  enabled: boolean;
  extraction: ExtractionMode;
  sourceLanguage: string | null;
  /** No built-in default: user-controlled. */
  targetLanguage: string | null;
  providerId: string | null;
  hook: HookConfig;
  ocr: OcrConfig;
  keepHistory: boolean;
  overlay: OverlayConfig;
}

export interface GameSettings {
  runtime: RuntimeConfig;
  graphics: GraphicsConfig;
  translation: TranslationConfig;
}

export type Theme = "system" | "dark" | "light";

export interface TranslationDefaults {
  sourceLanguage: string;
  targetLanguage: string | null;
  providerId: string | null;
}

export interface VndbSettings {
  enabled: boolean;
  blurSexualThreshold: number | null;
}

export interface AppSettings {
  defaultRunner: RunnerRef | null;
  prefixesDir: string | null;
  uiLanguage: string | null;
  theme: Theme;
  translation: TranslationDefaults;
  defaultGraphics: GraphicsConfig;
  vndb: VndbSettings;
  /** Custom gamescope binary for every game; null = automatic. */
  gamescopePath: string | null;
}

// ---------------------------------------------------------------- events

export type LogLevel = "trace" | "debug" | "info" | "warn" | "error";

export interface LogEntry {
  tsMs: number;
  level: LogLevel;
  source: string;
  sessionId: SessionId | null;
  message: string;
}

export type TextOrigin = "hook" | "ocr" | "manual" | "clipboard";

export interface TranslationLine {
  id: string;
  sessionId: SessionId | null;
  gameId: GameId | null;
  origin: TextOrigin;
  sourceText: string;
  translatedText: string | null;
  sourceLanguage: string | null;
  targetLanguage: string | null;
  providerId: string | null;
  error: string | null;
  tsMs: number;
}

export type TaskState = "running" | "done" | "failed";

export interface TaskProgress {
  taskId: string;
  label: string;
  state: TaskState;
  progress: number | null;
  message: string | null;
}

/** Tauri event names and payloads emitted by the backend. */
export interface EventMap {
  "vnl://session": SessionInfo;
  "vnl://log": LogEntry;
  "vnl://translation": TranslationLine;
  "vnl://task": TaskProgress;
}
