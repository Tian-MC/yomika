// Typed IPC layer. Every backend call in the UI goes through `call()` and every
// backend event through `listen()`. Outside Tauri (plain browser, Vitest) both
// are routed to the in-memory mock backend in `./mock`; inside Tauri the mock
// is never loaded.
import { invoke, isTauri as tauriIsTauri } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { AppError, CommandArgs, CommandName, CommandResult, ErrorCode, EventMap } from "../contract";

export type EventName = keyof EventMap;
export type Unlisten = () => void;

const ERROR_CODES: readonly ErrorCode[] = [
  "notFound",
  "invalidInput",
  "io",
  "database",
  "network",
  "missingDependency",
  "process",
  "confirmationRequired",
  "notImplemented",
  "unsupported",
  "internal",
];

/** True when running inside the Tauri webview (real backend available). */
export function isTauri(): boolean {
  try {
    return tauriIsTauri();
  } catch {
    return false;
  }
}

/** Demo mode = no backend; data comes from the in-memory mock. */
export function isDemoMode(): boolean {
  return !isTauri();
}

export function isAppError(value: unknown): value is AppError {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return typeof v.code === "string" && typeof v.message === "string";
}

/** Normalise anything a rejected promise may carry into an `AppError`. */
export function normalizeError(error: unknown): AppError {
  if (isAppError(error)) {
    const code = (ERROR_CODES as readonly string[]).includes(error.code) ? error.code : "internal";
    const details =
      error.details === undefined || error.details === null
        ? null
        : typeof error.details === "string"
          ? error.details
          : safeStringify(error.details);
    return { code, message: error.message, details };
  }
  if (typeof error === "string") {
    // Tauri rejects with a plain string for e.g. unknown commands / arg errors.
    const notFound = /command .* not found/i.test(error);
    return { code: notFound ? "notImplemented" : "internal", message: error, details: null };
  }
  if (error instanceof Error) {
    return { code: "internal", message: error.message || error.name, details: error.stack ?? null };
  }
  if (typeof error === "object" && error !== null) {
    const v = error as Record<string, unknown>;
    if (typeof v.message === "string") return { code: "internal", message: v.message, details: safeStringify(error) };
    return { code: "internal", message: "Unexpected error", details: safeStringify(error) };
  }
  return { code: "internal", message: "Unexpected error", details: error === undefined ? null : String(error) };
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

type MockModule = typeof import("./mock");
let mockModule: Promise<MockModule> | null = null;
function loadMock(): Promise<MockModule> {
  mockModule ??= import("./mock");
  return mockModule;
}

/**
 * Invoke a backend command. Always rejects with a normalised `AppError`.
 */
export async function call<K extends CommandName>(cmd: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
  try {
    if (isTauri()) {
      return await invoke<CommandResult<K>>(cmd, args as Record<string, unknown>);
    }
    const mock = await loadMock();
    return await mock.mockInvoke(cmd, args);
  } catch (error) {
    throw normalizeError(error);
  }
}

/**
 * Subscribe to a backend event. Resolves with an unlisten function.
 */
export async function listen<E extends EventName>(event: E, handler: (payload: EventMap[E]) => void): Promise<Unlisten> {
  if (isTauri()) {
    return tauriListen<EventMap[E]>(event, (e) => handler(e.payload));
  }
  const mock = await loadMock();
  return mock.mockListen(event, handler);
}
