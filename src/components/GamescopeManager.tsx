import { Download, Hammer, RefreshCw, Square, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { AppError, GamescopeBinary, LogEntry } from "../contract";
import { GAMESCOPE_BUILD_LOG_SOURCE, GAMESCOPE_BUILD_TASK } from "../contract/graphics";
import { useT } from "../i18n";
import { call, normalizeError } from "../lib/api";
import { useCommand, useDebounced, useEvent } from "../lib/hooks";
import { useTasks } from "../state/app";
import { useToasts } from "../state/toasts";
import { Button } from "./ui/Button";
import { Badge, Callout, ErrorState, ProgressBar, Spinner } from "./ui/feedback";
import { ConfirmDialog } from "./ui/Modal";
import { PathField } from "./ui/form";

const MAX_LOG_LINES = 400;

interface Props {
  /** Application-wide custom gamescope path (part of the settings draft). */
  customPath: string | null;
  onCustomPathChange: (path: string | null) => void;
}

function BinaryRow({ bin }: { bin: GamescopeBinary }) {
  const t = useT();
  return (
    <li className="gs-binary">
      <div className="row gap-sm wrap">
        <Badge tone={bin.source === "managed" ? "accent" : "neutral"}>{t(`gamescope.source.${bin.source}`)}</Badge>
        {bin.version && <span className="text-muted">{bin.version}</span>}
        <Badge tone={bin.supportsAnime4k ? "success" : "neutral"}>{bin.supportsAnime4k ? t("gamescope.anime4kYes") : t("gamescope.anime4kNo")}</Badge>
      </div>
      <code className="mono small">{bin.path}</code>
      {bin.filters.length > 0 && (
        <div className="text-muted small">
          {t("gamescope.filters")}: {bin.filters.join(", ")}
        </div>
      )}
    </li>
  );
}

/** Gamescope binaries, custom path and the optional guided build of the Anime4K fork. */
export function GamescopeManager({ customPath, onCustomPathChange }: Props) {
  const t = useT();
  const toasts = useToasts();
  const { tasks } = useTasks();
  const status = useCommand("gamescope_status", {});
  const reload = status.reload;
  const [lines, setLines] = useState<LogEntry[]>([]);
  const [starting, setStarting] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);

  const task = tasks.find((x) => x.taskId === GAMESCOPE_BUILD_TASK);
  const running = (task?.state === "running") || (task === undefined && status.data?.build.running === true);

  // Past output (the build may have started before this page was opened).
  useEffect(() => {
    let cancelled = false;
    void call("logs_recent", { limit: 600 })
      .then((all) => {
        if (!cancelled) setLines(all.filter((l) => l.source === GAMESCOPE_BUILD_LOG_SOURCE).slice(-MAX_LOG_LINES));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  useEvent("vnl://log", (entry) => {
    if (entry.source === GAMESCOPE_BUILD_LOG_SOURCE) setLines((prev) => [...prev, entry].slice(-MAX_LOG_LINES));
  });

  // A finished build changes what is installed.
  const finishedState = task && task.state !== "running" ? task.state : null;
  useEffect(() => {
    if (finishedState) void reload();
  }, [finishedState, reload]);

  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines]);

  // Validate the custom path as it is typed/picked. The result is tied to the
  // path it was computed for, so a stale answer is never shown for a new path.
  const [inspection, setInspection] = useState<{ path: string; bin: GamescopeBinary | null; error: AppError | null } | null>(null);
  const debouncedPath = useDebounced((customPath ?? "").trim(), 400);
  useEffect(() => {
    if (!debouncedPath) return;
    let cancelled = false;
    call("gamescope_inspect_path", { path: debouncedPath })
      .then((bin) => !cancelled && setInspection({ path: debouncedPath, bin, error: null }))
      .catch((e) => !cancelled && setInspection({ path: debouncedPath, bin: null, error: normalizeError(e) }));
    return () => {
      cancelled = true;
    };
  }, [debouncedPath]);
  const current = inspection && inspection.path === (customPath ?? "").trim() ? inspection : null;
  const inspected = current?.bin ?? null;
  const inspectError = current?.error ?? null;

  const start = async () => {
    setStarting(true);
    setLines([]);
    try {
      await call("gamescope_build_start", {});
      void reload();
    } catch (e) {
      toasts.error(e, t("gamescope.buildFailedToStart"));
    } finally {
      setStarting(false);
    }
  };

  const cancel = async () => {
    try {
      await call("gamescope_build_cancel", {});
    } catch (e) {
      toasts.error(e, t("gamescope.cancelFailed"));
    }
  };

  const remove = async () => {
    setConfirmRemove(false);
    try {
      await call("gamescope_managed_remove", {});
      toasts.success(t("gamescope.removed"));
      void reload();
    } catch (e) {
      toasts.error(e, t("gamescope.removeFailed"));
    }
  };

  const s = status.data;
  const req = s?.requirements;

  return (
    <div className="stack gap-md">
      {status.error && !s && <ErrorState error={status.error} onRetry={() => void reload()} compact />}
      {!s && !status.error && <Spinner label={t("common.loading")} />}

      {s && (
        <>
          <div>
            <div className="row gap-sm spread">
              <h3 className="h3">{t("gamescope.binaries")}</h3>
              <Button size="sm" icon={RefreshCw} onClick={() => void reload()} loading={status.refreshing}>
                {t("common.refresh")}
              </Button>
            </div>
            {s.binaries.length === 0 ? <Callout tone="warning">{t("gamescope.noneFound")}</Callout> : <ul className="plain-list stack gap-sm">{s.binaries.map((b) => <BinaryRow key={b.path} bin={b} />)}</ul>}
            <p className="text-muted small">{t("gamescope.anime4kStatus", { state: s.anime4kAvailable ? t("gamescope.anime4kAvailable") : t("gamescope.anime4kUnavailable") })}</p>
          </div>

          <div>
            <PathField label={t("gamescope.customPath")} clearable value={customPath ?? ""} onChange={(v) => onCustomPathChange(v.trim() ? v : null)} hint={t("gamescope.customPathHint")} placeholder="/opt/gamescope/bin/gamescope" />
            {inspectError && <p className="text-danger small" role="alert">{inspectError.message}</p>}
            {inspected && (
              <p className="small text-muted">
                {t("gamescope.customOk", { version: inspected.version ?? "?" })} {inspected.supportsAnime4k ? t("gamescope.supportsAnime4k") : t("gamescope.noAnime4k")}
              </p>
            )}
          </div>

          <div className="stack gap-sm">
            <h3 className="h3">{t("gamescope.managedTitle")}</h3>
            <p className="text-muted">{t("gamescope.managedDesc")}</p>
            {s.managed.installed && s.managed.manifest && (
              <Callout tone="success" title={t("gamescope.managedInstalled")}>
                <div className="small">
                  {s.repoUrl.replace(/\.git$/, "")} · {s.managed.manifest.commit.slice(0, 10)}
                </div>
                <code className="mono small">{s.managed.dir}</code>
              </Callout>
            )}

            {req && !req.ok && (
              <Callout tone="warning" title={t("gamescope.missingDeps", { n: req.missing.length })}>
                <div className="small">{req.missing.join(", ")}</div>
                {req.hints.map((h) => (
                  <div key={h.distro} className="small">
                    <strong>{h.distro}</strong>: <code className="mono">{h.command}</code>
                  </div>
                ))}
              </Callout>
            )}

            <div className="row gap-sm wrap">
              {running ? (
                <Button variant="danger" icon={Square} onClick={() => void cancel()}>
                  {t("gamescope.cancelBuild")}
                </Button>
              ) : (
                <Button variant="primary" icon={s.managed.installed ? Hammer : Download} onClick={() => void start()} loading={starting} disabled={!req?.ok}>
                  {s.managed.installed ? t("gamescope.rebuild") : t("gamescope.build")}
                </Button>
              )}
              {s.managed.installed && !running && (
                <Button icon={Trash2} onClick={() => setConfirmRemove(true)}>
                  {t("gamescope.remove")}
                </Button>
              )}
            </div>
            <p className="text-muted small">{t("gamescope.buildNote")}</p>

            {(running || task) && task && (
              <div>
                <ProgressBar value={task.state === "running" ? task.progress : task.state === "done" ? 1 : null} label={t("gamescope.progress")} />
                <div className={task.state === "failed" ? "text-danger small" : "small text-muted"} role="status">
                  {task.message}
                </div>
              </div>
            )}
            {lines.length > 0 && (
              <pre ref={logRef} className="log-box mono small" aria-label={t("gamescope.log")} style={{ maxHeight: 260, overflow: "auto" }}>
                {lines.map((l) => l.message).join("\n")}
              </pre>
            )}
          </div>
        </>
      )}

      <ConfirmDialog open={confirmRemove} title={t("gamescope.removeTitle")} confirmLabel={t("gamescope.remove")} cancelLabel={t("common.cancel")} variant="danger" onConfirm={() => void remove()} onCancel={() => setConfirmRemove(false)}>
        {t("gamescope.removeBody")}
      </ConfirmDialog>
    </div>
  );
}
