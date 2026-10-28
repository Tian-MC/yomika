import type { Game, GameSort, PlayStatus } from "../../contract";

export interface LibraryFilter {
  search: string;
  status: PlayStatus | null;
  favoriteOnly: boolean;
  tag: string | null;
  sort: GameSort;
  descending: boolean;
}

export const DEFAULT_FILTER: LibraryFilter = {
  search: "",
  status: null,
  favoriteOnly: false,
  tag: null,
  sort: "lastPlayed",
  descending: true,
};

export const ALL_SORTS: GameSort[] = ["lastPlayed", "title", "playtime", "addedAt", "releaseDate"];
export const ALL_STATUSES: PlayStatus[] = ["playing", "unplayed", "planned", "onHold", "completed", "dropped"];

function normalize(s: string): string {
  return s.normalize("NFKC").toLowerCase();
}

function searchHaystack(g: Game): string {
  return normalize([g.title, g.originalTitle ?? "", ...g.metadata.aliases, ...g.metadata.developers, g.metadata.vndbId ?? ""].join("\u0000"));
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Value used for sorting; `null` always sorts last regardless of direction. */
function sortKey(g: Game, sort: GameSort): number | string | null {
  switch (sort) {
    case "title":
      return g.title;
    case "lastPlayed":
      return g.lastPlayedAt;
    case "playtime":
      return g.playtimeSeconds;
    case "addedAt":
      return g.addedAt;
    case "releaseDate":
      return g.metadata.releaseDate || null;
  }
}

/** Client-side filtering/sorting: instant even with hundreds of games. */
export function filterAndSortGames(games: readonly Game[], f: LibraryFilter): Game[] {
  const terms = normalize(f.search.trim()).split(/\s+/).filter(Boolean);
  const out = games.filter((g) => {
    if (f.status && g.status !== f.status) return false;
    if (f.favoriteOnly && !g.favorite) return false;
    if (f.tag && !g.metadata.tags.includes(f.tag)) return false;
    if (terms.length) {
      const hay = searchHaystack(g);
      if (!terms.every((t) => hay.includes(t))) return false;
    }
    return true;
  });
  const dir = f.descending ? -1 : 1;
  out.sort((a, b) => {
    const ka = sortKey(a, f.sort);
    const kb = sortKey(b, f.sort);
    if (ka === null && kb === null) return collator.compare(a.title, b.title);
    if (ka === null) return 1;
    if (kb === null) return -1;
    const c = typeof ka === "string" && typeof kb === "string" ? collator.compare(ka, kb) : (ka as number) - (kb as number);
    return c !== 0 ? c * dir : collator.compare(a.title, b.title);
  });
  return out;
}

/** Tag → number of games, most common first. */
export function tagCounts(games: readonly Game[]): Array<[string, number]> {
  const m = new Map<string, number>();
  for (const g of games) for (const t of g.metadata.tags) m.set(t, (m.get(t) ?? 0) + 1);
  return [...m.entries()].sort((a, b) => b[1] - a[1] || collator.compare(a[0], b[0]));
}
