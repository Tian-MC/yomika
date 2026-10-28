import { Activity, BookOpen, CircleAlert, CircleCheck, Info, Languages, Library, LoaderCircle, Search, Settings, Square, Stethoscope, TriangleAlert, Wine, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Link, NavLink, Outlet, useLocation, useNavigate, useSearchParams } from "react-router-dom";
import { useI18n } from "../../i18n";
import { call, isDemoMode } from "../../lib/api";
import { formatElapsed } from "../../lib/format";
import { useNow } from "../../lib/hooks";
import { isActiveSession, useAppSettings, useLibrary, useSessions, useTasks } from "../../state/app";
import { useToastList, useToasts } from "../../state/toasts";
import { IconButton } from "../ui/Button";
import { ProgressBar } from "../ui/feedback";

export function AppLayout() {
  useThemeEffect();
  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">
        Skip to content
      </a>
      <Sidebar />
      <div className="app-main">
        <TopBar />
        <main id="main-content" className="app-content" tabIndex={-1}>
          <Outlet />
        </main>
      </div>
      <TaskTray />
      <ToastViewport />
    </div>
  );
}

function useThemeEffect() {
  const { appSettings } = useAppSettings();
  const theme = appSettings?.theme ?? "system";
  useEffect(() => {
    const root = document.documentElement;
    const mq = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: light)") : null;
    const apply = () => {
      root.dataset.theme = theme === "system" ? (mq?.matches ? "light" : "dark") : theme;
    };
    apply();
    mq?.addEventListener?.("change", apply);
    return () => mq?.removeEventListener?.("change", apply);
  }, [theme]);
}

function Sidebar() {
  const { t } = useI18n();
  const { sessions } = useSessions();
  const running = sessions.filter(isActiveSession).length;
  const items = [
    { to: "/", label: t("nav.library"), icon: Library, end: true },
    { to: "/translation", label: t("nav.translation"), icon: Languages },
    { to: "/runtime", label: t("nav.runtime"), icon: Wine },
    { to: "/diagnostics", label: t("nav.diagnostics"), icon: Stethoscope },
    { to: "/settings", label: t("nav.settings"), icon: Settings },
  ];
  return (
    <nav className="sidebar" aria-label={t("nav.main")}>
      <Link to="/" className="brand" aria-label="VN Launcher">
        <span className="brand-mark" aria-hidden>
          <BookOpen size={18} />
        </span>
        <span className="brand-text">
          VN Launcher
          <small lang="ja">ノベルランチャー</small>
        </span>
      </Link>
      <ul className="nav-list">
        {items.map(({ to, label, icon: Icon, end }) => (
          <li key={to}>
            <NavLink to={to} end={end} className={({ isActive }) => (isActive ? "nav-item is-active" : "nav-item")} title={label}>
              <Icon size={18} aria-hidden />
              <span className="nav-label">{label}</span>
              {to === "/" && running > 0 && <span className="nav-dot" aria-label={t("session.runningCount", { n: running })} />}
            </NavLink>
          </li>
        ))}
      </ul>
      <div className="sidebar-foot">{isDemoMode() && <DemoBadge />}</div>
    </nav>
  );
}

export function DemoBadge() {
  const { t } = useI18n();
  return (
    <div className="demo-badge" role="note" title={t("demo.tooltip")}>
      <Info size={13} aria-hidden />
      <span>{t("demo.badge")}</span>
    </div>
  );
}

function TopBar() {
  const { t } = useI18n();
  const location = useLocation();
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const onLibrary = location.pathname === "/";
  const q = onLibrary ? (params.get("q") ?? "") : "";
  const [text, setText] = useState(q);
  const [syncedQ, setSyncedQ] = useState(q);
  if (syncedQ !== q) {
    setSyncedQ(q);
    setText(q);
  }
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const typing = target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT" || target.isContentEditable);
      if ((e.key === "k" && (e.ctrlKey || e.metaKey)) || (e.key === "/" && !typing)) {
        e.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const apply = (value: string) => {
    setText(value);
    if (onLibrary) {
      const next = new URLSearchParams(params);
      if (value) next.set("q", value);
      else next.delete("q");
      setParams(next, { replace: true });
    } else {
      navigate(value ? `/?q=${encodeURIComponent(value)}` : "/");
    }
  };

  return (
    <header className="topbar">
      <div className="search-box">
        <Search size={16} aria-hidden />
        <input
          ref={input}
          type="search"
          placeholder={t("search.placeholder")}
          aria-label={t("search.label")}
          value={text}
          onChange={(e) => apply(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && text) {
              e.preventDefault();
              apply("");
            }
          }}
        />
        {text ? <IconButton icon={X} size="sm" label={t("common.clear")} onClick={() => apply("")} /> : <kbd className="kbd">Ctrl K</kbd>}
      </div>
      <div className="topbar-right">
        <SessionIndicator />
      </div>
    </header>
  );
}

function SessionIndicator() {
  const { t } = useI18n();
  const { sessions } = useSessions();
  const library = useLibrary();
  const toasts = useToasts();
  const active = sessions.filter(isActiveSession);
  const now = useNow(1000, active.length > 0);
  if (active.length === 0) return null;
  return (
    <div className="session-indicator" role="status" aria-label={t("session.runningCount", { n: active.length })}>
      {active.map((s) => {
        const game = library.byId(s.gameId);
        return (
          <div className={`session-pill state-${s.state}`} key={s.id}>
            <span className="pulse-dot" aria-hidden />
            <Link to={`/game/${s.gameId}`} className="session-title">
              {game?.title ?? t("session.gameN", { id: s.gameId })}
            </Link>
            <span className="session-meta">{s.state === "running" ? formatElapsed(now / 1000 - s.startedAt) : t(`session.state.${s.state}`)}</span>
            <IconButton
              icon={Square}
              size="sm"
              label={t("session.stop")}
              disabled={s.state === "stopping"}
              onClick={() => call("session_stop", { sessionId: s.id, force: false }).catch((e: unknown) => toasts.error(e, t("session.stopFailed")))}
            />
          </div>
        );
      })}
    </div>
  );
}

function TaskTray() {
  const { t } = useI18n();
  const { tasks, dismiss } = useTasks();
  if (tasks.length === 0) return null;
  return (
    <aside className="task-tray" aria-label={t("tasks.title")} aria-live="polite">
      {tasks.map((task) => (
        <div className={`task-item state-${task.state}`} key={task.taskId}>
          <div className="task-head">
            {task.state === "running" ? <LoaderCircle size={15} className="spin" aria-hidden /> : task.state === "done" ? <CircleCheck size={15} aria-hidden /> : <CircleAlert size={15} aria-hidden />}
            <span className="task-label">{task.label}</span>
            {task.state !== "running" && <IconButton icon={X} size="sm" label={t("common.dismiss")} onClick={() => dismiss(task.taskId)} />}
          </div>
          {task.state === "running" && <ProgressBar value={task.progress} label={task.label} />}
          {task.message && <div className="task-msg">{task.message}</div>}
        </div>
      ))}
    </aside>
  );
}

function ToastViewport() {
  const { t } = useI18n();
  const toasts = useToastList();
  const { dismiss } = useToasts();
  return (
    <div className="toast-viewport" role="region" aria-label={t("toasts.label")}>
      {toasts.map((toast) => {
        const Icon = toast.kind === "success" ? CircleCheck : toast.kind === "error" ? CircleAlert : toast.kind === "warning" ? TriangleAlert : Activity;
        return (
          <div className={`toast toast-${toast.kind}`} key={toast.id} role={toast.kind === "error" ? "alert" : "status"}>
            <Icon size={18} className="toast-icon" aria-hidden />
            <div className="toast-body">
              <div className="toast-title">{toast.title}</div>
              {toast.code === "notImplemented" && <div className="toast-msg">{t("error.notImplementedHint")}</div>}
              {toast.message && <div className="toast-msg">{toast.message}</div>}
              {toast.details && (
                <details className="toast-details">
                  <summary>{t("common.details")}</summary>
                  <pre>{toast.details}</pre>
                </details>
              )}
            </div>
            <IconButton icon={X} size="sm" label={t("common.dismiss")} onClick={() => dismiss(toast.id)} />
          </div>
        );
      })}
    </div>
  );
}
