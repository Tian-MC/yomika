// In-memory mock of the Tauri backend for the browser demo and Vitest.
// It mirrors the documented semantics of each command closely enough to drive
// every screen (including events), but never touches the real system.
import type {
  AppError,
  AppSettings,
  CommandArgs,
  CommandName,
  CommandResult,
  ErrorCode,
  Game,
  GameSettings,
  GlossaryEntry,
  GraphicsConfig,
  GraphicsPreview,
  LogEntry,
  LogLevel,
  PlaySession,
  PrefixInfo,
  ProviderInfo,
  Runner,
  SaveBackup,
  SessionInfo,
  GamescopeBinary,
  GamescopeBuildStatus,
  GamescopeStatus,
  TaskProgress,
  TranslationLine,
} from "../../contract";
import { defaultAppSettings, defaultGameSettings, newId } from "../defaults";
import {
  SAMPLE_LINES,
  seedBackups,
  seedCapabilities,
  seedDependencies,
  seedGames,
  seedGlossary,
  seedHistory,
  seedHookBackends,
  seedLanguages,
  seedOcrBackends,
  seedPlaySessions,
  seedPrefixComponents,
  seedPrefixes,
  seedProviders,
  seedRunners,
  seedSettings,
  seedSystem,
  seedUpscaler,
  vndbCatalog,
} from "./data";
import { mockEmit } from "./events";

type Handlers = {
  [K in CommandName]: (args: CommandArgs<K>) => CommandResult<K> | Promise<CommandResult<K>>;
};

const HOME = "/home/demo";
const nowS = () => Math.floor(Date.now() / 1000);

function fail(code: ErrorCode, message: string, details: string | null = null): never {
  const err: AppError = { code, message, details };
  // Mirrors Tauri: commands reject with a plain AppError object.
  // eslint-disable-next-line @typescript-eslint/only-throw-error
  throw err;
}

interface State {
  games: Game[];
  settings: Map<number, GameSettings>;
  app: AppSettings;
  runners: Runner[];
  prefixes: PrefixInfo[];
  sessions: Map<string, SessionInfo>;
  plays: PlaySession[];
  logs: LogEntry[];
  history: TranslationLine[];
  glossary: GlossaryEntry[];
  providers: ProviderInfo[];
  manualSaves: Map<number, string[]>;
  backups: SaveBackup[];
  nextGameId: number;
  nextId: number;
  gamescope: { managed: boolean; build: GamescopeBuildStatus; cancelled: boolean };
}

function createState(): State {
  const now = nowS();
  const games = seedGames(now);
  const app = defaultAppSettings();
  app.defaultRunner = { id: "system-wine" };
  app.translation.providerId = "argos";
  return {
    games,
    settings: new Map(games.map((g) => [g.id, seedSettings(g.id)])),
    app,
    runners: seedRunners(),
    prefixes: seedPrefixes(now),
    sessions: new Map(),
    plays: seedPlaySessions(now),
    logs: [],
    history: seedHistory(Date.now()),
    glossary: seedGlossary(),
    providers: seedProviders(),
    manualSaves: new Map(),
    backups: seedBackups(now),
    nextGameId: 100,
    nextId: 1000,
    gamescope: { managed: false, cancelled: false, build: { running: false, taskId: null, state: null, message: null, startedAt: null, finishedAt: null, logPath: null } },
  };
}

let state = createState();
const timers = new Set<ReturnType<typeof setTimeout>>();

function later(ms: number, fn: () => void): void {
  const t = setTimeout(() => {
    timers.delete(t);
    fn();
  }, ms);
  timers.add(t);
}

export function resetMockState(): void {
  for (const t of timers) clearTimeout(t);
  timers.clear();
  state = createState();
}

/** Test helper: inspect/modify the mock state directly. */
export function mockState(): State {
  return state;
}

function log(level: LogLevel, source: string, message: string, sessionId: string | null = null): void {
  const entry: LogEntry = { tsMs: Date.now(), level, source, sessionId, message };
  state.logs.push(entry);
  if (state.logs.length > 5000) state.logs.splice(0, state.logs.length - 5000);
  mockEmit("vnl://log", entry);
}

const MOCK_GAMESCOPE_SYSTEM: GamescopeBinary = {
  path: "/usr/bin/gamescope",
  version: "3.17.0",
  source: "system",
  filters: ["linear", "nearest", "fsr", "nis", "pixel", "sgsr"],
  supportsAnime4k: false,
};
const MOCK_GAMESCOPE_MANAGED: GamescopeBinary = {
  path: "~/.local/share/vn-launcher/tools/gamescope-anime4k/bin/gamescope",
  version: "863ff3b",
  source: "managed",
  filters: ["linear", "nearest", "fsr", "nis", "pixel", "anime4k"],
  supportsAnime4k: true,
};

function gamescopeStatus(): GamescopeStatus {
  const g = state.gamescope;
  const custom = state.app.gamescopePath;
  const binaries: GamescopeBinary[] = [];
  if (custom) binaries.push({ ...MOCK_GAMESCOPE_SYSTEM, path: custom, source: "custom", version: "custom" });
  binaries.push(MOCK_GAMESCOPE_SYSTEM);
  if (g.managed) binaries.push(MOCK_GAMESCOPE_MANAGED);
  return {
    binaries,
    anime4kAvailable: binaries.some((b) => b.supportsAnime4k),
    managed: {
      installed: g.managed,
      dir: "~/.local/share/vn-launcher/tools/gamescope-anime4k",
      binary: g.managed ? MOCK_GAMESCOPE_MANAGED : null,
      manifest: g.managed ? { repo: "https://github.com/Et3rnos/gamescope-anime4k.git", branch: "master", commit: "863ff3b892a30aefbdd16f31902472814c28134b", builtAt: nowS(), version: "863ff3b", filters: MOCK_GAMESCOPE_MANAGED.filters } : null,
    },
    requirements: {
      items: [
        { id: "git", name: "git", kind: "tool", found: true, detail: "/usr/bin/git" },
        { id: "meson", name: "meson", kind: "tool", found: true, detail: "/usr/bin/meson" },
        { id: "libdecor-0", name: "libdecor-0", kind: "library", found: true, detail: "0.2.3" },
      ],
      missing: [],
      hints: [],
      ok: true,
    },
    build: { ...g.build },
    repoUrl: "https://github.com/Et3rnos/gamescope-anime4k.git",
    branch: "master",
    customPath: custom,
  };
}

function getGame(id: number): Game {
  return state.games.find((g) => g.id === id) ?? fail("notFound", `Game ${id} not found`);
}

function getSettings(id: number): GameSettings {
  getGame(id);
  let s = state.settings.get(id);
  if (!s) {
    s = defaultGameSettings(state.app);
    state.settings.set(id, s);
  }
  return s;
}

function runnerFor(settings: GameSettings): Runner {
  const id = settings.runtime.runner?.id ?? state.app.defaultRunner?.id ?? state.runners[0]?.id;
  return state.runners.find((r) => r.id === id) ?? fail("notFound", `Runner ${id ?? "(none)"} not found`, "Pick a runner in Compatibility settings.");
}

function prefixFor(game: Game, settings: GameSettings): PrefixInfo {
  const choice = settings.runtime.prefix;
  if (choice.type === "shared") {
    return state.prefixes.find((p) => p.id === choice.id) ?? fail("notFound", `Shared prefix ${choice.id} not found`);
  }
  if (choice.type === "custom") {
    return { id: "custom", name: "Custom prefix", path: choice.path, arch: "win64", kind: { type: "external" }, runnerId: null, createdAt: nowS(), managed: false, initialized: true };
  }
  const existing = state.prefixes.find((p) => p.kind.type === "dedicated" && p.kind.gameId === game.id);
  if (existing) return existing;
  return {
    id: `pfx-game-${game.id}`,
    name: game.title,
    path: `${HOME}/.local/share/vn-launcher/prefixes/game-${game.id}`,
    arch: "win64",
    kind: { type: "dedicated", gameId: game.id },
    runnerId: settings.runtime.runner?.id ?? null,
    createdAt: nowS(),
    managed: true,
    initialized: false,
  };
}

export function buildGraphicsPreview(config: GraphicsConfig): GraphicsPreview {
  const gs = config.gamescope;
  const wrapperArgs: string[] = [];
  const env: Record<string, string> = {};
  const warnings: string[] = [];
  if (gs.enabled) {
    wrapperArgs.push("gamescope");
    if (gs.gameWidth) wrapperArgs.push("-w", String(gs.gameWidth));
    if (gs.gameHeight) wrapperArgs.push("-h", String(gs.gameHeight));
    if (gs.outputWidth) wrapperArgs.push("-W", String(gs.outputWidth));
    if (gs.outputHeight) wrapperArgs.push("-H", String(gs.outputHeight));
    if (gs.windowMode === "fullscreen") wrapperArgs.push("-f");
    if (gs.windowMode === "borderless") wrapperArgs.push("-b");
    if (gs.scaler !== "auto") wrapperArgs.push("-S", gs.scaler);
    if (gs.filter !== "linear") wrapperArgs.push("-F", gs.filter);
    if (gs.sharpness !== null && (gs.filter === "fsr" || gs.filter === "nis")) wrapperArgs.push("--sharpness", String(gs.sharpness));
    if (gs.sharpness !== null && gs.filter !== "fsr" && gs.filter !== "nis") warnings.push("Sharpness only applies to the FSR and NIS filters.");
    if (gs.fpsLimit) wrapperArgs.push("-r", String(gs.fpsLimit));
    if (gs.fpsLimitUnfocused) wrapperArgs.push("-o", String(gs.fpsLimitUnfocused));
    if (gs.preferOutput) wrapperArgs.push("--prefer-output", gs.preferOutput);
    if (gs.forceGrabCursor) wrapperArgs.push("--force-grab-cursor");
    if (gs.hdr) {
      wrapperArgs.push("--hdr-enabled");
      env.ENABLE_GAMESCOPE_WSI = "1";
      env.DXVK_HDR = "1";
    }
    wrapperArgs.push(...gs.extraArgs, "--");
    if ((gs.gameWidth === null) !== (gs.gameHeight === null)) warnings.push("Set both game width and height, or neither.");
    if ((gs.outputWidth === null) !== (gs.outputHeight === null)) warnings.push("Set both output width and height, or neither.");
  }
  if (config.wineFsr.enabled) {
    env.WINE_FULLSCREEN_FSR = "1";
    if (config.wineFsr.strength !== null) env.WINE_FULLSCREEN_FSR_STRENGTH = String(config.wineFsr.strength);
    warnings.push("Wine FSR only works with Proton-GE/Wine-GE builds and fullscreen games rendering below native resolution.");
    if (gs.enabled && gs.filter === "fsr") warnings.push("Both gamescope FSR and Wine FSR are enabled; this usually upscales twice.");
  }
  if (config.upscaler.enabled) warnings.push("RT-Upscaler is not installed; the upscaler will be skipped.");
  return { wrapperArgs, env, warnings };
}

function quote(arg: string): string {
  return /^[\w@%+=:,./-]+$/.test(arg) ? arg : `'${arg.replace(/'/g, `'\\''`)}'`;
}

function launchPreview(gameId: number): CommandResult<"session_preview"> {
  const game = getGame(gameId);
  const settings = getSettings(gameId);
  const runner = runnerFor(settings);
  const prefix = prefixFor(game, settings);
  const g = buildGraphicsPreview(settings.graphics);
  const env: Record<string, string> = { ...g.env };
  const warnings = [...g.warnings];
  const rt = settings.runtime;
  if (rt.locale.locale) {
    env.LANG = rt.locale.locale;
    env.LC_ALL = rt.locale.locale;
  } else if (rt.locale.japaneseLocale) {
    env.LANG = "ja_JP.UTF-8";
    env.LC_ALL = "ja_JP.UTF-8";
  }
  if (rt.locale.timezone) env.TZ = rt.locale.timezone;
  const dll = Object.entries(rt.dllOverrides).map(([k, v]) => `${k}=${v}`).join(";");
  if (dll) env.WINEDLLOVERRIDES = dll;
  let program: string;
  let args: string[];
  if (runner.kind === "proton") {
    env.WINEPREFIX = prefix.path;
    env.STEAM_COMPAT_DATA_PATH = prefix.path.replace(/\/pfx$/, "");
    if (rt.proton.useWined3d) env.PROTON_USE_WINED3D = "1";
    if (rt.proton.noEsync) env.PROTON_NO_ESYNC = "1";
    if (rt.proton.noFsync) env.PROTON_NO_FSYNC = "1";
    if (rt.proton.enableLog) env.PROTON_LOG = "1";
    if (rt.proton.launcher === "direct") {
      program = `${runner.path}/proton`;
      args = ["run", game.exePath];
    } else {
      env.GAMEID = rt.proton.umuGameId ?? "umu-default";
      env.PROTONPATH = runner.path;
      program = "umu-run";
      args = [game.exePath];
      if (rt.proton.launcher === "auto") warnings.push("umu-run not found; falling back to calling proton directly at launch.");
    }
  } else if (runner.kind === "wine") {
    env.WINEPREFIX = prefix.path;
    program = runner.path;
    args = [game.exePath];
  } else {
    program = game.exePath;
    args = [];
  }
  args.push(...game.launchArgs);
  Object.assign(env, rt.env);
  if (!prefix.initialized && runner.kind !== "native") warnings.push("The prefix will be created on first launch (this can take a minute).");
  if (g.wrapperArgs.length > 0) {
    args = [...g.wrapperArgs.slice(1), program, ...args];
    program = g.wrapperArgs[0]!;
  }
  const command = { program, args, env, envRemove: [], cwd: game.workingDir ?? game.installDir };
  const preview = [
    `cd ${quote(command.cwd)} &&`,
    ...Object.entries(env).map(([k, v]) => `${k}=${quote(v)}`),
    quote(program),
    ...args.map(quote),
  ].join(" ");
  const companions = settings.graphics.upscaler.enabled
    ? [{ program: "rt-upscaler", args: ["--follow-window", game.title], env: {}, envRemove: [], cwd: null }]
    : [];
  return { runner, prefix, command, preview, companions, warnings };
}

function emitSession(s: SessionInfo): void {
  state.sessions.set(s.id, s);
  mockEmit("vnl://session", s);
}

function emitTask(t: TaskProgress): void {
  mockEmit("vnl://task", t);
}

function endSession(id: string, exitCode: number): void {
  const s = state.sessions.get(id);
  if (!s || s.state === "exited" || s.state === "failed") return;
  const endedAt = nowS();
  const duration = Math.max(1, endedAt - s.startedAt);
  const ended: SessionInfo = { ...s, state: "exited", endedAt, exitCode, pid: null, aux: s.aux.map((a) => ({ ...a, running: false, pid: null })) };
  const game = state.games.find((g) => g.id === s.gameId);
  if (game) {
    game.playtimeSeconds += duration;
    game.lastPlayedAt = endedAt;
    if (game.status === "unplayed") game.status = "playing";
  }
  state.plays.push({ id: state.nextId++, gameId: s.gameId, startedAt: s.startedAt, endedAt, durationSeconds: duration });
  log("info", "session", `Game exited with code ${exitCode} after ${duration}s`, id);
  emitSession(ended);
}

function translateText(text: string, target: string): string {
  const known = SAMPLE_LINES.find(([src]) => src === text.trim());
  if (known && target === "en") return known[1];
  return `[${target}] ${known ? known[1] : text}`;
}

function scheduleTranslationLines(sessionId: string, gameId: number, i = 0): void {
  later(4000, () => {
    const s = state.sessions.get(sessionId);
    if (!s || s.state !== "running") return;
    const settings = getSettings(gameId);
    const target = settings.translation.targetLanguage ?? state.app.translation.targetLanguage;
    const [src] = SAMPLE_LINES[i % SAMPLE_LINES.length]!;
    const line: TranslationLine = {
      id: newId(),
      sessionId,
      gameId,
      origin: settings.translation.extraction === "ocr" ? "ocr" : "hook",
      sourceText: src,
      translatedText: target ? translateText(src, target) : null,
      sourceLanguage: settings.translation.sourceLanguage ?? state.app.translation.sourceLanguage,
      targetLanguage: target,
      providerId: settings.translation.providerId ?? state.app.translation.providerId,
      error: target ? null : "No target language selected",
      tsMs: Date.now(),
    };
    if (settings.translation.keepHistory) state.history.unshift(line);
    mockEmit("vnl://translation", line);
    scheduleTranslationLines(sessionId, gameId, i + 1);
  });
}

function makeLine(text: string, partial: Partial<TranslationLine>): TranslationLine {
  return {
    id: newId(),
    sessionId: null,
    gameId: null,
    origin: "manual",
    sourceText: text,
    translatedText: null,
    sourceLanguage: null,
    targetLanguage: null,
    providerId: null,
    error: null,
    tsMs: Date.now(),
    ...partial,
  };
}

function basename(p: string): string {
  const parts = p.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] ?? p;
}

function dirname(p: string): string {
  const i = p.replace(/\/+$/, "").lastIndexOf("/");
  return i > 0 ? p.slice(0, i) : "/";
}

function prettyTitle(name: string): string {
  return name
    .replace(/\.(exe|msi)$/i, "")
    .replace(/[_-]+/g, " ")
    .replace(/\b(setup|install(er)?)\b/gi, "")
    .trim()
    .replace(/\s+/g, " ") || "New game";
}

function createBackup(gameId: number, label: string | null, kind: SaveBackup["kind"]): SaveBackup {
  const game = getGame(gameId);
  const id = state.nextId++;
  const b: SaveBackup = {
    id,
    gameId,
    createdAt: nowS(),
    label: label?.trim() || null,
    kind,
    sizeBytes: 1_500_000 + Math.floor(Math.random() * 500_000),
    fileCount: 40 + Math.floor(Math.random() * 10),
    archivePath: `${HOME}/.local/share/vn-launcher/backups/${gameId}/backup-${id}.tar.zst`,
    sourcePaths: [`${game.installDir}/savedata`, ...(state.manualSaves.get(gameId) ?? [])],
  };
  state.backups.push(b);
  return b;
}

const handlers: Handlers = {
  // ------------------------------------------------------------- library
  library_list_games: ({ query }) => {
    let list = [...state.games];
    const q = query.search?.trim().toLowerCase();
    if (q) list = list.filter((g) => [g.title, g.originalTitle ?? "", ...g.metadata.aliases].some((s) => s.toLowerCase().includes(q)));
    if (query.status) list = list.filter((g) => g.status === query.status);
    if (query.favoriteOnly) list = list.filter((g) => g.favorite);
    if (query.tag) list = list.filter((g) => g.metadata.tags.includes(query.tag!));
    const sort = query.sort ?? "title";
    list.sort((a, b) => {
      switch (sort) {
        case "lastPlayed":
          return (a.lastPlayedAt ?? 0) - (b.lastPlayedAt ?? 0);
        case "playtime":
          return a.playtimeSeconds - b.playtimeSeconds;
        case "addedAt":
          return a.addedAt - b.addedAt;
        case "releaseDate":
          return (a.metadata.releaseDate ?? "").localeCompare(b.metadata.releaseDate ?? "");
        default:
          return a.title.localeCompare(b.title);
      }
    });
    if (query.descending) list.reverse();
    return list;
  },
  library_get_game: ({ id }) => getGame(id),
  library_scan_path: ({ path }) => {
    const trimmed = path.trim();
    if (!trimmed.startsWith("/")) fail("invalidInput", "Path must be absolute", trimmed);
    const isExe = /\.(exe|msi|bat|lnk)$/i.test(trimmed);
    const root = isExe ? dirname(trimmed) : trimmed.replace(/\/+$/, "");
    const name = basename(isExe ? trimmed : root);
    const installer = /(setup|install).*\.(exe|msi)$|\.msi$/i.test(name);
    if (installer) {
      return {
        root,
        candidates: [{ path: trimmed, relativePath: name, sizeBytes: 1_480_000_000, score: 10, hints: ["installer"], isInstaller: true }],
        suggestedTitle: prettyTitle(basename(root)),
        isInstaller: true,
        existingGameId: null,
      };
    }
    const folderTitle = /drive_c/.test(root) ? "Installed game" : prettyTitle(basename(root));
    const main = isExe ? name : `${basename(root).replace(/\s+/g, "")}.exe`;
    const gameDir = /drive_c$/.test(root) ? `${root}/Program Files (x86)/Game` : root;
    const rel = (f: string) => (gameDir === root ? f : `${gameDir.slice(root.length + 1)}/${f}`);
    const candidates = [
      { path: `${gameDir}/${main}`, relativePath: rel(main), sizeBytes: 6_412_288, score: 92, hints: ["largest", "matchesFolderName", "hasGameData"], isInstaller: false },
      { path: `${gameDir}/config.exe`, relativePath: rel("config.exe"), sizeBytes: 812_032, score: 31, hints: ["configTool"], isInstaller: false },
      { path: `${gameDir}/uninst.exe`, relativePath: rel("uninst.exe"), sizeBytes: 120_320, score: 4, hints: ["uninstaller"], isInstaller: false },
    ];
    const existing = state.games.find((g) => candidates.some((c) => c.path === g.exePath));
    return { root, candidates, suggestedTitle: folderTitle, isInstaller: false, existingGameId: existing?.id ?? null };
  },
  library_import_game: ({ request }) => {
    if (!request.title.trim()) fail("invalidInput", "Title is required");
    if (!request.exePath.trim()) fail("invalidInput", "Executable path is required");
    if (state.games.some((g) => g.exePath === request.exePath)) fail("invalidInput", "This executable is already in the library");
    const id = state.nextGameId++;
    const game: Game = {
      id,
      title: request.title.trim(),
      originalTitle: null,
      exePath: request.exePath,
      installDir: request.installDir ?? dirname(request.exePath),
      workingDir: request.workingDir ?? null,
      launchArgs: request.launchArgs ?? [],
      status: "unplayed",
      favorite: false,
      playtimeSeconds: 0,
      lastPlayedAt: null,
      addedAt: nowS(),
      metadata: {
        vndbId: request.vndbId ?? null,
        description: null,
        developers: [],
        releaseDate: null,
        tags: [],
        aliases: [],
        rating: null,
        lengthMinutes: null,
        coverPath: null,
        coverUrl: null,
        coverSexual: null,
        fetchedAt: null,
      },
      notes: null,
    };
    state.games.push(game);
    state.settings.set(id, request.settings ? structuredClone(request.settings) : defaultGameSettings(state.app));
    log("info", "library", `Imported "${game.title}" (${game.exePath})`);
    return game;
  },
  library_update_game: ({ game }) => {
    const idx = state.games.findIndex((g) => g.id === game.id);
    if (idx < 0) fail("notFound", `Game ${game.id} not found`);
    if (!game.title.trim()) fail("invalidInput", "Title cannot be empty");
    const prev = state.games[idx]!;
    const next: Game = { ...game, playtimeSeconds: prev.playtimeSeconds, lastPlayedAt: prev.lastPlayedAt, addedAt: prev.addedAt };
    state.games[idx] = next;
    return next;
  },
  library_remove_game: ({ id }) => {
    getGame(id);
    state.games = state.games.filter((g) => g.id !== id);
    state.settings.delete(id);
    return null;
  },
  library_get_settings: ({ gameId }) => getSettings(gameId),
  library_save_settings: ({ gameId, settings }) => {
    getGame(gameId);
    for (const aux of settings.runtime.auxiliary) {
      if (!aux.name.trim() || !aux.exePath.trim()) fail("invalidInput", "Auxiliary programs need a name and an executable");
    }
    state.settings.set(gameId, structuredClone(settings));
    return settings;
  },
  library_play_sessions: ({ gameId }) => state.plays.filter((p) => p.gameId === gameId).sort((a, b) => b.startedAt - a.startedAt),

  settings_get_app: () => state.app,
  settings_save_app: ({ settings }) => {
    state.app = structuredClone(settings);
    return state.app;
  },

  vndb_search: ({ query }) => {
    if (!state.app.vndb.enabled) fail("unsupported", "VNDB lookups are disabled in Settings");
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return vndbCatalog(nowS())
      .map((e) => e.result)
      .filter((r) => r.title.toLowerCase().includes(q) || (r.altTitle ?? "").toLowerCase().includes(q) || q.split(/\s+/).some((w) => w.length > 2 && r.title.toLowerCase().includes(w)));
  },
  vndb_apply: ({ gameId, vndbId }) => {
    const game = getGame(gameId);
    const entry = vndbCatalog(nowS()).find((e) => e.result.id === vndbId) ?? fail("notFound", `VNDB entry ${vndbId} not found`);
    game.metadata = { ...entry.metadata, fetchedAt: nowS() };
    game.originalTitle = entry.result.altTitle;
    return game;
  },

  saves_detect: ({ gameId }) => {
    const game = getGame(gameId);
    const settings = getSettings(gameId);
    const prefix = prefixFor(game, settings);
    const detected = [
      { path: `${game.installDir}/savedata`, source: "detected" as const, exists: true, reason: "Folder named 'savedata' in install dir" },
      { path: `${prefix.path}/drive_c/users/steamuser/AppData/Roaming/${game.title.replace(/[^\w]+/g, "")}`, source: "detected" as const, exists: false, reason: "AppData/Roaming match" },
    ];
    const manual = (state.manualSaves.get(gameId) ?? []).map((path) => ({ path, source: "manual" as const, exists: true, reason: null }));
    return [...detected, ...manual];
  },
  saves_set_locations: ({ gameId, paths }) => {
    state.manualSaves.set(gameId, [...paths]);
    return handlers.saves_detect({ gameId });
  },
  saves_list_backups: ({ gameId }) => state.backups.filter((b) => b.gameId === gameId).sort((a, b) => b.createdAt - a.createdAt),
  saves_create_backup: ({ gameId, label }) => createBackup(gameId, label ?? null, "manual"),
  saves_restore_backup: ({ backupId, confirm }) => {
    const b = state.backups.find((x) => x.id === backupId) ?? fail("notFound", `Backup ${backupId} not found`);
    if (confirm !== true) fail("confirmationRequired", "Restoring overwrites current saves; confirm to continue");
    const safety = createBackup(b.gameId, `Safety backup before restoring #${b.id}`, "safety");
    return { restored: b, safetyBackup: safety, filesRestored: b.fileCount };
  },


  // ------------------------------------------------------------- runtime
  runtime_list_runners: () => state.runners,
  runtime_refresh_runners: () => {
    log("info", "runtime", `Discovered ${state.runners.length} runners`);
    return state.runners;
  },
  runtime_add_custom_runner: ({ path, name }) => {
    if (!path.startsWith("/")) fail("invalidInput", "Runner path must be absolute");
    const kind = /proton/i.test(path) ? "proton" : "wine";
    const r: Runner = { id: `custom-${state.nextId++}`, name: name?.trim() || basename(path), kind, source: "custom", path, version: null };
    state.runners.push(r);
    return r;
  },
  runtime_list_prefixes: () => state.prefixes,
  runtime_create_prefix: ({ request }) => {
    if (!request.name.trim()) fail("invalidInput", "Prefix name is required");
    const slug = request.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-");
    const p: PrefixInfo = {
      id: `pfx-${slug}-${state.nextId++}`,
      name: request.name.trim(),
      path: `${state.app.prefixesDir ?? `${HOME}/.local/share/vn-launcher/prefixes`}/${slug}`,
      arch: request.arch,
      kind: { type: "shared" },
      runnerId: request.runnerId,
      createdAt: nowS(),
      managed: true,
      initialized: true,
    };
    state.prefixes.push(p);
    log("info", "runtime", `Created prefix ${p.path} (${p.arch})`);
    return p;
  },
  runtime_register_prefix: ({ request }) => {
    if (!request.path.startsWith("/")) fail("invalidInput", "Prefix path must be absolute");
    if (state.prefixes.some((p) => p.path === request.path)) fail("invalidInput", "This prefix is already registered");
    const p: PrefixInfo = {
      id: `pfx-ext-${state.nextId++}`,
      name: request.name.trim() || basename(request.path),
      path: request.path,
      arch: "win64",
      kind: { type: "external" },
      runnerId: null,
      createdAt: nowS(),
      managed: false,
      initialized: true,
    };
    state.prefixes.push(p);
    return p;
  },
  runtime_prefix_tool: ({ request }) => {
    const p = state.prefixes.find((x) => x.id === request.prefixId) ?? fail("notFound", `Prefix ${request.prefixId} not found`);
    if (request.tool === "winetricks" && !(request.verbs ?? []).length) fail("invalidInput", "Pick at least one winetricks verb");
    log("info", "runtime", `Running ${request.tool}${request.verbs?.length ? ` ${request.verbs.join(" ")}` : ""} in ${p.path} with ${request.runnerId}`);
    if (request.tool === "winetricks") {
      const taskId = newId();
      const label = `winetricks ${(request.verbs ?? []).join(" ")}`;
      emitTask({ taskId, label, state: "running", progress: null, message: "Downloading…" });
      later(2500, () => emitTask({ taskId, label, state: "done", progress: 1, message: "Finished" }));
    }
    return null;
  },
  runtime_run_installer: ({ request }) => {
    const runner = state.runners.find((r) => r.id === request.runnerId) ?? fail("notFound", `Runner ${request.runnerId} not found`);
    const taskId = newId();
    const slug = request.nameHint.toLowerCase().replace(/[^a-z0-9]+/g, "-") || "installer";
    let prefix: PrefixInfo;
    if (request.prefix.type === "shared") {
      const id = request.prefix.id;
      prefix = state.prefixes.find((p) => p.id === id) ?? fail("notFound", `Prefix ${id} not found`);
    } else if (request.prefix.type === "custom") {
      prefix = { id: `pfx-ext-${state.nextId++}`, name: request.nameHint, path: request.prefix.path, arch: "win64", kind: { type: "external" }, runnerId: runner.id, createdAt: nowS(), managed: false, initialized: true };
      state.prefixes.push(prefix);
    } else {
      prefix = {
        id: `pfx-${slug}-${state.nextId++}`,
        name: request.nameHint,
        path: `${HOME}/.local/share/vn-launcher/prefixes/${slug}`,
        arch: "win64",
        kind: { type: "shared" },
        runnerId: runner.id,
        createdAt: nowS(),
        managed: true,
        initialized: true,
      };
      state.prefixes.push(prefix);
    }
    const label = `Installing ${request.nameHint}`;
    const steps = [0.1, 0.35, 0.6, 0.85];
    emitTask({ taskId, label, state: "running", progress: 0, message: "Preparing prefix…" });
    steps.forEach((p, i) => later(500 * (i + 1), () => emitTask({ taskId, label, state: "running", progress: p, message: i < 1 ? "Starting installer…" : "Waiting for the installer to finish…" })));
    later(500 * (steps.length + 1), () => emitTask({ taskId, label, state: "done", progress: 1, message: "Installer exited" }));
    return { taskId, prefix, driveC: `${prefix.path}/drive_c` };
  },

  // ------------------------------------------------------------ graphics
  graphics_capabilities: () => seedCapabilities(),
  graphics_preview: ({ config }) => buildGraphicsPreview(config),
  graphics_upscaler_info: () => seedUpscaler(),
  gamescope_status: () => gamescopeStatus(),
  gamescope_inspect_path: ({ path }) => {
    if (!path.startsWith("/")) fail("invalidInput", "Choose the gamescope executable with its full path");
    if (!path.includes("gamescope")) fail("invalidInput", `${path} is not an executable gamescope`);
    return { ...MOCK_GAMESCOPE_SYSTEM, path, source: "custom", supportsAnime4k: path.includes("anime4k"), filters: path.includes("anime4k") ? MOCK_GAMESCOPE_MANAGED.filters : MOCK_GAMESCOPE_SYSTEM.filters };
  },
  gamescope_build_start: () => {
    const g = state.gamescope;
    if (g.build.running) fail("invalidInput", "A gamescope build is already running");
    const taskId = "gamescope-build";
    const label = "Build gamescope (Anime4K)";
    g.cancelled = false;
    g.build = { running: true, taskId, state: "running", message: "Starting", startedAt: nowS(), finishedAt: null, logPath: "~/.local/share/vn-launcher/logs/gamescope-build.log" };
    emitTask({ taskId, label, state: "running", progress: 0, message: "Starting" });
    const steps: Array<[number, string]> = [[0.05, "Fetching the gamescope-anime4k sources"], [0.2, "Fetching submodules"], [0.32, "Configuring (meson setup)"], [0.5, "Compiling (150/424)"], [0.8, "Compiling (380/424)"], [0.92, "Installing into the private directory"]];
    steps.forEach(([p, message], i) =>
      later(400 * (i + 1), () => {
        if (g.cancelled) return;
        g.build.message = message;
        log("info", "gamescope-build", `$ ${message}`);
        emitTask({ taskId, label, state: "running", progress: p, message });
      }),
    );
    later(400 * (steps.length + 1), () => {
      if (g.cancelled) return;
      g.managed = true;
      g.build = { ...g.build, running: false, state: "done", message: "Installed gamescope-anime4k 863ff3b892", finishedAt: nowS() };
      log("info", "gamescope-build", "Installed gamescope-anime4k 863ff3b892");
      emitTask({ taskId, label, state: "done", progress: 1, message: g.build.message });
    });
    return taskId;
  },
  gamescope_build_cancel: () => {
    const g = state.gamescope;
    if (!g.build.running) return false;
    g.cancelled = true;
    g.build = { ...g.build, running: false, state: "failed", message: "Build cancelled", finishedAt: nowS() };
    log("error", "gamescope-build", "Build cancelled");
    emitTask({ taskId: "gamescope-build", label: "Build gamescope (Anime4K)", state: "failed", progress: null, message: "Build cancelled" });
    return true;
  },
  gamescope_managed_remove: () => {
    if (state.gamescope.build.running) fail("invalidInput", "A gamescope build is running; cancel it first");
    state.gamescope.managed = false;
    return null;
  },
  diagnostics_dependencies: () => seedDependencies(),
  diagnostics_system: () => seedSystem(),
  diagnostics_prefix_components: ({ prefixPath }) => {
    if (!prefixPath) fail("invalidInput", "Prefix path is required");
    return seedPrefixComponents();
  },

  // --------------------------------------------------------- translation
  translation_providers: () => state.providers,
  translation_configure_provider: ({ providerId, config, apiKey }) => {
    const p = state.providers.find((x) => x.id === providerId) ?? fail("notFound", `Provider ${providerId} not found`);
    for (const f of p.fields) {
      if (f.required && (config[f.key] === undefined || config[f.key] === null || config[f.key] === "")) fail("invalidInput", `${f.label} is required`);
    }
    p.config = { ...config };
    // undefined/null = keep, "" = delete, non-empty = store.
    if (apiKey === "") p.hasApiKey = false;
    else if (typeof apiKey === "string") p.hasApiKey = true;
    p.configured = !p.requiresApiKey || p.hasApiKey;
    return p;
  },
  translation_test_provider: ({ providerId, targetLanguage }) => {
    const p = state.providers.find((x) => x.id === providerId) ?? fail("notFound", `Provider ${providerId} not found`);
    if (!targetLanguage) fail("invalidInput", "Choose a target language first");
    if (p.requiresApiKey && !p.hasApiKey) fail("invalidInput", `${p.name} needs an API key`);
    const src = SAMPLE_LINES[0]![0];
    return makeLine(src, { providerId, sourceLanguage: "ja", targetLanguage, translatedText: translateText(src, targetLanguage) });
  },
  translation_translate: ({ request }) => {
    const target = request.targetLanguage ?? state.app.translation.targetLanguage;
    if (!target) fail("invalidInput", "No target language selected", "Choose a target language in the translation settings.");
    const providerId = request.providerId ?? state.app.translation.providerId;
    if (!providerId) fail("invalidInput", "No translation provider selected");
    const p = state.providers.find((x) => x.id === providerId) ?? fail("notFound", `Provider ${providerId} not found`);
    if (p.requiresApiKey && !p.hasApiKey) fail("invalidInput", `${p.name} needs an API key`);
    if (!request.text.trim()) fail("invalidInput", "Nothing to translate");
    const line = makeLine(request.text, {
      gameId: request.gameId ?? null,
      sessionId: request.sessionId ?? null,
      sourceLanguage: request.sourceLanguage ?? state.app.translation.sourceLanguage,
      targetLanguage: target,
      providerId,
      translatedText: translateText(request.text, target),
    });
    state.history.unshift(line);
    return line;
  },
  translation_languages: () => seedLanguages(),
  translation_glossary_list: ({ gameId }) => state.glossary.filter((e) => (gameId === undefined ? true : e.gameId === gameId)),
  translation_glossary_upsert: ({ entry }) => {
    if (!entry.source.trim()) fail("invalidInput", "Source text is required");
    if (entry.id > 0) {
      const idx = state.glossary.findIndex((e) => e.id === entry.id);
      if (idx < 0) fail("notFound", `Glossary entry ${entry.id} not found`);
      state.glossary[idx] = { ...entry };
      return entry;
    }
    const created = { ...entry, id: state.nextId++ };
    state.glossary.push(created);
    return created;
  },
  translation_glossary_delete: ({ id }) => {
    state.glossary = state.glossary.filter((e) => e.id !== id);
    return null;
  },
  translation_history: ({ gameId, limit }) => {
    const list = state.history.filter((l) => (gameId === undefined || gameId === null ? true : l.gameId === gameId));
    return list.slice(0, limit ?? 200);
  },
  translation_history_clear: ({ gameId }) => {
    state.history = gameId === undefined || gameId === null ? [] : state.history.filter((l) => l.gameId !== gameId);
    return null;
  },
  translation_ocr_backends: () => seedOcrBackends(),
  translation_ocr_capture: ({ request }) => {
    const target = state.app.translation.targetLanguage;
    const src = SAMPLE_LINES[1]![0];
    const translate = request.translate ?? true;
    return makeLine(src, {
      origin: "ocr",
      gameId: request.gameId ?? null,
      sessionId: request.sessionId ?? null,
      sourceLanguage: "ja",
      targetLanguage: target,
      providerId: state.app.translation.providerId,
      translatedText: translate && target ? translateText(src, target) : null,
      error: translate && !target ? "No target language selected" : null,
    });
  },
  translation_hook_backends: () => seedHookBackends(),
  translation_session_status: ({ sessionId }) => {
    const s = state.sessions.get(sessionId) ?? fail("notFound", `Session ${sessionId} not found`);
    const t = getSettings(s.gameId).translation;
    const hook = t.enabled && (t.extraction === "hook" || t.extraction === "hookAndOcr") ? "active" : "inactive";
    const ocr = t.enabled && (t.extraction === "ocr" || t.extraction === "hookAndOcr") ? "active" : "inactive";
    return { sessionId, hook, ocr, diagnostics: t.enabled ? [] : ["Translation is disabled for this game."] };
  },
  overlay_show: ({ config }) => {
    const gameCfg = config ?? defaultGameSettings(state.app).translation.overlay;
    mockEmit("vnl://overlay-config", gameCfg);
    log("info", "overlay", "Overlay window shown (demo: open #/overlay in another tab)");
    return null;
  },
  overlay_hide: () => {
    log("info", "overlay", "Overlay window hidden");
    return null;
  },

  // -------------------------------------------------------------- session
  session_launch: ({ gameId }) => {
    const game = getGame(gameId);
    const running = [...state.sessions.values()].find((s) => s.gameId === gameId && (s.state === "running" || s.state === "starting"));
    if (running) fail("invalidInput", `${game.title} is already running`);
    const preview = launchPreview(gameId);
    const settings = getSettings(gameId);
    const id = newId();
    const s: SessionInfo = {
      id,
      gameId,
      state: "starting",
      startedAt: nowS(),
      endedAt: null,
      pid: null,
      exitCode: null,
      error: null,
      runnerId: preview.runner.id,
      prefixPath: preview.prefix.path,
      aux: settings.runtime.auxiliary.map((a) => ({ auxId: a.id, name: a.name, pid: null, running: false, exitCode: null })),
      commandPreview: preview.preview,
    };
    emitSession(s);
    log("info", "session", `Launching ${game.title}`, id);
    log("debug", "runtime", preview.preview, id);
    later(700, () => {
      const cur = state.sessions.get(id);
      if (!cur || cur.state !== "starting") return;
      const pid = 40000 + Math.floor(Math.random() * 9999);
      emitSession({
        ...cur,
        state: "running",
        pid,
        aux: cur.aux.map((a) => {
          const def = settings.runtime.auxiliary.find((x) => x.id === a.auxId);
          return def?.autostart ? { ...a, running: true, pid: pid + 7 } : a;
        }),
      });
      log("info", "runtime", `Process started (pid ${pid})`, id);
      log("warn", "wine", "fixme:ntdll:NtQuerySystemInformation info_class SYSTEM_PERFORMANCE_INFORMATION", id);
      if (settings.translation.enabled && settings.translation.extraction !== "disabled") {
        log("info", "translation", `Text extraction started (${settings.translation.extraction})`, id);
        scheduleTranslationLines(id, gameId);
      }
    });
    return s;
  },
  session_stop: ({ sessionId, force }) => {
    const s = state.sessions.get(sessionId) ?? fail("notFound", `Session ${sessionId} not found`);
    if (s.state === "exited" || s.state === "failed") return null;
    emitSession({ ...s, state: "stopping" });
    log(force ? "warn" : "info", "session", force ? "Force-killing the process tree (wineserver -k)" : "Asking the game to close", sessionId);
    later(force ? 200 : 900, () => endSession(sessionId, force ? 137 : 0));
    return null;
  },
  session_list: () => [...state.sessions.values()],
  session_start_aux: ({ sessionId, auxId }) => {
    const s = state.sessions.get(sessionId) ?? fail("notFound", `Session ${sessionId} not found`);
    if (s.state !== "running") fail("invalidInput", "The game is not running");
    const aux = s.aux.find((a) => a.auxId === auxId) ?? fail("notFound", "Auxiliary program not found", "Save the settings first.");
    const next = { ...s, aux: s.aux.map((a) => (a.auxId === auxId ? { ...a, running: true, pid: (s.pid ?? 1) + 11 } : a)) };
    emitSession(next);
    log("info", "session", `Started auxiliary program ${aux.name}`, sessionId);
    return next;
  },
  session_preview: ({ gameId }) => launchPreview(gameId),
  logs_recent: ({ limit, sessionId }) => {
    const list = sessionId ? state.logs.filter((l) => l.sessionId === sessionId) : state.logs;
    return list.slice(-(limit ?? 500));
  },
  app_info: () => ({
    version: "0.1.0-demo",
    dataDir: `${HOME}/.local/share/vn-launcher`,
    configDir: `${HOME}/.config/vn-launcher`,
    cacheDir: `${HOME}/.cache/vn-launcher`,
    dbPath: `${HOME}/.local/share/vn-launcher/library.sqlite3`,
  }),
};

export function seedStartupLogs(): void {
  if (state.logs.length > 0) return;
  log("info", "app", "Yomika demo backend started");
  log("debug", "runtime", "Scanning ~/.steam/root/compatibilitytools.d");
  log("info", "runtime", `Found ${state.runners.length} runners`);
  log("warn", "graphics", "RT-Upscaler not found in PATH");
}

export function runHandler<K extends CommandName>(cmd: K, args: CommandArgs<K>): CommandResult<K> | Promise<CommandResult<K>> {
  const handler = handlers[cmd] as ((a: CommandArgs<K>) => CommandResult<K> | Promise<CommandResult<K>>) | undefined;
  if (!handler) fail("notImplemented", `Command ${cmd} is not implemented in the demo backend`);
  return handler(args);
}
