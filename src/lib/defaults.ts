// Client-side mirrors of the Rust `Default` impls in crates/vnl-core/src/settings.rs.
// Used to build complete settings objects (import wizard, mock backend) and to
// fill missing fields defensively.
import type {
  AppSettings,
  AuxProgram,
  GameSettings,
  GraphicsConfig,
  OverlayConfig,
  RuntimeConfig,
  TranslationConfig,
} from "../contract";

export function defaultGraphicsConfig(): GraphicsConfig {
  return {
    gamescope: {
      enabled: false,
      gameWidth: null,
      gameHeight: null,
      outputWidth: null,
      outputHeight: null,
      windowMode: "windowed",
      scaler: "auto",
      filter: "linear",
      sharpness: null,
      fpsLimit: null,
      fpsLimitUnfocused: null,
      preferOutput: null,
      forceGrabCursor: false,
      hdr: false,
      extraArgs: [],
    },
    wineFsr: { enabled: false, strength: null },
    upscaler: { enabled: false, backend: null, model: null, preset: null, scale: null, targetTitle: null, extraArgs: [] },
  };
}

export function defaultRuntimeConfig(): RuntimeConfig {
  return {
    runner: null,
    prefix: { type: "dedicated" },
    locale: { japaneseLocale: true, locale: null, timezone: null },
    env: {},
    dllOverrides: {},
    proton: { launcher: "auto", useWined3d: false, noEsync: false, noFsync: false, enableLog: false, umuGameId: null },
    auxiliary: [],
    waitForWineserver: false,
  };
}

export function defaultOverlayConfig(): OverlayConfig {
  return { enabled: false, fontSize: 22, opacity: 0.85, showOriginal: true };
}

/** Note: target language deliberately stays `null` (no built-in default). */
export function defaultTranslationConfig(): TranslationConfig {
  return {
    enabled: false,
    extraction: "disabled",
    sourceLanguage: null,
    targetLanguage: null,
    providerId: null,
    hook: { backend: null, hookCode: null, attachDelayMs: 0 },
    ocr: { backend: null, region: null, intervalMs: 0 },
    keepHistory: true,
    overlay: defaultOverlayConfig(),
  };
}

export function defaultGameSettings(app?: AppSettings | null): GameSettings {
  return {
    runtime: defaultRuntimeConfig(),
    graphics: app ? structuredClone(app.defaultGraphics) : defaultGraphicsConfig(),
    translation: defaultTranslationConfig(),
  };
}

export function defaultAppSettings(): AppSettings {
  return {
    defaultRunner: null,
    prefixesDir: null,
    uiLanguage: null,
    theme: "system",
    translation: { sourceLanguage: "ja", targetLanguage: null, providerId: null },
    defaultGraphics: defaultGraphicsConfig(),
    vndb: { enabled: true, blurSexualThreshold: 1 },
  };
}

export function newAuxProgram(): AuxProgram {
  return {
    id: newId(),
    name: "",
    exePath: "",
    args: [],
    workingDir: null,
    windows: true,
    autostart: false,
    delayMs: 0,
    stopWithGame: true,
    env: {},
  };
}

export function newId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return `id-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}
