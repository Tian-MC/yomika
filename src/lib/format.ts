// Locale-aware formatting helpers.

export function formatDuration(seconds: number, lang: string): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return lang === "it" ? "0 min" : "0 min";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h === 0) return `${Math.max(1, m)} min`;
  if (m === 0) return `${h} h`;
  return `${h} h ${m} min`;
}

/** Compact "1:23:45" style elapsed timer. */
export function formatElapsed(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(sec).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}

export function formatDate(unixSeconds: number, lang: string): string {
  return new Date(unixSeconds * 1000).toLocaleDateString(lang, { year: "numeric", month: "short", day: "numeric" });
}

export function formatDateTime(unixSeconds: number, lang: string): string {
  return new Date(unixSeconds * 1000).toLocaleString(lang, { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

export function formatTimeMs(ms: number, lang: string): string {
  return new Date(ms).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

/** "3 days ago" etc. using Intl.RelativeTimeFormat. */
export function formatRelative(unixSeconds: number, lang: string, nowMs = Date.now()): string {
  const diff = unixSeconds - nowMs / 1000;
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  const abs = Math.abs(diff);
  if (abs < 60) return rtf.format(Math.round(diff), "second");
  if (abs < 3600) return rtf.format(Math.round(diff / 60), "minute");
  if (abs < 86_400) return rtf.format(Math.round(diff / 3600), "hour");
  if (abs < 86_400 * 30) return rtf.format(Math.round(diff / 86_400), "day");
  if (abs < 86_400 * 365) return rtf.format(Math.round(diff / (86_400 * 30)), "month");
  return rtf.format(Math.round(diff / (86_400 * 365)), "year");
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

/** VNDB ratings are 10–100; show them on a 0–10 scale. */
export function formatRating(rating: number): string {
  const v = rating > 10 ? rating / 10 : rating;
  return v.toFixed(1);
}

/** Strip VNDB BBCode ([url=..]x[/url], [spoiler] ...) for plain display. */
export function stripBbcode(text: string): string {
  return text
    .replace(/\[url=[^\]]*\]([\s\S]*?)\[\/url\]/gi, "$1")
    .replace(/\[spoiler\][\s\S]*?\[\/spoiler\]/gi, "")
    .replace(/\[\/?(b|i|u|s|raw|quote|code)\]/gi, "")
    .trim();
}

/** Contains Japanese/CJK characters? Used to tag text with lang="ja". */
export function hasCjk(text: string | null | undefined): boolean {
  return !!text && /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uff66-\uff9f]/.test(text);
}

/** Split a command-line-ish string into args, honouring simple quotes. */
export function splitArgs(input: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quote: '"' | "'" | null = null;
  let has = false;
  for (let i = 0; i < input.length; i++) {
    const c = input[i]!;
    if (quote) {
      if (c === quote) quote = null;
      else if (c === "\\" && quote === '"' && i + 1 < input.length) cur += input[++i];
      else cur += c;
    } else if (c === '"' || c === "'") {
      quote = c;
      has = true;
    } else if (/\s/.test(c)) {
      if (has || cur) out.push(cur);
      cur = "";
      has = false;
    } else if (c === "\\" && i + 1 < input.length) {
      cur += input[++i];
      has = true;
    } else {
      cur += c;
      has = true;
    }
  }
  if (has || cur) out.push(cur);
  return out;
}

export function joinArgs(args: string[]): string {
  return args.map((a) => (a === "" ? '""' : /[\s"'\\]/.test(a) ? `"${a.replace(/(["\\])/g, "\\$1")}"` : a)).join(" ");
}
