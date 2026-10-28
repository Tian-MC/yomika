import { useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
import type { AppError, CommandArgs, CommandName, CommandResult, EventMap } from "../contract";
import { call, listen, normalizeError, type Unlisten } from "./api";

export interface CommandState<T> {
  data: T | undefined;
  error: AppError | null;
  /** True until the first response for the current args arrives. */
  loading: boolean;
  /** True while a manual reload is in flight. */
  refreshing: boolean;
  reload: () => Promise<T | undefined>;
  /** Locally replace the cached data (optimistic updates, results of mutations). */
  mutate: (next: T | ((prev: T | undefined) => T)) => void;
}

interface Snapshot<T> {
  key: string | null;
  data: T | undefined;
  error: AppError | null;
}

/**
 * Load a command's result and keep it in state. Pass `null` as args to skip.
 * Args must be JSON-serialisable (they are used as the cache key).
 */
export function useCommand<K extends CommandName>(cmd: K, args: CommandArgs<K> | null): CommandState<CommandResult<K>> {
  type R = CommandResult<K>;
  const key = args === null ? null : JSON.stringify(args);
  const [snap, setSnap] = useState<Snapshot<R>>({ key: null, data: undefined, error: null });
  const [refreshing, setRefreshing] = useState(false);
  const seq = useRef(0);

  const run = useCallback(
    async (k: string): Promise<R | undefined> => {
      const id = ++seq.current;
      try {
        const data = await call(cmd, JSON.parse(k) as CommandArgs<K>);
        if (id === seq.current) setSnap({ key: k, data, error: null });
        return data;
      } catch (e) {
        if (id === seq.current) setSnap((s) => ({ key: k, data: s.key === k ? s.data : undefined, error: normalizeError(e) }));
        return undefined;
      }
    },
    [cmd],
  );

  useEffect(() => {
    if (key !== null) void run(key);
  }, [key, run]);

  const reload = useCallback(async () => {
    if (key === null) return undefined;
    setRefreshing(true);
    try {
      return await run(key);
    } finally {
      setRefreshing(false);
    }
  }, [key, run]);

  const mutate = useCallback(
    (next: R | ((prev: R | undefined) => R)) => {
      setSnap((s) => ({
        key: s.key ?? key,
        error: null,
        data: typeof next === "function" ? (next as (p: R | undefined) => R)(s.data) : next,
      }));
    },
    [key],
  );

  const current = snap.key === key;
  return {
    data: current ? snap.data : undefined,
    error: current ? snap.error : null,
    loading: key !== null && !current,
    refreshing,
    reload,
    mutate,
  };
}

/** Subscribe to a backend event for the lifetime of the component. */
export function useEvent<E extends keyof EventMap>(event: E, handler: (payload: EventMap[E]) => void): void {
  const onEvent = useEffectEvent(handler);
  useEffect(() => {
    let disposed = false;
    let unlisten: Unlisten | null = null;
    listen(event, (p) => onEvent(p))
      .then((u) => {
        if (disposed) u();
        else unlisten = u;
      })
      .catch((e: unknown) => console.warn(`listen(${event}) failed`, e));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [event]);
}

/** Re-render every `ms` milliseconds (live timers). */
export function useNow(ms: number, active = true): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(t);
  }, [ms, active]);
  return now;
}

/** Debounce a value. */
export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}
