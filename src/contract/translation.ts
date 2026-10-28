// Translation / text extraction / OCR / overlay contract. Owned by Agent 4.
// Rust side: crates/vnl-translate + src-tauri/src/commands/translation.rs
import type { CaptureRegion, GameId, OverlayConfig, SessionId, TranslationLine } from "./core";

export type ProviderKind = "local" | "online";

export interface ProviderConfigField {
  key: string;
  label: string;
  kind: "string" | "url" | "number" | "boolean" | "select";
  options?: string[];
  required: boolean;
  default: string | number | boolean | null;
}

export interface ProviderInfo {
  id: string;
  name: string;
  kind: ProviderKind;
  /** True when using it sends text to a remote server. */
  sendsDataRemotely: boolean;
  requiresApiKey: boolean;
  hasApiKey: boolean;
  configured: boolean;
  available: boolean;
  /** Current non-secret configuration values. */
  config: Record<string, unknown>;
  fields: ProviderConfigField[];
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
  id: number;
  /** null = global entry. */
  gameId: GameId | null;
  kind: GlossaryKind;
  source: string;
  target: string;
  note: string | null;
  enabled: boolean;
}

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
}

export type TextSourceState = "inactive" | "starting" | "active" | "failed";

export interface TranslationSessionStatus {
  sessionId: SessionId;
  hook: TextSourceState;
  ocr: TextSourceState;
  diagnostics: string[];
}

export interface TranslationCommands {
  translation_providers: { args: Record<string, never>; result: ProviderInfo[] };
  /** apiKey: undefined = keep, null = delete, string = store securely. */
  translation_configure_provider: {
    args: { providerId: string; config: Record<string, unknown>; apiKey?: string | null };
    result: ProviderInfo;
  };
  translation_test_provider: { args: { providerId: string; targetLanguage: string }; result: TranslationLine };
  translation_translate: { args: { request: TranslateRequest }; result: TranslationLine };
  translation_languages: { args: Record<string, never>; result: Language[] };
  translation_glossary_list: { args: { gameId?: GameId | null }; result: GlossaryEntry[] };
  translation_glossary_upsert: { args: { entry: GlossaryEntry }; result: GlossaryEntry };
  translation_glossary_delete: { args: { id: number }; result: null };
  translation_history: { args: { gameId?: GameId | null; limit?: number }; result: TranslationLine[] };
  translation_history_clear: { args: { gameId?: GameId | null }; result: null };
  translation_ocr_backends: { args: Record<string, never>; result: BackendInfo[] };
  translation_ocr_capture: { args: { request: OcrCaptureRequest }; result: TranslationLine };
  translation_hook_backends: { args: Record<string, never>; result: BackendInfo[] };
  translation_session_status: { args: { sessionId: SessionId }; result: TranslationSessionStatus };
  overlay_show: { args: { config?: OverlayConfig | null }; result: null };
  overlay_hide: { args: Record<string, never>; result: null };
}
