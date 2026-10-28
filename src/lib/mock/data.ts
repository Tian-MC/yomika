// Seed data for the demo/mock backend. Never used inside Tauri.
import type {
  BackendInfo,
  DependencyStatus,
  Game,
  GameMetadata,
  GameSettings,
  GlossaryEntry,
  GraphicsCapabilities,
  Language,
  PlaySession,
  PrefixComponent,
  PrefixInfo,
  ProviderInfo,
  Runner,
  SaveBackup,
  SystemReport,
  TranslationLine,
  UpscalerInfo,
  VndbSearchResult,
} from "../../contract";
import { defaultAppSettings, defaultGameSettings } from "../defaults";

const HOME = "/home/demo";
const DAY = 86_400;

/** Deterministic SVG "cover art" so the demo has covers without remote assets. */
export function svgCover(title: string, sub: string, hueA: number, hueB: number): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="300" height="420" viewBox="0 0 300 420">
<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
<stop offset="0" stop-color="hsl(${hueA} 55% 38%)"/><stop offset="1" stop-color="hsl(${hueB} 60% 16%)"/></linearGradient>
<radialGradient id="r" cx="0.7" cy="0.25" r="0.6"><stop offset="0" stop-color="hsl(${hueA} 90% 80%)" stop-opacity="0.55"/><stop offset="1" stop-color="hsl(${hueA} 90% 80%)" stop-opacity="0"/></radialGradient></defs>
<rect width="300" height="420" fill="url(#g)"/><rect width="300" height="420" fill="url(#r)"/>
<circle cx="230" cy="110" r="46" fill="hsl(${hueB} 80% 92%)" fill-opacity="0.18"/>
<path d="M0 330 Q 80 290 150 320 T 300 300 V 420 H 0 Z" fill="hsl(${hueB} 50% 8%)" fill-opacity="0.55"/>
<text x="24" y="368" font-family="Noto Sans CJK JP, Noto Sans JP, sans-serif" font-size="26" font-weight="700" fill="#fff">${escapeXml(sub)}</text>
<text x="24" y="396" font-family="sans-serif" font-size="14" fill="#fff" fill-opacity="0.8">${escapeXml(title)}</text>
</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

function escapeXml(s: string): string {
  return s.replace(/[<>&"']/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;", '"': "&quot;", "'": "&apos;" })[c] ?? c);
}

function meta(partial: Partial<GameMetadata>): GameMetadata {
  return {
    vndbId: null,
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
    ...partial,
  };
}

export interface VndbCatalogEntry {
  result: VndbSearchResult;
  metadata: GameMetadata;
}

export function vndbCatalog(now: number): VndbCatalogEntry[] {
  const entries: Array<[string, string, string, string, string[], number, number, string[], string, number, number, number]> = [
    ["v2002", "Steins;Gate", "シュタインズ・ゲート", "2009-10-15", ["5pb.", "Nitroplus"], 89, 2700, ["Time Travel", "Science Fiction", "Mystery"], "A self-proclaimed mad scientist discovers a way to send messages to the past.", 0, 200, 230],
    ["v4", "Clannad", "CLANNAD", "2004-04-28", ["Key"], 85, 4200, ["Drama", "School Life", "Family"], "Tomoya Okazaki meets a girl standing alone at the bottom of the hill to school.", 0, 30, 350],
    ["v97", "Saya no Uta", "沙耶の唄", "2003-12-26", ["Nitroplus"], 80, 360, ["Horror", "Romance", "Psychological"], "After an accident, a medical student perceives the world as a nightmare — except for one girl.", 2, 0, 320],
    ["v92", "Muv-Luv Alternative", "マブラヴ オルタネイティヴ", "2006-02-24", ["âge"], 90, 3000, ["Mecha", "War", "Science Fiction"], "Humanity fights a desperate war against an alien threat known as the BETA.", 0, 210, 260],
    ["v17", "Ever17 -the out of infinity-", "Ever17 -the out of infinity-", "2002-08-29", ["KID"], 86, 1800, ["Mystery", "Underwater", "Thriller"], "Seven people are trapped in an underwater theme park slowly flooding.", 0, 190, 220],
    ["v11", "Fate/stay night", "フェイト/ステイナイト", "2004-01-30", ["TYPE-MOON"], 84, 4500, ["Fantasy", "Action", "Magic"], "Seven mages summon heroic spirits to fight for the Holy Grail.", 1, 350, 10],
    ["v67", "Higurashi no Naku Koro ni", "ひぐらしのなく頃に", "2002-08-10", ["07th Expansion"], 82, 2400, ["Mystery", "Horror", "Rural"], "In the summer of 1983, strange deaths repeat in the village of Hinamizawa.", 0, 40, 100],
    ["v5154", "Sakura no Uta", "サクラノ詩 －櫻の森の上を舞う－", "2015-10-30", ["Makura"], 84, 3200, ["Art", "Drama", "Coming of Age"], "A young painter returns to drawing amid the cherry blossoms of his town.", 1, 330, 290],
  ];
  return entries.map(([id, title, alt, released, developers, rating, length, tags, description, sexual, hueA, hueB]) => {
    const cover = svgCover(title, alt, hueA, hueB);
    return {
      result: { id, title, altTitle: alt, released, developers, imageUrl: null, imageSexual: sexual, rating },
      metadata: meta({
        vndbId: id,
        description,
        developers,
        releaseDate: released,
        tags,
        aliases: [alt],
        rating,
        lengthMinutes: length,
        coverPath: cover,
        coverSexual: sexual,
        fetchedAt: now - 3 * DAY,
      }),
    };
  });
}

export function seedGames(now: number): Game[] {
  const cat = vndbCatalog(now);
  const pick = (i: number): GameMetadata => structuredClone(cat[i]!.metadata);
  const g = (
    id: number,
    title: string,
    originalTitle: string | null,
    folder: string,
    exe: string,
    status: Game["status"],
    favorite: boolean,
    playtimeSeconds: number,
    lastPlayedDaysAgo: number | null,
    addedDaysAgo: number,
    metadata: GameMetadata,
    notes: string | null = null,
  ): Game => ({
    id,
    title,
    originalTitle,
    exePath: `${HOME}/Games/VN/${folder}/${exe}`,
    installDir: `${HOME}/Games/VN/${folder}`,
    workingDir: null,
    launchArgs: [],
    status,
    favorite,
    playtimeSeconds,
    lastPlayedAt: lastPlayedDaysAgo === null ? null : now - lastPlayedDaysAgo * DAY - 3600,
    addedAt: now - addedDaysAgo * DAY,
    metadata,
    notes,
  });
  return [
    g(1, "Steins;Gate", "シュタインズ・ゲート", "SteinsGate", "Launcher.exe", "completed", true, 41 * 3600 + 1200, 12, 90, pick(0), "True ending reached. El Psy Kongroo."),
    g(2, "Clannad", "CLANNAD", "CLANNAD", "RealLive.exe", "playing", true, 18 * 3600 + 540, 1, 60, pick(1)),
    g(3, "Saya no Uta", "沙耶の唄", "Saya", "saya.exe", "onHold", false, 2 * 3600, 30, 45, pick(2)),
    g(4, "Muv-Luv Alternative", "マブラヴ オルタネイティヴ", "MuvLuvAlt", "alternative.exe", "planned", false, 0, null, 20, pick(3)),
    g(5, "Ever17 -the out of infinity-", null, "Ever17", "ever17PC_us.exe", "dropped", false, 5 * 3600 + 900, 120, 200, pick(4)),
    g(6, "Higurashi no Naku Koro ni", "ひぐらしのなく頃に", "Higurashi", "HigurashiEp01.exe", "playing", false, 9 * 3600, 3, 15, pick(6)),
    g(7, "Untitled doujin VN", null, "doujin_vn", "game.exe", "unplayed", false, 0, null, 2, meta({ tags: ["Doujin"] })),
  ];
}

export function seedSettings(gameId: number): GameSettings {
  const s = defaultGameSettings(defaultAppSettings());
  if (gameId === 1) {
    s.runtime.runner = { id: "proton-ge-9-20" };
    s.graphics.gamescope.enabled = true;
    s.graphics.gamescope.gameWidth = 1280;
    s.graphics.gamescope.gameHeight = 720;
    s.graphics.gamescope.outputWidth = 2560;
    s.graphics.gamescope.outputHeight = 1440;
    s.graphics.gamescope.filter = "fsr";
    s.graphics.gamescope.scaler = "fit";
    s.translation.enabled = true;
    s.translation.extraction = "hook";
    s.translation.providerId = "argos";
  }
  if (gameId === 2) {
    s.runtime.prefix = { type: "shared", id: "pfx-shared-jp" };
    s.runtime.auxiliary = [
      {
        id: "aux-textractor",
        name: "Textractor",
        exePath: `${HOME}/Tools/Textractor/x86/Textractor.exe`,
        args: [],
        workingDir: null,
        windows: true,
        autostart: true,
        delayMs: 3000,
        stopWithGame: true,
        env: {},
      },
    ];
  }
  return s;
}

export function seedRunners(): Runner[] {
  return [
    { id: "system-wine", name: "Wine (system)", kind: "wine", source: "system", path: "/usr/bin/wine", version: "wine-10.4" },
    { id: "proton-ge-9-20", name: "GE-Proton9-20", kind: "proton", source: "steam", path: `${HOME}/.steam/root/compatibilitytools.d/GE-Proton9-20`, version: "GE-Proton9-20" },
    { id: "proton-9", name: "Proton 9.0", kind: "proton", source: "steam", path: `${HOME}/.steam/steam/steamapps/common/Proton 9.0`, version: "9.0-4" },
    { id: "native", name: "Native (Linux)", kind: "native", source: "system", path: "/usr/bin/env", version: null },
  ];
}

export function seedPrefixes(now: number): PrefixInfo[] {
  return [
    { id: "pfx-shared-jp", name: "Shared JP prefix", path: `${HOME}/.local/share/vn-launcher/prefixes/shared-jp`, arch: "win64", kind: { type: "shared" }, runnerId: "system-wine", createdAt: now - 80 * DAY, managed: true, initialized: true },
    { id: "pfx-game-1", name: "Steins;Gate", path: `${HOME}/.local/share/vn-launcher/prefixes/game-1`, arch: "win64", kind: { type: "dedicated", gameId: 1 }, runnerId: "proton-ge-9-20", createdAt: now - 90 * DAY, managed: true, initialized: true },
    { id: "pfx-legacy", name: "Old Lutris prefix", path: `${HOME}/Games/wine-prefixes/vn32`, arch: "win32", kind: { type: "external" }, runnerId: null, createdAt: now - 400 * DAY, managed: false, initialized: true },
  ];
}

export function seedPlaySessions(now: number): PlaySession[] {
  const out: PlaySession[] = [];
  let id = 1;
  const add = (gameId: number, daysAgo: number, minutes: number) => {
    const startedAt = now - daysAgo * DAY - 7200;
    out.push({ id: id++, gameId, startedAt, endedAt: startedAt + minutes * 60, durationSeconds: minutes * 60 });
  };
  [12, 14, 17, 25, 33].forEach((d, i) => add(1, d, 90 + i * 25));
  [1, 2, 5, 9].forEach((d, i) => add(2, d, 60 + i * 30));
  add(3, 30, 120);
  [3, 4, 6].forEach((d) => add(6, d, 180));
  return out;
}

export function seedLanguages(): Language[] {
  return [
    { code: "ja", name: "Japanese", nativeName: "日本語" },
    { code: "en", name: "English", nativeName: "English" },
    { code: "it", name: "Italian", nativeName: "Italiano" },
    { code: "de", name: "German", nativeName: "Deutsch" },
    { code: "fr", name: "French", nativeName: "Français" },
    { code: "es", name: "Spanish", nativeName: "Español" },
    { code: "pt-BR", name: "Portuguese (Brazil)", nativeName: "Português (Brasil)" },
    { code: "ru", name: "Russian", nativeName: "Русский" },
    { code: "zh-Hans", name: "Chinese (Simplified)", nativeName: "简体中文" },
    { code: "zh-Hant", name: "Chinese (Traditional)", nativeName: "繁體中文" },
    { code: "ko", name: "Korean", nativeName: "한국어" },
  ];
}

export function seedProviders(): ProviderInfo[] {
  return [
    {
      id: "argos",
      name: "Argos Translate (offline)",
      kind: "local",
      sendsDataRemotely: false,
      requiresApiKey: false,
      hasApiKey: false,
      configured: true,
      available: true,
      config: { modelsDir: `${HOME}/.local/share/argos-translate` },
      fields: [{ key: "modelsDir", label: "Models directory", kind: "string", required: false, default: null }],
      notes: "Runs fully on this machine. Quality for Japanese is limited.",
    },
    {
      id: "ollama",
      name: "Ollama (local LLM)",
      kind: "local",
      sendsDataRemotely: false,
      requiresApiKey: false,
      hasApiKey: false,
      configured: false,
      available: true,
      config: { endpoint: "http://127.0.0.1:11434", model: "" },
      fields: [
        { key: "endpoint", label: "Endpoint", kind: "url", required: true, default: "http://127.0.0.1:11434" },
        { key: "model", label: "Model", kind: "select", options: ["qwen2.5:14b", "gemma3:12b", "llama3.1:8b"], required: true, default: null },
        { key: "temperature", label: "Temperature", kind: "number", required: false, default: 0.2 },
        { key: "includeContext", label: "Send previous lines as context", kind: "boolean", required: false, default: true },
      ],
      notes: "Uses a model served by Ollama. Remote endpoints would send text off this machine.",
    },
    {
      id: "deepl",
      name: "DeepL API",
      kind: "online",
      sendsDataRemotely: true,
      requiresApiKey: true,
      hasApiKey: false,
      configured: false,
      available: true,
      config: { plan: "free" },
      fields: [
        { key: "plan", label: "Plan", kind: "select", options: ["free", "pro"], required: true, default: "free" },
        { key: "formality", label: "Formality", kind: "select", options: ["default", "more", "less"], required: false, default: "default" },
      ],
      notes: null,
    },
    {
      id: "openai-compatible",
      name: "OpenAI-compatible API",
      kind: "online",
      sendsDataRemotely: true,
      requiresApiKey: true,
      hasApiKey: true,
      configured: true,
      available: true,
      config: { baseUrl: "https://api.example.com/v1", model: "translator-large" },
      fields: [
        { key: "baseUrl", label: "Base URL", kind: "url", required: true, default: null },
        { key: "model", label: "Model", kind: "string", required: true, default: null },
        { key: "maxTokens", label: "Max tokens", kind: "number", required: false, default: 512 },
      ],
      notes: "Any server exposing the OpenAI chat completions API.",
    },
  ];
}

export function seedGlossary(): GlossaryEntry[] {
  return [
    { id: 1, gameId: null, kind: "term", source: "先輩", target: "senpai", note: "Keep honorific", enabled: true },
    { id: 2, gameId: 1, kind: "characterName", source: "岡部倫太郎", target: "Rintarou Okabe", note: null, enabled: true },
    { id: 3, gameId: 1, kind: "characterName", source: "牧瀬紅莉栖", target: "Kurisu Makise", note: "Also 'Christina'", enabled: true },
    { id: 4, gameId: 1, kind: "term", source: "Dメール", target: "D-Mail", note: null, enabled: true },
  ];
}

export const SAMPLE_LINES: Array<[string, string]> = [
  ["「またここで会ったね」", "“We meet here again.”"],
  ["桜の花びらが、静かに舞い落ちていく。", "Cherry petals drift down quietly."],
  ["……それでも、俺は前に進む。", "...Even so, I keep moving forward."],
  ["「この世界線は、何かがおかしい」", "“Something is wrong with this world line.”"],
  ["夕焼けの坂道を、ふたりで歩いた。", "We walked the hill road together at sunset."],
];

export function seedHistory(nowMs: number): TranslationLine[] {
  return SAMPLE_LINES.slice(0, 3).map(([src, dst], i) => ({
    id: `hist-${i + 1}`,
    sessionId: null,
    gameId: 1,
    origin: "hook",
    sourceText: src,
    translatedText: dst,
    sourceLanguage: "ja",
    targetLanguage: "en",
    providerId: "argos",
    error: null,
    tsMs: nowMs - (3 - i) * 600_000,
  }));
}

export function seedBackups(now: number): SaveBackup[] {
  return [
    {
      id: 1,
      gameId: 1,
      createdAt: now - 14 * DAY,
      label: "Before true route",
      sizeBytes: 1_842_000,
      archivePath: `${HOME}/.local/share/vn-launcher/backups/1/2025-backup-1.tar.zst`,
      sourcePaths: [`${HOME}/Games/VN/SteinsGate/savedata`],
    },
    {
      id: 2,
      gameId: 1,
      createdAt: now - 2 * DAY,
      label: null,
      sizeBytes: 1_910_400,
      archivePath: `${HOME}/.local/share/vn-launcher/backups/1/2025-backup-2.tar.zst`,
      sourcePaths: [`${HOME}/Games/VN/SteinsGate/savedata`],
    },
  ];
}

export function seedDependencies(): DependencyStatus[] {
  const hints = (pkg: string, flatpak?: string) => [
    { distro: "arch", command: `sudo pacman -S ${pkg}` },
    { distro: "debian", command: `sudo apt install ${pkg}` },
    { distro: "fedora", command: `sudo dnf install ${pkg}` },
    ...(flatpak ? [{ distro: "flatpak", command: flatpak }] : []),
  ];
  return [
    { id: "wine", name: "Wine", category: "runtime", required: true, found: true, version: "wine-10.4", path: "/usr/bin/wine", purpose: "Runs Windows games outside of Proton.", installHints: hints("wine") },
    { id: "umu", name: "umu-launcher", category: "runtime", required: false, found: false, version: null, path: null, purpose: "Runs Proton outside Steam with the Steam Runtime container.", installHints: hints("umu-launcher") },
    { id: "winetricks", name: "winetricks", category: "runtime", required: false, found: true, version: "20250102", path: "/usr/bin/winetricks", purpose: "Installs CJK fonts, codecs and runtimes into prefixes.", installHints: hints("winetricks") },
    { id: "gamescope", name: "gamescope", category: "graphics", required: false, found: true, version: "3.16.1", path: "/usr/bin/gamescope", purpose: "Micro-compositor for scaling, FSR/NIS upscaling and frame limiting.", installHints: hints("gamescope", "flatpak install org.freedesktop.Platform.VulkanLayer.gamescope") },
    { id: "rt-upscaler", name: "RT-Upscaler", category: "graphics", required: false, found: false, version: null, path: null, purpose: "Real-time AI upscaling of low-resolution VNs.", installHints: [{ distro: "generic", command: "See https://github.com/ (project page) for release binaries" }] },
    { id: "vulkaninfo", name: "vulkaninfo", category: "graphics", required: false, found: true, version: "1.4.304", path: "/usr/bin/vulkaninfo", purpose: "Reports Vulkan driver support.", installHints: hints("vulkan-tools") },
    { id: "tesseract", name: "Tesseract OCR + jpn", category: "ocr", required: false, found: true, version: "5.5.0", path: "/usr/bin/tesseract", purpose: "Offline OCR for games without hookable text.", installHints: hints("tesseract tesseract-data-jpn") },
    { id: "grim", name: "grim", category: "ocr", required: false, found: false, version: null, path: null, purpose: "Screen capture on wlroots Wayland compositors.", installHints: hints("grim") },
    { id: "lunahost", name: "LunaHost", category: "translation", required: false, found: false, version: null, path: null, purpose: "Text hooking engine (runs inside the prefix).", installHints: [{ distro: "generic", command: "Download LunaHook release and set its path in Translation › Hook backends" }] },
    { id: "secret-service", name: "Secret Service (keyring)", category: "other", required: false, found: true, version: null, path: null, purpose: "Stores API keys securely.", installHints: hints("gnome-keyring") },
  ];
}

export function seedSystem(): SystemReport {
  return {
    os: "Arch Linux",
    kernel: "6.17.4-zen1-1-zen",
    desktop: "KDE",
    sessionType: "wayland",
    insideFlatpak: false,
    cpu: "AMD Ryzen 7 7840U w/ Radeon 780M Graphics (16 threads)",
    memoryMb: 31_744,
    gpus: ["AMD Radeon 780M (RADV PHOENIX)"],
    vulkan: "1.4.304 (Mesa 25.2.3)",
  };
}

export function seedCapabilities(): GraphicsCapabilities {
  return {
    sessionType: "wayland",
    xwayland: true,
    desktop: "KDE",
    gamescope: { path: "/usr/bin/gamescope", version: "3.16.1" },
    rtUpscaler: null,
    monitors: [
      { name: "eDP-1", width: 2880, height: 1800, refreshHz: 120, primary: true },
      { name: "DP-2", width: 2560, height: 1440, refreshHz: 144, primary: false },
    ],
    gpus: ["AMD Radeon 780M (RADV PHOENIX)"],
    notes: ["HDR requires a Wayland session and the gamescope WSI layer.", "Nested gamescope on KDE may need --force-grab-cursor for some engines."],
  };
}

export function seedUpscaler(): UpscalerInfo {
  return {
    available: false,
    tool: null,
    models: [
      { id: "anime4k-s", name: "Anime4K (S)", path: null, scale: 2 },
      { id: "realesrgan-anime", name: "Real-ESRGAN anime", path: null, scale: 4 },
    ],
    presets: ["quality", "balanced", "performance"],
    limitations: [
      "RT-Upscaler was not found on this system.",
      "Only works with windowed games; adds roughly one frame of latency.",
    ],
  };
}

export function seedPrefixComponents(): PrefixComponent[] {
  return [
    { id: "cjkfonts", name: "CJK fonts", category: "fonts", winetricksVerbs: ["cjkfonts"], installed: true, description: "Japanese/Chinese/Korean fonts so text is not rendered as boxes." },
    { id: "fakejapanese", name: "Japanese font substitutes", category: "fonts", winetricksVerbs: ["fakejapanese"], installed: false, description: "Maps MS Gothic/Mincho to available Japanese fonts." },
    { id: "quartz", name: "DirectShow (quartz)", category: "codecs", winetricksVerbs: ["quartz", "devenum"], installed: false, description: "Needed by many older VNs for opening movies." },
    { id: "lavfilters", name: "LAV Filters", category: "codecs", winetricksVerbs: ["lavfilters"], installed: null, description: "Decodes MPEG/H.264 video used in OP/ED movies." },
    { id: "vcrun", name: "Visual C++ runtimes", category: "runtime", winetricksVerbs: ["vcrun2019"], installed: true, description: "Required by many modern engines." },
  ];
}

export function seedOcrBackends(): BackendInfo[] {
  return [
    { id: "tesseract", name: "Tesseract (jpn)", available: true, diagnostics: [] },
    { id: "manga-ocr", name: "manga-ocr", available: false, diagnostics: ["Python package 'manga_ocr' not found."] },
    { id: "ollama-vision", name: "Ollama vision model", available: false, diagnostics: ["No vision model configured in Ollama."] },
  ];
}

export function seedHookBackends(): BackendInfo[] {
  return [
    { id: "lunahost", name: "LunaHost", available: false, diagnostics: ["LunaHost CLI not found. Set its path in Settings.", "Requires a Windows prefix (not available for native games)."] },
    { id: "textractor", name: "Textractor CLI", available: true, diagnostics: [] },
  ];
}
