import { CircleCheck, CircleX, FolderOpen, RefreshCw } from "lucide-react";
import { useState } from "react";
import { LogViewer } from "../../components/LogViewer";
import { Button, IconButton } from "../../components/ui/Button";
import { Badge, CopyButton, ErrorState, KeyValueList, LoadingBlock, Section } from "../../components/ui/feedback";
import type { DependencyCategory, DependencyStatus } from "../../contract";
import { useI18n } from "../../i18n";
import { isDemoMode } from "../../lib/api";
import { openInFileManager } from "../../lib/dialogs";
import { useCommand } from "../../lib/hooks";
import { useToasts } from "../../state/toasts";

const CATEGORY_ORDER: DependencyCategory[] = ["runtime", "graphics", "translation", "ocr", "packaging", "other"];

function guessDistro(report: string | null | undefined): string | null {
  const os = (report ?? "").toLowerCase();
  if (/arch|manjaro|endeavour|cachy|garuda/.test(os)) return "arch";
  if (/debian|ubuntu|mint|pop!?_?os|elementary|zorin/.test(os)) return "debian";
  if (/fedora|nobara|bazzite|rhel|centos/.test(os)) return "fedora";
  return null;
}

export function DiagnosticsPage() {
  const { t } = useI18n();
  const deps = useCommand("diagnostics_dependencies", {});
  const system = useCommand("diagnostics_system", {});
  const caps = useCommand("graphics_capabilities", {});
  const info = useCommand("app_info", {});
  const distro = guessDistro(system.data?.os);
  const [missingOnly, setMissingOnly] = useState(false);

  const grouped = CATEGORY_ORDER.map((cat) => [cat, (deps.data ?? []).filter((d) => d.category === cat && (!missingOnly || !d.found))] as const).filter(([, list]) => list.length > 0);
  const missingRequired = (deps.data ?? []).filter((d) => d.required && !d.found);
  const missingTotal = (deps.data ?? []).filter((d) => !d.found).length;

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>{t("diag.title")}</h1>
          <p className="page-sub">{t("diag.subtitle")}</p>
        </div>
      </header>

      <Section
        title={t("diag.dependencies")}
        description={deps.data ? t("diag.depsSummary", { found: deps.data.length - missingTotal, total: deps.data.length }) : undefined}
        actions={
          <div className="row gap-sm">
            <button type="button" className={missingOnly ? "chip is-active" : "chip"} aria-pressed={missingOnly} onClick={() => setMissingOnly(!missingOnly)}>
              {t("diag.missingOnly")}
            </button>
            <IconButton icon={RefreshCw} size="sm" label={t("common.refresh")} onClick={() => void deps.reload()} />
          </div>
        }
      >
        {deps.error && <ErrorState error={deps.error} onRetry={() => void deps.reload()} />}
        {deps.loading && <LoadingBlock />}
        {missingRequired.length > 0 && <ErrorState error={{ code: "missingDependency", message: t("diag.missingRequired", { names: missingRequired.map((d) => d.name).join(", ") }), details: null }} />}
        {grouped.map(([cat, list]) => (
          <div className="dep-group" key={cat}>
            <h3>{t(`diag.cat.${cat}`)}</h3>
            <table className="table dep-table">
              <thead>
                <tr>
                  <th>{t("diag.dep")}</th>
                  <th>{t("diag.status")}</th>
                  <th>{t("diag.version")}</th>
                  <th>{t("diag.purpose")}</th>
                </tr>
              </thead>
              <tbody>
                {list.map((d) => (
                  <DependencyRow key={d.id} dep={d} distro={distro} />
                ))}
              </tbody>
            </table>
          </div>
        ))}
      </Section>

      <div className="grid-2 align-start">
        <Section title={t("diag.system")} actions={system.data && <CopyButton text={JSON.stringify({ system: system.data, graphics: caps.data, app: info.data }, null, 2)} label={t("diag.copyReport")} />}>
          {system.error && <ErrorState error={system.error} compact onRetry={() => void system.reload()} />}
          {system.loading && <LoadingBlock />}
          {system.data && (
            <KeyValueList
              items={[
                [t("diag.os"), system.data.os ?? "—"],
                [t("diag.kernel"), system.data.kernel ?? "—"],
                [t("diag.desktop"), `${system.data.desktop ?? "—"} · ${system.data.sessionType}`],
                [t("diag.flatpak"), system.data.insideFlatpak ? t("common.yes") : t("common.no")],
                [t("diag.cpu"), system.data.cpu ?? "—"],
                [t("diag.memory"), system.data.memoryMb ? `${(system.data.memoryMb / 1024).toFixed(1)} GiB` : "—"],
                [t("diag.gpus"), system.data.gpus.join(", ") || "—"],
                ["Vulkan", system.data.vulkan ?? t("diag.notDetected")],
              ]}
            />
          )}
        </Section>

        <Section title={t("diag.graphics")}>
          {caps.error && <ErrorState error={caps.error} compact onRetry={() => void caps.reload()} />}
          {caps.loading && <LoadingBlock />}
          {caps.data && (
            <>
              <KeyValueList
                items={[
                  [t("diag.session"), `${caps.data.sessionType}${caps.data.xwayland ? " + XWayland" : ""}`],
                  ["gamescope", caps.data.gamescope ? `${caps.data.gamescope.version ?? "?"} · ${caps.data.gamescope.path}` : t("diag.notDetected")],
                  ["RT-Upscaler", caps.data.rtUpscaler ? `${caps.data.rtUpscaler.version ?? "?"} · ${caps.data.rtUpscaler.path}` : t("diag.notDetected")],
                  [t("diag.monitors"), caps.data.monitors.map((m) => `${m.name}${m.width ? ` ${m.width}×${m.height}` : ""}${m.refreshHz ? `@${Math.round(m.refreshHz)}` : ""}${m.primary ? " ★" : ""}`).join(", ") || "—"],
                  [t("diag.gpus"), caps.data.gpus.join(", ") || "—"],
                ]}
              />
              {caps.data.notes.length > 0 && (
                <ul className="plain-list muted small">
                  {caps.data.notes.map((n) => (
                    <li key={n}>{n}</li>
                  ))}
                </ul>
              )}
            </>
          )}
        </Section>
      </div>

      <Section title={t("diag.paths")}>
        {info.error && <ErrorState error={info.error} compact onRetry={() => void info.reload()} />}
        {info.data && (
          <KeyValueList
            items={[
              [t("diag.version"), `${info.data.version}${isDemoMode() ? ` (${t("demo.badge")})` : ""}`],
              [t("diag.dataDir"), <PathValue key="d" path={info.data.dataDir} />],
              [t("diag.configDir"), <PathValue key="c" path={info.data.configDir} />],
              [t("diag.cacheDir"), <PathValue key="k" path={info.data.cacheDir} />],
              [t("diag.db"), <PathValue key="b" path={info.data.dbPath} noOpen />],
            ]}
          />
        )}
      </Section>

      <Section title={t("logs.title")} description={t("logs.desc")}>
        <LogViewer />
      </Section>
    </div>
  );
}

function PathValue({ path, noOpen }: { path: string; noOpen?: boolean }) {
  const { t } = useI18n();
  const toasts = useToasts();
  return (
    <span className="row gap-xs">
      <code className="break">{path}</code>
      <CopyButton text={path} />
      {!noOpen && <IconButton icon={FolderOpen} size="sm" label={t("game.openFolder")} onClick={() => openInFileManager(path).then((ok) => !ok && toasts.info(t("demo.noFileManager"), path)).catch((e: unknown) => toasts.error(e))} />}
    </span>
  );
}

function DependencyRow({ dep, distro }: { dep: DependencyStatus; distro: string | null }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const hints = distro ? [...dep.installHints].sort((a, b) => (a.distro === distro ? -1 : b.distro === distro ? 1 : 0)) : dep.installHints;
  return (
    <>
      <tr className={dep.found ? undefined : "is-missing"}>
        <td>
          <strong>{dep.name}</strong> {dep.required && <Badge tone="accent">{t("diag.required")}</Badge>}
          {dep.path && <div className="muted small mono">{dep.path}</div>}
        </td>
        <td>
          {dep.found ? (
            <Badge tone="success" icon={CircleCheck}>
              {t("diag.found")}
            </Badge>
          ) : (
            <Badge tone={dep.required ? "danger" : "warning"} icon={CircleX}>
              {t("diag.missing")}
            </Badge>
          )}
        </td>
        <td className="mono small">{dep.version ?? "—"}</td>
        <td className="small">
          {dep.purpose}
          {!dep.found && hints.length > 0 && (
            <div>
              <Button size="sm" variant="ghost" onClick={() => setOpen(!open)} aria-expanded={open}>
                {open ? t("diag.hideInstall") : t("diag.howToInstall")}
              </Button>
            </div>
          )}
        </td>
      </tr>
      {open && (
        <tr className="hint-row">
          <td colSpan={4}>
            <ul className="install-hints">
              {hints.map((h) => (
                <li key={`${h.distro}-${h.command}`}>
                  <Badge tone={h.distro === distro ? "accent" : "neutral"}>{h.distro}</Badge>
                  <code>{h.command}</code>
                  <CopyButton text={h.command} />
                </li>
              ))}
            </ul>
          </td>
        </tr>
      )}
    </>
  );
}
