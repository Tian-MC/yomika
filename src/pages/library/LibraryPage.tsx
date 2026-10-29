import { ArrowDownWideNarrow, ArrowUpNarrowWide, BookOpen, Clock, FilterX, Heart, LayoutGrid, List, Plus, SearchX, Star } from "lucide-react";
import { memo, useMemo, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import { Cover } from "../../components/Cover";
import { PlayControls } from "../../components/PlayControls";
import { Button, IconButton } from "../../components/ui/Button";
import { Badge, EmptyState, ErrorState, LoadingBlock } from "../../components/ui/feedback";
import { Segmented } from "../../components/ui/form";
import type { Game, GameSort, PlayStatus } from "../../contract";
import { useI18n } from "../../i18n";
import { formatDuration, formatRelative, hasCjk } from "../../lib/format";
import { useLibrary, useSessions } from "../../state/app";
import { ALL_SORTS, ALL_STATUSES, DEFAULT_FILTER, filterAndSortGames, tagCounts, type LibraryFilter } from "./filter";

type View = "grid" | "list";

function loadPref<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v && (allowed as readonly string[]).includes(v) ? (v as T) : fallback;
  } catch {
    return fallback;
  }
}

function savePref(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* ignore */
  }
}

export function LibraryPage() {
  const { t } = useI18n();
  const { games, error, reload } = useLibrary();
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const search = params.get("q") ?? "";
  const [view, setView] = useState<View>(() => loadPref("vnl.library.view", ["grid", "list"] as const, "grid"));
  const [status, setStatus] = useState<PlayStatus | null>(null);
  const [favoriteOnly, setFavoriteOnly] = useState(false);
  const [tag, setTag] = useState<string | null>(null);
  const [sort, setSort] = useState<GameSort>(() => loadPref("vnl.library.sort", ALL_SORTS, DEFAULT_FILTER.sort));
  const [descending, setDescending] = useState(() => loadPref("vnl.library.dir", ["asc", "desc"] as const, DEFAULT_FILTER.descending ? "desc" : "asc") === "desc");

  const visible = useMemo(() => {
    const filter: LibraryFilter = { search, status, favoriteOnly, tag, sort, descending };
    return games ? filterAndSortGames(games, filter) : [];
  }, [games, search, status, favoriteOnly, tag, sort, descending]);
  const tags = useMemo(() => (games ? tagCounts(games).slice(0, 40) : []), [games]);
  const filtersActive = !!search || !!status || favoriteOnly || !!tag;

  const clearFilters = () => {
    setStatus(null);
    setFavoriteOnly(false);
    setTag(null);
    if (search) void navigate("/", { replace: true });
  };

  if (games === null) return <LoadingBlock />;

  return (
    <div className="page library-page">
      <header className="page-head">
        <div>
          <h1>{t("library.title")}</h1>
          <p className="page-sub">{games.length === 0 ? t("library.subtitleEmpty") : filtersActive ? t("library.countFiltered", { shown: visible.length, total: games.length }) : t("library.count", { n: games.length })}</p>
        </div>
        <div className="page-actions">
          <Button variant="primary" icon={Plus} onClick={() => navigate("/import")}>
            {t("library.addGame")}
          </Button>
        </div>
      </header>

      {error && <ErrorState error={error} onRetry={() => void reload()} />}

      {games.length === 0 && !error ? (
        <EmptyState
          icon={BookOpen}
          title={t("library.empty.title")}
          action={
            <Button variant="primary" size="lg" icon={Plus} onClick={() => navigate("/import")}>
              {t("library.empty.cta")}
            </Button>
          }
        >
          {t("library.empty.text")}
        </EmptyState>
      ) : (
        <>
          <div className="library-toolbar" role="toolbar" aria-label={t("library.filters")}>
            <div className="toolbar-group">
              <label className="compact-field">
                <span>{t("library.status")}</span>
                <select className="input select input-sm" value={status ?? ""} onChange={(e) => setStatus((e.target.value || null) as PlayStatus | null)}>
                  <option value="">{t("library.allStatuses")}</option>
                  {ALL_STATUSES.map((s) => (
                    <option key={s} value={s}>
                      {t(`status.${s}`)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="compact-field">
                <span>{t("library.tag")}</span>
                <select className="input select input-sm" value={tag ?? ""} onChange={(e) => setTag(e.target.value || null)}>
                  <option value="">{t("library.allTags")}</option>
                  {tags.map(([name, n]) => (
                    <option key={name} value={name}>
                      {name} ({n})
                    </option>
                  ))}
                </select>
              </label>
              <button type="button" className={favoriteOnly ? "chip is-active" : "chip"} aria-pressed={favoriteOnly} onClick={() => setFavoriteOnly(!favoriteOnly)}>
                <Heart size={14} aria-hidden /> {t("library.favorites")}
              </button>
              {filtersActive && (
                <Button size="sm" variant="ghost" icon={FilterX} onClick={clearFilters}>
                  {t("library.clearFilters")}
                </Button>
              )}
            </div>
            <div className="toolbar-group">
              <label className="compact-field">
                <span>{t("library.sort")}</span>
                <select
                  className="input select input-sm"
                  value={sort}
                  onChange={(e) => {
                    const v = e.target.value as GameSort;
                    setSort(v);
                    savePref("vnl.library.sort", v);
                  }}
                >
                  {ALL_SORTS.map((s) => (
                    <option key={s} value={s}>
                      {t(`sort.${s}`)}
                    </option>
                  ))}
                </select>
              </label>
              <IconButton
                icon={descending ? ArrowDownWideNarrow : ArrowUpNarrowWide}
                label={descending ? t("library.descending") : t("library.ascending")}
                size="sm"
                onClick={() => {
                  setDescending(!descending);
                  savePref("vnl.library.dir", descending ? "asc" : "desc");
                }}
              />
              <Segmented<View>
                label={t("library.view")}
                hideLabel
                value={view}
                onChange={(v) => {
                  setView(v);
                  savePref("vnl.library.view", v);
                }}
                options={[
                  { value: "grid", label: <LayoutGrid size={15} aria-label={t("library.grid")} />, title: t("library.grid") },
                  { value: "list", label: <List size={15} aria-label={t("library.list")} />, title: t("library.list") },
                ]}
              />
            </div>
          </div>

          {visible.length === 0 ? (
            <EmptyState icon={SearchX} title={t("library.noMatches")} action={<Button onClick={clearFilters}>{t("library.clearFilters")}</Button>}>
              {search ? t("library.noMatchesSearch", { q: search }) : t("library.noMatchesText")}
            </EmptyState>
          ) : view === "grid" ? (
            <ul className="game-grid" aria-label={t("library.games")}>
              {visible.map((g) => (
                <GameCard key={g.id} game={g} />
              ))}
            </ul>
          ) : (
            <div className="game-list" role="table" aria-label={t("library.games")}>
              <div className="game-list-head" role="row">
                <span role="columnheader" className="gl-cover" />
                <span role="columnheader">{t("library.col.title")}</span>
                <span role="columnheader">{t("library.col.status")}</span>
                <span role="columnheader">{t("library.col.playtime")}</span>
                <span role="columnheader">{t("library.col.lastPlayed")}</span>
                <span role="columnheader" className="sr-only">
                  {t("session.play")}
                </span>
              </div>
              {visible.map((g) => (
                <GameRow key={g.id} game={g} />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

const STATUS_TONE: Record<PlayStatus, "neutral" | "accent" | "success" | "warning" | "danger" | "info"> = {
  unplayed: "neutral",
  playing: "accent",
  completed: "success",
  onHold: "warning",
  dropped: "danger",
  planned: "info",
};

export function StatusBadge({ status }: { status: PlayStatus }) {
  const { t } = useI18n();
  return <Badge tone={STATUS_TONE[status]}>{t(`status.${status}`)}</Badge>;
}

const GameCard = memo(function GameCard({ game }: { game: Game }) {
  const { t, lang } = useI18n();
  const { activeFor } = useSessions();
  const running = !!activeFor(game.id);
  return (
    <li className={running ? "game-card is-running" : "game-card"} data-testid="game-card">
      <Link to={`/game/${game.id}`} className="game-card-link" aria-label={game.title}>
        <Cover title={game.title} originalTitle={game.originalTitle} metadata={game.metadata} />
        <div className="game-card-body">
          <span className="game-card-title">{game.title}</span>
          {game.originalTitle && game.originalTitle !== game.title && (
            <span className="game-card-orig" lang={hasCjk(game.originalTitle) ? "ja" : undefined}>
              {game.originalTitle}
            </span>
          )}
          <span className="game-card-meta">
            <Clock size={12} aria-hidden />
            {game.playtimeSeconds > 0 ? formatDuration(game.playtimeSeconds, lang) : t("library.neverPlayed")}
            {game.lastPlayedAt && <span className="dot-sep">{formatRelative(game.lastPlayedAt, lang)}</span>}
          </span>
        </div>
      </Link>
      <div className="game-card-badges">
        {game.favorite && (
          <span className="fav-badge" title={t("library.favorite")}>
            <Star size={13} aria-hidden fill="currentColor" />
            <span className="sr-only">{t("library.favorite")}</span>
          </span>
        )}
        {running && <Badge tone="success">{t("session.state.running")}</Badge>}
      </div>
      <div className="game-card-play">
        <PlayControls game={game} compact />
      </div>
    </li>
  );
});

const GameRow = memo(function GameRow({ game }: { game: Game }) {
  const { t, lang } = useI18n();
  return (
    <div className="game-row" role="row" data-testid="game-row">
      <span role="cell" className="gl-cover">
        <Cover title={game.title} originalTitle={game.originalTitle} metadata={game.metadata} size="sm" />
      </span>
      <span role="cell" className="gl-title">
        <Link to={`/game/${game.id}`}>{game.title}</Link>
        {game.favorite && <Star size={12} className="fav-inline" aria-label={t("library.favorite")} fill="currentColor" />}
        {game.originalTitle && game.originalTitle !== game.title && (
          <small lang={hasCjk(game.originalTitle) ? "ja" : undefined}>{game.originalTitle}</small>
        )}
      </span>
      <span role="cell">
        <StatusBadge status={game.status} />
      </span>
      <span role="cell" className="num">
        {game.playtimeSeconds > 0 ? formatDuration(game.playtimeSeconds, lang) : "—"}
      </span>
      <span role="cell">{game.lastPlayedAt ? formatRelative(game.lastPlayedAt, lang) : t("library.never")}</span>
      <span role="cell" className="gl-play">
        <PlayControls game={game} compact />
      </span>
    </div>
  );
});
