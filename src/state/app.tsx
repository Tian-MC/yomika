// Global app state: app settings, the game list, live sessions, background
// tasks and the translation language list. Split into several contexts so
// fast-changing data (sessions/tasks) does not re-render the whole tree.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { AppError, AppSettings, Game, GameId, Language, SessionInfo, TaskProgress } from "../contract";
import { call, normalizeError } from "../lib/api";
import { useEvent } from "../lib/hooks";
import { resolveUiLanguage, translate } from "../i18n";
import { useToasts } from "./toasts";

// ------------------------------------------------------------- settings

interface SettingsValue {
  appSettings: AppSettings | null;
  error: AppError | null;
  reload: () => Promise<void>;
  save: (s: AppSettings) => Promise<AppSettings>;
}

const SettingsContext = createContext<SettingsValue | null>(null);

// -------------------------------------------------------------- library

interface LibraryValue {
  games: Game[] | null;
  error: AppError | null;
  reload: () => Promise<void>;
  upsert: (g: Game) => void;
  removeLocal: (id: GameId) => void;
  byId: (id: GameId) => Game | undefined;
}

const LibraryContext = createContext<LibraryValue | null>(null);

// ------------------------------------------------------------- sessions

interface SessionsValue {
  sessions: SessionInfo[];
  /** Latest session (any state) for the game. */
  latestFor: (gameId: GameId) => SessionInfo | undefined;
  /** The game's session if it is starting/running/stopping. */
  activeFor: (gameId: GameId) => SessionInfo | undefined;
  upsert: (s: SessionInfo) => void;
}

const SessionsContext = createContext<SessionsValue | null>(null);

export function isActiveSession(s: SessionInfo): boolean {
  return s.state === "starting" || s.state === "running" || s.state === "stopping";
}

// ---------------------------------------------------------------- tasks

interface TasksValue {
  tasks: TaskProgress[];
  dismiss: (taskId: string) => void;
}

const TasksContext = createContext<TasksValue | null>(null);

// ------------------------------------------------------------ languages

interface LanguagesValue {
  languages: Language[] | null;
  error: AppError | null;
  nameFor: (code: string | null | undefined) => string | null;
}

const LanguagesContext = createContext<LanguagesValue | null>(null);

// ------------------------------------------------------------- provider

export function AppStateProvider({ children }: { children: ReactNode }) {
  const toasts = useToasts();

  // settings
  const [appSettings, setAppSettings] = useState<AppSettings | null>(null);
  const [settingsError, setSettingsError] = useState<AppError | null>(null);
  const reloadSettings = useCallback(async () => {
    try {
      setAppSettings(await call("settings_get_app", {}));
      setSettingsError(null);
    } catch (e) {
      setSettingsError(normalizeError(e));
    }
  }, []);
  const saveSettings = useCallback(async (s: AppSettings) => {
    const saved = await call("settings_save_app", { settings: s });
    setAppSettings(saved);
    setSettingsError(null);
    return saved;
  }, []);

  // library
  const [games, setGames] = useState<Game[] | null>(null);
  const [gamesError, setGamesError] = useState<AppError | null>(null);
  const reloadGames = useCallback(async () => {
    try {
      setGames(await call("library_list_games", { query: {} }));
      setGamesError(null);
    } catch (e) {
      setGamesError(normalizeError(e));
      setGames((g) => g ?? []);
    }
  }, []);
  const upsertGame = useCallback((g: Game) => {
    setGames((list) => {
      if (!list) return [g];
      const idx = list.findIndex((x) => x.id === g.id);
      if (idx < 0) return [...list, g];
      const next = [...list];
      next[idx] = g;
      return next;
    });
  }, []);
  const removeGameLocal = useCallback((id: GameId) => setGames((list) => (list ? list.filter((g) => g.id !== id) : list)), []);

  // sessions
  const [sessionMap, setSessionMap] = useState<Record<string, SessionInfo>>({});
  const upsertSession = useCallback((s: SessionInfo) => setSessionMap((m) => ({ ...m, [s.id]: s })), []);

  // tasks
  const [taskMap, setTaskMap] = useState<Record<string, TaskProgress>>({});
  const taskTimers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const dismissTask = useCallback((id: string) => {
    setTaskMap((m) => {
      const next = { ...m };
      delete next[id];
      return next;
    });
  }, []);

  // languages
  const [languages, setLanguages] = useState<Language[] | null>(null);
  const [languagesError, setLanguagesError] = useState<AppError | null>(null);

  useEffect(() => {
    call("settings_get_app", {})
      .then((s) => {
        setAppSettings(s);
        setSettingsError(null);
      })
      .catch((e: unknown) => setSettingsError(normalizeError(e)));
    call("library_list_games", { query: {} })
      .then((g) => {
        setGames(g);
        setGamesError(null);
      })
      .catch((e: unknown) => {
        setGamesError(normalizeError(e));
        setGames((g) => g ?? []);
      });
    call("session_list", {})
      .then((list) => setSessionMap((m) => Object.fromEntries([...list.map((s) => [s.id, s] as const), ...Object.entries(m)])))
      .catch(() => undefined);
    call("translation_languages", {})
      .then((l) => setLanguages(l))
      .catch((e: unknown) => {
        setLanguagesError(normalizeError(e));
        setLanguages([]);
      });
  }, []);

  useEffect(() => {
    const timers = taskTimers.current;
    return () => {
      for (const timer of timers.values()) clearTimeout(timer);
    };
  }, []);

  useEvent("vnl://session", (s) => {
    const prev = sessionMap[s.id];
    upsertSession(s);
    if ((s.state === "exited" || s.state === "failed") && prev?.state !== s.state) {
      void reloadGames();
      if (s.state === "failed") toasts.push({ kind: "error", title: translate(resolveUiLanguage(appSettings?.uiLanguage), "session.failed"), message: s.error, details: s.commandPreview });
    }
  });

  useEvent("vnl://task", (task) => {
    setTaskMap((m) => ({ ...m, [task.taskId]: task }));
    const existing = taskTimers.current.get(task.taskId);
    if (existing) clearTimeout(existing);
    if (task.state === "done") {
      taskTimers.current.set(
        task.taskId,
        setTimeout(() => {
          taskTimers.current.delete(task.taskId);
          dismissTask(task.taskId);
        }, 5000),
      );
    }
  });

  const settingsValue = useMemo<SettingsValue>(
    () => ({ appSettings, error: settingsError, reload: reloadSettings, save: saveSettings }),
    [appSettings, settingsError, reloadSettings, saveSettings],
  );

  const libraryValue = useMemo<LibraryValue>(
    () => ({
      games,
      error: gamesError,
      reload: reloadGames,
      upsert: upsertGame,
      removeLocal: removeGameLocal,
      byId: (id) => games?.find((g) => g.id === id),
    }),
    [games, gamesError, reloadGames, upsertGame, removeGameLocal],
  );

  const sessionsValue = useMemo<SessionsValue>(() => {
    const sessions = Object.values(sessionMap).sort((a, b) => b.startedAt - a.startedAt);
    return {
      sessions,
      latestFor: (gameId) => sessions.find((s) => s.gameId === gameId),
      activeFor: (gameId) => sessions.find((s) => s.gameId === gameId && isActiveSession(s)),
      upsert: upsertSession,
    };
  }, [sessionMap, upsertSession]);

  const tasksValue = useMemo<TasksValue>(() => ({ tasks: Object.values(taskMap), dismiss: dismissTask }), [taskMap, dismissTask]);

  const languagesValue = useMemo<LanguagesValue>(
    () => ({
      languages,
      error: languagesError,
      nameFor: (code) => {
        if (!code) return null;
        const l = languages?.find((x) => x.code === code);
        return l ? l.name : code;
      },
    }),
    [languages, languagesError],
  );

  return (
    <SettingsContext.Provider value={settingsValue}>
      <LanguagesContext.Provider value={languagesValue}>
        <LibraryContext.Provider value={libraryValue}>
          <SessionsContext.Provider value={sessionsValue}>
            <TasksContext.Provider value={tasksValue}>{children}</TasksContext.Provider>
          </SessionsContext.Provider>
        </LibraryContext.Provider>
      </LanguagesContext.Provider>
    </SettingsContext.Provider>
  );
}

function must<T>(v: T | null, name: string): T {
  if (v === null) throw new Error(`${name} must be used inside <AppStateProvider>`);
  return v;
}

export const useAppSettings = () => must(useContext(SettingsContext), "useAppSettings");
export const useLibrary = () => must(useContext(LibraryContext), "useLibrary");
export const useSessions = () => must(useContext(SessionsContext), "useSessions");
export const useTasks = () => must(useContext(TasksContext), "useTasks");
export const useLanguages = () => must(useContext(LanguagesContext), "useLanguages");
