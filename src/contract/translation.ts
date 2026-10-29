// Translation / text extraction / OCR / overlay contract. Owned by Agent 4.
// Rust side: crates/vnl-translate + src-tauri/src/commands/translation.rs
import type { CaptureRegion, GameId, HookThreadSelection, OverlayConfig, SessionId, TranslationLine } from "./core";

export type ProviderKind = "local" | "online";

/** Emitted to the overlay window only; payload: OverlayConfig. */
export const OVERLAY_CONFIG_EVENT = "vnl://overlay-config";

export interface ProviderConfigField {
  /** Known keys: ollama {baseUrl, model, temperature, disableThinking, allowRemoteModels, timeoutSecs};
   *  openai-compatible {baseUrl, model, temperature, timeoutSecs}; libretranslate {url, timeoutSecs};
   *  deepl {plan, formality, endpoint, timeoutSecs}. API keys are never part of `config`. */
  key: string;
  label: string;
  kind: "string" | "url" | "number" | "boolean" | "select";
  options?: string[];
  required: boolean;
  default: string | number | boolean | null;
}

/** Provider ids: "ollama" | "openai-compatible" | "libretranslate" | "deepl". */
export interface ProviderInfo {
  id: string;
  name: string;
  kind: ProviderKind;
  /** True when using it sends text to a remote server (non-loopback URL, DeepL, Ollama cloud models). */
  sendsDataRemotely: boolean;
  requiresApiKey: boolean;
  hasApiKey: boolean;
  configured: boolean;
  available: boolean;
  /** Current non-secret configuration values. */
  config: Record<string, unknown>;
  fields: ProviderConfigField[];
  /** Human-readable status, incl. where the API key is stored (keyring or 0600 file fallback). */
  notes: string | null;
}

export interface Language {
  code: string;
  name: string;
  nativeName: string;
}

export interface TranslateRequest {
  text: string;
  sourceLanguage?: string | null;
  targetLanguage?: string | null;
  providerId?: string | null;
  gameId?: GameId | null;
  sessionId?: SessionId | null;
}

export type GlossaryKind = "term" | "characterName";

export interface GlossaryEntry {
  /** 0 = create on upsert. */
  id: number;
  /** null = global entry. */
  gameId: GameId | null;
  kind: GlossaryKind;
  source: string;
  target: string;
  note: string | null;
  enabled: boolean;
}

/** OCR backends: "tesseract", "ollama:<model>"; capture methods: "capture:x11" | "capture:portal" | "capture:file".
 *  Hook backends: "lunahost", "clipboard". */
export interface BackendInfo {
  id: string;
  name: string;
  available: boolean;
  /** Why it is unavailable / what is missing. */
  diagnostics: string[];
}

export interface OcrCaptureRequest {
  gameId?: GameId | null;
  sessionId?: SessionId | null;
  region?: CaptureRegion | null;
  backend?: string | null;
  /** Translate the recognised text (default true). */
  translate?: boolean;
  /** "x11" (default; captures the game window found via WM_CLASS = exe name) | "portal" (interactive) | "file". */
  capture?: string | null;
  /** X11 display, e.g. gamescope's nested ":2". Default $DISPLAY. */
  display?: string | null;
  /** Explicit X11 window id such as "0x3000003". */
  windowId?: string | null;
  /** OCR an existing image file instead of capturing. */
  imagePath?: string | null;
}

export type TextSourceState = "inactive" | "starting" | "active" | "failed";

export interface HookThread {
  id: string;
  selector: HookThreadSelection;
  preview: string;
  received: number;
}

export interface TranslationSessionStatus {
  hookThreads: HookThread[];
  selectedHookThread: string | null;
  sessionId: SessionId;
  hook: TextSourceState;
  ocr: TextSourceState;
  diagnostics: string[];
}

export interface TranslationCommands {
  translation_providers: { args: Record<string, never>; result: ProviderInfo[] };
  /** apiKey: undefined/null = keep, "" = delete, other string = store in the Secret Service
   *  (Tauri cannot distinguish a missing argument from null). */
  translation_configure_provider: {
    args: { providerId: string; config: Record<string, unknown>; apiKey?: string | null };
    result: ProviderInfo;
  };
  /** Translates a fixed Japanese sample; not stored in history. */
  translation_test_provider: { args: { providerId: string; targetLanguage: string }; result: TranslationLine };
  /** Errors (InvalidInput) when no provider/target language is configured; provider failures come back in line.error. */
  translation_translate: { args: { request: TranslateRequest }; result: TranslationLine };
  translation_languages: { args: Record<string, never>; result: Language[] };
  /** gameId null = every entry; gameId set = global entries + that game's. */
  translation_glossary_list: { args: { gameId?: GameId | null }; result: GlossaryEntry[] };
  translation_glossary_upsert: { args: { entry: GlossaryEntry }; result: GlossaryEntry };
  translation_glossary_delete: { args: { id: number }; result: null };
  /** gameId null = all games; returns the latest `limit` (default 200) lines, oldest first. */
  translation_history: { args: { gameId?: GameId | null; limit?: number }; result: TranslationLine[] };
  translation_history_clear: { args: { gameId?: GameId | null }; result: null };
  /** OCR backends followed by capture methods (ids "capture:*"). */
  translation_ocr_backends: { args: Record<string, never>; result: BackendInfo[] };
  translation_ocr_capture: { args: { request: OcrCaptureRequest }; result: TranslationLine };
  translation_hook_backends: { args: Record<string, never>; result: BackendInfo[] };
  translation_hook_select_thread: { args: { sessionId: SessionId; gameId: GameId; threadId: string | null }; result: HookThreadSelection | null };
  translation_session_status: { args: { sessionId: SessionId }; result: TranslationSessionStatus };
  /** Creates/shows window "overlay" (index.html#/overlay) and sends OverlayConfig to it via OVERLAY_CONFIG_EVENT. */
  overlay_show: { args: { config?: OverlayConfig | null }; result: null };
  overlay_hide: { args: Record<string, never>; result: null };
}
