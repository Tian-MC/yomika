import { Check, CircleAlert, CircleCheck, Copy, Info, LoaderCircle, TriangleAlert, type LucideIcon } from "lucide-react";
import { useState, type ReactNode } from "react";
import type { AppError } from "../../contract";
import { useT } from "../../i18n";
import { Button } from "./Button";

export function Spinner({ label, size = 18 }: { label?: string; size?: number }) {
  return (
    <span className="spinner" role="status" aria-live="polite">
      <LoaderCircle size={size} className="spin" aria-hidden />
      {label ? <span>{label}</span> : <span className="sr-only">Loading</span>}
    </span>
  );
}

export function LoadingBlock({ label }: { label?: string }) {
  const t = useT();
  return (
    <div className="loading-block">
      <Spinner label={label ?? t("common.loading")} />
    </div>
  );
}

export type CalloutTone = "info" | "warning" | "danger" | "success" | "neutral";

const TONE_ICON: Record<CalloutTone, LucideIcon> = {
  info: Info,
  warning: TriangleAlert,
  danger: CircleAlert,
  success: CircleCheck,
  neutral: Info,
};

export function Callout({ tone = "info", title, children, action, icon, role }: { tone?: CalloutTone; title?: ReactNode; children?: ReactNode; action?: ReactNode; icon?: LucideIcon; role?: "alert" | "status" }) {
  const Icon = icon ?? TONE_ICON[tone];
  return (
    <div className={`callout callout-${tone}`} role={role}>
      <Icon size={18} className="callout-icon" aria-hidden />
      <div className="callout-body">
        {title && <div className="callout-title">{title}</div>}
        {children && <div className="callout-text">{children}</div>}
      </div>
      {action && <div className="callout-action">{action}</div>}
    </div>
  );
}

export function EmptyState({ icon: Icon, title, children, action }: { icon?: LucideIcon; title: ReactNode; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty-state">
      {Icon && (
        <div className="empty-icon">
          <Icon size={28} aria-hidden />
        </div>
      )}
      <h3>{title}</h3>
      {children && <div className="empty-text">{children}</div>}
      {action && <div className="empty-action">{action}</div>}
    </div>
  );
}

/** Human-friendly message for an AppError code. */
export function useErrorTitle() {
  const t = useT();
  return (e: AppError) => {
    switch (e.code) {
      case "notImplemented":
        return t("error.notImplemented");
      case "missingDependency":
        return t("error.missingDependency");
      case "network":
        return t("error.network");
      case "notFound":
        return t("error.notFound");
      case "unsupported":
        return t("error.unsupported");
      default:
        return t("error.generic");
    }
  };
}

export function ErrorState({ error, onRetry, compact }: { error: AppError; onRetry?: () => void; compact?: boolean }) {
  const t = useT();
  const titleFor = useErrorTitle();
  const tone: CalloutTone = error.code === "notImplemented" || error.code === "unsupported" ? "warning" : "danger";
  return (
    <div className={compact ? "error-state compact" : "error-state"}>
      <Callout tone={tone} title={titleFor(error)} role="alert" action={onRetry ? <Button size="sm" onClick={onRetry}>{t("common.retry")}</Button> : undefined}>
        <span>{error.message}</span>
        {error.details && (
          <details className="error-details">
            <summary>{t("common.details")}</summary>
            <pre>{error.details}</pre>
          </details>
        )}
      </Callout>
    </div>
  );
}

export type BadgeTone = "neutral" | "accent" | "success" | "warning" | "danger" | "info" | "rose";

export function Badge({ tone = "neutral", children, icon: Icon, title }: { tone?: BadgeTone; children: ReactNode; icon?: LucideIcon; title?: string }) {
  return (
    <span className={`badge badge-${tone}`} title={title}>
      {Icon && <Icon size={12} aria-hidden />}
      {children}
    </span>
  );
}

export function ProgressBar({ value, label }: { value: number | null; label?: string }) {
  const pct = value === null ? null : Math.round(Math.min(1, Math.max(0, value)) * 100);
  return (
    <div
      className={pct === null ? "progress indeterminate" : "progress"}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct ?? undefined}
    >
      <div className="progress-fill" style={pct === null ? undefined : { width: `${pct}%` }} />
    </div>
  );
}

export function CopyButton({ text, label, size = "sm" }: { text: string; label?: string; size?: "sm" | "md" }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Fallback for environments without the async clipboard API.
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand("copy");
      } catch {
        /* ignore */
      }
      ta.remove();
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <Button size={size} variant="ghost" icon={copied ? Check : Copy} onClick={() => void copy()} aria-label={label ?? t("common.copy")} title={label ?? t("common.copy")}>
      {copied ? t("common.copied") : undefined}
    </Button>
  );
}

export function Section({ title, description, actions, children, id }: { title: ReactNode; description?: ReactNode; actions?: ReactNode; children: ReactNode; id?: string }) {
  return (
    <section className="section" aria-labelledby={id}>
      <header className="section-head">
        <div>
          <h2 id={id}>{title}</h2>
          {description && <p className="section-desc">{description}</p>}
        </div>
        {actions && <div className="section-actions">{actions}</div>}
      </header>
      <div className="section-body">{children}</div>
    </section>
  );
}

export function KeyValueList({ items }: { items: Array<[ReactNode, ReactNode]> }) {
  return (
    <dl className="kv-list">
      {items.map(([k, v], i) => (
        <div className="kv-row" key={i}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  );
}
