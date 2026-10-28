import { ArrowDownToLine, Eraser, RefreshCw } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { LogEntry, LogLevel, SessionId } from "../contract";
import { useI18n } from "../i18n";
import { call, normalizeError } from "../lib/api";
import { formatTimeMs } from "../lib/format";
import { useEvent } from "../lib/hooks";
import { useLibrary, useSessions } from "../state/app";
import { Button, IconButton } from "./ui/Button";
import { ErrorState } from "./ui/feedback";
import type { AppError } from "../contract";

const LEVELS: LogLevel[] = ["trace", "debug", "info", "warn", "error"];
const MAX_ENTRIES = 3000;

interface LogViewerProps {
  /** Restrict to one session (game page). */
  sessionId?: SessionId | null;
  height?: number;
}

export function LogViewer({ sessionId, height = 420 }: LogViewerProps) {
  const { t, lang } = useI18n();
  const { sessions } = useSessions();
  const library = useLibrary();
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [error, setError] = useState<AppError | null>(null);
  const [minLevel, setMinLevel] = useState<LogLevel>("debug");
  const [source, setSource] = useState("");
  const [sessionFilter, setSessionFilter] = useState("");
  const [text, setText] = useState("");
  const [autoscroll, setAutoscroll] = useState(true);
  const [clearedAt, setClearedAt] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);

  const load = async () => {
    try {
      const list = await call("logs_recent", { limit: 1000, sessionId: sessionId ?? null });
      setEntries(list);
      setError(null);
    } catch (e) {
      setError(normalizeError(e));
    }
  };

  useEffect(() => {
    let alive = true;
    call("logs_recent", { limit: 1000, sessionId: sessionId ?? null })
      .then((list) => {
        if (!alive) return;
        // Merge with anything that already arrived via events.
        setEntries((cur) => (cur.length ? dedupe([...list, ...cur]) : list));
        setError(null);
      })
      .catch((e: unknown) => alive && setError(normalizeError(e)));
    return () => {
      alive = false;
    };
  }, [sessionId]);

  useEvent("vnl://log", (entry) => {
    if (sessionId && entry.sessionId !== sessionId) return;
    setEntries((cur) => {
      const next = cur.length >= MAX_ENTRIES ? cur.slice(cur.length - MAX_ENTRIES + 1) : cur.slice();
      next.push(entry);
      return next;
    });
  });

  const sources = useMemo(() => [...new Set(entries.map((e) => e.source))].sort(), [entries]);
  const sessionIds = useMemo(() => [...new Set(entries.map((e) => e.sessionId).filter((x): x is string => !!x))], [entries]);

  const filtered = useMemo(() => {
    const min = LEVELS.indexOf(minLevel);
    const q = text.trim().toLowerCase();
    return entries.filter(
      (e) =>
        e.tsMs >= clearedAt &&
        LEVELS.indexOf(e.level) >= min &&
        (!source || e.source === source) &&
        (!sessionFilter || e.sessionId === sessionFilter) &&
        (!q || e.message.toLowerCase().includes(q)),
    );
  }, [entries, minLevel, source, sessionFilter, text, clearedAt]);

  useLayoutEffect(() => {
    if (autoscroll && scroller.current) scroller.current.scrollTop = scroller.current.scrollHeight;
  }, [filtered, autoscroll]);

  const sessionLabel = (id: string) => {
    const s = sessions.find((x) => x.id === id);
    const game = s ? library.byId(s.gameId) : undefined;
    return game ? `${game.title} · ${id.slice(0, 8)}` : id.slice(0, 8);
  };

  return (
    <div className="log-viewer">
      <div className="log-toolbar">
        <label className="compact-field">
          <span>{t("logs.level")}</span>
          <select className="input select input-sm" value={minLevel} onChange={(e) => setMinLevel(e.target.value as LogLevel)}>
            {LEVELS.map((l) => (
              <option key={l} value={l}>
                {t(`logs.level.${l}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="compact-field">
          <span>{t("logs.source")}</span>
          <select className="input select input-sm" value={source} onChange={(e) => setSource(e.target.value)}>
            <option value="">{t("logs.allSources")}</option>
            {sources.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </label>
        {!sessionId && (
          <label className="compact-field">
            <span>{t("logs.session")}</span>
            <select className="input select input-sm" value={sessionFilter} onChange={(e) => setSessionFilter(e.target.value)}>
              <option value="">{t("logs.allSessions")}</option>
              {sessionIds.map((s) => (
                <option key={s} value={s}>
                  {sessionLabel(s)}
                </option>
              ))}
            </select>
          </label>
        )}
        <input className="input input-sm log-search" type="search" placeholder={t("logs.filterText")} aria-label={t("logs.filterText")} value={text} onChange={(e) => setText(e.target.value)} />
        <div className="log-toolbar-actions">
          <IconButton icon={ArrowDownToLine} label={t("logs.autoscroll")} active={autoscroll} size="sm" onClick={() => setAutoscroll(!autoscroll)} />
          <IconButton icon={Eraser} label={t("logs.clearView")} size="sm" onClick={() => setClearedAt(Date.now())} />
          <IconButton icon={RefreshCw} label={t("common.refresh")} size="sm" onClick={() => void load()} />
        </div>
      </div>
      {error && <ErrorState error={error} onRetry={() => void load()} compact />}
      <div
        className="log-lines"
        ref={scroller}
        style={{ height }}
        role="log"
        aria-live="off"
        aria-label={t("logs.title")}
        tabIndex={0}
        onScroll={(e) => {
          const el = e.currentTarget;
          const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
          if (!atBottom && autoscroll) setAutoscroll(false);
        }}
      >
        {filtered.length === 0 ? (
          <div className="log-empty">{t("logs.empty")}</div>
        ) : (
          filtered.slice(-1500).map((e, i) => (
            <div className={`log-line lvl-${e.level}`} key={`${e.tsMs}-${i}`}>
              <span className="log-ts">{formatTimeMs(e.tsMs, lang)}</span>
              <span className={`log-lvl lvl-${e.level}`}>{e.level}</span>
              <span className="log-src">{e.source}</span>
              <span className="log-msg">{e.message}</span>
            </div>
          ))
        )}
      </div>
      {!autoscroll && (
        <div className="log-foot">
          <Button size="sm" variant="ghost" icon={ArrowDownToLine} onClick={() => setAutoscroll(true)}>
            {t("logs.jumpToLatest")}
          </Button>
        </div>
      )}
    </div>
  );
}

function dedupe(list: LogEntry[]): LogEntry[] {
  const seen = new Set<string>();
  const out: LogEntry[] = [];
  for (const e of list) {
    const k = `${e.tsMs}|${e.source}|${e.message}`;
    if (seen.has(k)) continue;
    seen.add(k);
    out.push(e);
  }
  return out.sort((a, b) => a.tsMs - b.tsMs);
}
