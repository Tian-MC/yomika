// Native file pickers. In demo mode (no Tauri) we fall back to a prompt so the
// flows remain usable in a browser.
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { isTauri } from "./api";

export interface PickFileOptions {
  title?: string;
  /** e.g. [{ name: "Windows executable", extensions: ["exe"] }] */
  filters?: { name: string; extensions: string[] }[];
  defaultPath?: string;
  /** Used only in demo mode as the prompt's default value. */
  demoDefault?: string;
}

export async function pickFile(opts: PickFileOptions = {}): Promise<string | null> {
  if (!isTauri()) return demoPrompt(opts.title, opts.demoDefault ?? "/home/demo/Games/VN/NewGame/game.exe");
  const res = await open({ title: opts.title, filters: opts.filters, defaultPath: opts.defaultPath, multiple: false, directory: false });
  return typeof res === "string" ? res : null;
}

export async function pickDirectory(opts: Omit<PickFileOptions, "filters"> = {}): Promise<string | null> {
  if (!isTauri()) return demoPrompt(opts.title, opts.demoDefault ?? "/home/demo/Games/VN/NewGame");
  const res = await open({ title: opts.title, defaultPath: opts.defaultPath, multiple: false, directory: true });
  return typeof res === "string" ? res : null;
}

function demoPrompt(title: string | undefined, def: string): string | null {
  if (typeof window === "undefined" || typeof window.prompt !== "function") return null;
  try {
    const v = window.prompt(`${title ?? "Path"} (demo mode: type a path)`, def);
    return v && v.trim() ? v.trim() : null;
  } catch {
    return null;
  }
}

/** Open a folder/file with the desktop's default handler. No-op in demo mode. */
export async function openInFileManager(path: string): Promise<boolean> {
  if (!isTauri()) return false;
  await openPath(path);
  return true;
}

export async function revealInFileManager(path: string): Promise<boolean> {
  if (!isTauri()) return false;
  await revealItemInDir(path);
  return true;
}

export const EXE_FILTERS = [
  { name: "Windows executable", extensions: ["exe", "EXE", "msi", "MSI", "bat", "lnk"] },
  { name: "All files", extensions: ["*"] },
];
