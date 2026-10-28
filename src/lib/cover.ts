import { convertFileSrc } from "@tauri-apps/api/core";
import type { GameMetadata } from "../contract";
import { isTauri } from "./api";

/** Resolve a displayable image URL for a game's cover, or null. */
export function coverSrc(meta: Pick<GameMetadata, "coverPath" | "coverUrl">): string | null {
  const path = meta.coverPath;
  if (path) {
    if (path.startsWith("data:") || path.startsWith("blob:")) return path;
    if (isTauri()) {
      try {
        return convertFileSrc(path);
      } catch {
        /* fall through */
      }
    }
  }
  // CSP allows the VNDB image CDN; used when the cover was not cached yet.
  if (meta.coverUrl && /^https:\/\/t\.vndb\.org\//.test(meta.coverUrl)) return meta.coverUrl;
  return null;
}

/** Remote VNDB image URL, if CSP allows it. */
export function remoteImageSrc(url: string | null): string | null {
  if (!url) return null;
  if (url.startsWith("data:")) return url;
  return /^https:\/\/t\.vndb\.org\//.test(url) ? url : null;
}

export function shouldBlur(sexual: number | null | undefined, threshold: number | null | undefined): boolean {
  if (threshold === null || threshold === undefined) return false;
  if (sexual === null || sexual === undefined) return false;
  return sexual >= threshold;
}

/** Stable pleasant hue from a string (placeholder covers). */
export function hueFor(text: string): number {
  let h = 0;
  for (const ch of text) h = (h * 31 + (ch.codePointAt(0) ?? 0)) % 360;
  return h;
}
