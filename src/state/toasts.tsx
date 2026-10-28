import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import type { AppError } from "../contract";
import { normalizeError } from "../lib/api";

export type ToastKind = "info" | "success" | "warning" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  message?: string | null;
  details?: string | null;
  code?: AppError["code"];
}

interface ToastApi {
  push: (t: Omit<Toast, "id">) => number;
  dismiss: (id: number) => void;
  success: (title: string, message?: string) => void;
  info: (title: string, message?: string) => void;
  /** Show any rejection as an error toast. `context` is a short "what failed" title. */
  error: (error: unknown, context?: string) => AppError;
}

const ToastContext = createContext<ToastApi | null>(null);
const ToastListContext = createContext<Toast[]>([]);

const AUTO_DISMISS_MS: Record<ToastKind, number | null> = { info: 4500, success: 3500, warning: 7000, error: 9000 };

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  const dismiss = useCallback((id: number) => {
    setToasts((ts) => ts.filter((t) => t.id !== id));
    const timer = timers.current.get(id);
    if (timer) clearTimeout(timer);
    timers.current.delete(id);
  }, []);

  const push = useCallback(
    (t: Omit<Toast, "id">) => {
      const id = nextId.current++;
      setToasts((ts) => [...ts.slice(-4), { ...t, id }]);
      const ms = AUTO_DISMISS_MS[t.kind];
      if (ms) timers.current.set(id, setTimeout(() => dismiss(id), ms));
      return id;
    },
    [dismiss],
  );

  const api = useMemo<ToastApi>(
    () => ({
      push,
      dismiss,
      success: (title, message) => void push({ kind: "success", title, message }),
      info: (title, message) => void push({ kind: "info", title, message }),
      error: (error, context) => {
        const e = normalizeError(error);
        push({ kind: e.code === "notImplemented" ? "warning" : "error", title: context ?? e.message, message: context ? e.message : null, details: e.details, code: e.code });
        return e;
      },
    }),
    [push, dismiss],
  );

  return (
    <ToastContext.Provider value={api}>
      <ToastListContext.Provider value={toasts}>{children}</ToastListContext.Provider>
    </ToastContext.Provider>
  );
}

export function useToasts(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToasts must be used inside <ToastProvider>");
  return ctx;
}

export function useToastList(): Toast[] {
  return useContext(ToastListContext);
}
