import { Check, Search } from "lucide-react";
import { useState } from "react";
import type { AppError, VndbSearchResult } from "../contract";
import { useT } from "../i18n";
import { call, normalizeError } from "../lib/api";
import { remoteImageSrc } from "../lib/cover";
import { formatRating, hasCjk } from "../lib/format";
import { Cover } from "./Cover";
import { Button } from "./ui/Button";
import { EmptyState, ErrorState } from "./ui/feedback";

interface VndbSearchProps {
  initialQuery: string;
  selectedId?: string | null;
  onSelect: (r: VndbSearchResult) => void;
}

export function VndbSearch({ initialQuery, selectedId, onSelect }: VndbSearchProps) {
  const t = useT();
  const [query, setQuery] = useState(initialQuery);
  const [results, setResults] = useState<VndbSearchResult[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);

  const search = async (q = query) => {
    if (!q.trim()) return;
    setBusy(true);
    setError(null);
    try {
      setResults(await call("vndb_search", { query: q.trim() }));
    } catch (e) {
      setError(normalizeError(e));
      setResults(null);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="vndb-search">
      <form
        className="row gap-sm"
        role="search"
        onSubmit={(e) => {
          e.preventDefault();
          void search();
        }}
      >
        <input className="input grow" type="search" aria-label={t("vndb.query")} placeholder={t("vndb.query")} value={query} onChange={(e) => setQuery(e.target.value)} />
        <Button type="submit" icon={Search} loading={busy}>
          {t("vndb.search")}
        </Button>
      </form>
      {error && <ErrorState error={error} compact />}
      {results && results.length === 0 && <EmptyState title={t("vndb.noResults")} />}
      {results && results.length > 0 && (
        <ul className="vndb-results" aria-label={t("vndb.results")}>
          {results.map((r) => {
            const selected = r.id === selectedId;
            return (
              <li key={r.id}>
                <button type="button" className={selected ? "vndb-result is-selected" : "vndb-result"} aria-pressed={selected} onClick={() => onSelect(r)}>
                  <Cover title={r.title} originalTitle={r.altTitle} metadata={{ coverPath: null, coverUrl: r.imageUrl, coverSexual: r.imageSexual }} src={remoteImageSrc(r.imageUrl)} size="sm" />
                  <span className="vndb-result-text">
                    <span className="vndb-result-title">{r.title}</span>
                    {r.altTitle && r.altTitle !== r.title && <span className="vndb-result-alt" lang={hasCjk(r.altTitle) ? "ja" : undefined}>{r.altTitle}</span>}
                    <span className="vndb-result-meta">
                      {[r.released, r.developers.join(", "), r.rating !== null ? `★ ${formatRating(r.rating)}` : null, r.id].filter(Boolean).join(" · ")}
                    </span>
                  </span>
                  {selected && <Check size={18} className="vndb-check" aria-hidden />}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
