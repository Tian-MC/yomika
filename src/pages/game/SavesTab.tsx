import { Archive, FolderOpen, RefreshCw, RotateCcw, Save, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { Button, IconButton } from "../../components/ui/Button";
import { Badge, Callout, EmptyState, ErrorState, LoadingBlock, Section } from "../../components/ui/feedback";
import { StringListEditor, TextField } from "../../components/ui/form";
import { ConfirmDialog } from "../../components/ui/Modal";
import type { Game, SaveBackup, SaveLocation } from "../../contract";
import { useI18n } from "../../i18n";
import { call } from "../../lib/api";
import { openInFileManager } from "../../lib/dialogs";
import { formatBytes, formatDateTime } from "../../lib/format";
import { useCommand } from "../../lib/hooks";
import { useSessions } from "../../state/app";
import { useToasts } from "../../state/toasts";

export function SavesTab({ game }: { game: Game }) {
  const { t, lang } = useI18n();
  const toasts = useToasts();
  const { activeFor } = useSessions();
  const running = !!activeFor(game.id);
  const locations = useCommand("saves_detect", { gameId: game.id });
  const backups = useCommand("saves_list_backups", { gameId: game.id });

  const manualFromServer = (locations.data ?? []).filter((l) => l.source === "manual").map((l) => l.path);
  const [manual, setManual] = useState<string[]>([]);
  const [manualBase, setManualBase] = useState<SaveLocation[] | undefined>(undefined);
  if (locations.data !== manualBase) {
    setManualBase(locations.data);
    setManual(manualFromServer);
  }
  const manualDirty = JSON.stringify(manual.map((p) => p.trim()).filter(Boolean)) !== JSON.stringify(manualFromServer);
  const [savingLocations, setSavingLocations] = useState(false);

  const [label, setLabel] = useState("");
  const [creating, setCreating] = useState(false);
  const [restoreTarget, setRestoreTarget] = useState<SaveBackup | null>(null);
  const [restoring, setRestoring] = useState(false);

  const saveLocations = async () => {
    setSavingLocations(true);
    try {
      locations.mutate(await call("saves_set_locations", { gameId: game.id, paths: manual.map((p) => p.trim()).filter(Boolean) }));
      toasts.success(t("saves.locationsSaved"));
    } catch (e) {
      toasts.error(e, t("saves.locationsFailed"));
    } finally {
      setSavingLocations(false);
    }
  };

  const createBackup = async () => {
    setCreating(true);
    try {
      const b = await call("saves_create_backup", { gameId: game.id, label: label.trim() || null });
      backups.mutate((list) => [b, ...(list ?? [])]);
      setLabel("");
      toasts.success(t("saves.backupCreated"), `${formatBytes(b.sizeBytes)} · ${t("saves.files", { n: b.fileCount })}`);
    } catch (e) {
      toasts.error(e, t("saves.backupFailed"));
    } finally {
      setCreating(false);
    }
  };

  const restore = async () => {
    if (!restoreTarget) return;
    setRestoring(true);
    try {
      // Only reached after the user explicitly confirmed in the dialog.
      const res = await call("saves_restore_backup", { backupId: restoreTarget.id, confirm: true });
      toasts.success(t("saves.restored", { n: res.filesRestored }), res.safetyBackup ? t("saves.safetyCreated", { date: formatDateTime(res.safetyBackup.createdAt, lang) }) : t("saves.noSafetyNeeded"));
      setRestoreTarget(null);
      void backups.reload();
    } catch (e) {
      toasts.error(e, t("saves.restoreFailed"));
    } finally {
      setRestoring(false);
    }
  };

  const open = async (path: string) => {
    try {
      if (!(await openInFileManager(path))) toasts.info(t("demo.noFileManager"), path);
    } catch (e) {
      toasts.error(e);
    }
  };

  return (
    <div className="stack gap-lg">
      <Section
        title={t("saves.locations")}
        description={t("saves.locationsDesc")}
        actions={
          <Button size="sm" variant="ghost" icon={RefreshCw} onClick={() => void locations.reload()} loading={locations.refreshing}>
            {t("saves.detect")}
          </Button>
        }
      >
        {locations.error && <ErrorState error={locations.error} compact onRetry={() => void locations.reload()} />}
        {locations.loading && <LoadingBlock />}
        {locations.data && locations.data.length === 0 && <p className="muted">{t("saves.noneDetected")}</p>}
        {locations.data && locations.data.length > 0 && (
          <ul className="location-list">
            {locations.data.map((l) => (
              <li key={`${l.source}-${l.path}`} className={l.exists ? "location" : "location is-missing"}>
                <div className="location-text">
                  <code>{l.path}</code>
                  <span className="muted small">{l.reason ?? (l.source === "manual" ? t("saves.manual") : "")}</span>
                </div>
                <Badge tone={l.source === "manual" ? "accent" : "neutral"}>{l.source === "manual" ? t("saves.manual") : t("saves.detected")}</Badge>
                <Badge tone={l.exists ? "success" : "warning"}>{l.exists ? t("saves.exists") : t("saves.missing")}</Badge>
                <IconButton icon={FolderOpen} size="sm" label={t("game.openFolder")} disabled={!l.exists} onClick={() => void open(l.path)} />
              </li>
            ))}
          </ul>
        )}
        <StringListEditor label={t("saves.manualLocations")} value={manual} onChange={setManual} browse="directory" placeholder="/path/to/savedata" />
        <div className="row gap-sm">
          <Button size="sm" icon={Save} onClick={() => void saveLocations()} disabled={!manualDirty} loading={savingLocations}>
            {t("saves.saveLocations")}
          </Button>
        </div>
      </Section>

      <Section title={t("saves.backups")} description={t("saves.backupsDesc")}>
        {running && <Callout tone="warning">{t("saves.runningWarning")}</Callout>}
        <div className="row gap-sm align-end">
          <div className="grow">
            <TextField label={t("saves.backupLabel")} value={label} onChange={setLabel} placeholder={t("saves.backupLabelPlaceholder")} onEnter={() => void createBackup()} />
          </div>
          <Button variant="primary" icon={Archive} onClick={() => void createBackup()} loading={creating}>
            {t("saves.createBackup")}
          </Button>
        </div>
        {backups.error && <ErrorState error={backups.error} compact onRetry={() => void backups.reload()} />}
        {backups.loading && <LoadingBlock />}
        {backups.data && backups.data.length === 0 && <EmptyState icon={Archive} title={t("saves.noBackups")} />}
        {backups.data && backups.data.length > 0 && (
          <ul className="backup-list" aria-label={t("saves.backups")}>
            {backups.data.map((b) => (
              <li key={b.id} className="backup">
                <div className="backup-text">
                  <strong>{b.label ?? t("saves.backupN", { id: b.id })}</strong>
                  <span className="muted small">
                    {formatDateTime(b.createdAt, lang)} · {formatBytes(b.sizeBytes)} · {t("saves.files", { n: b.fileCount })}
                  </span>
                  <span className="muted small mono ellipsis" title={b.sourcePaths.join("\n")}>
                    {b.sourcePaths.join(", ")}
                  </span>
                </div>
                {b.kind === "safety" && (
                  <Badge tone="info" icon={ShieldCheck}>
                    {t("saves.safety")}
                  </Badge>
                )}
                <Button size="sm" icon={RotateCcw} onClick={() => setRestoreTarget(b)} aria-label={t("saves.restoreNamed", { name: b.label ?? `#${b.id}` })}>
                  {t("saves.restore")}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </Section>

      <ConfirmDialog
        open={restoreTarget !== null}
        title={t("saves.restoreTitle")}
        confirmLabel={t("saves.restoreConfirm")}
        variant="danger"
        busy={restoring}
        onCancel={() => setRestoreTarget(null)}
        onConfirm={() => void restore()}
      >
        {restoreTarget && (
          <div className="stack gap-sm">
            <p>{t("saves.restoreText", { name: restoreTarget.label ?? `#${restoreTarget.id}`, date: formatDateTime(restoreTarget.createdAt, lang) })}</p>
            <Callout tone="info" icon={ShieldCheck} title={t("saves.safetyTitle")}>
              {t("saves.safetyText")}
            </Callout>
            {running && <Callout tone="warning">{t("saves.runningWarning")}</Callout>}
          </div>
        )}
      </ConfirmDialog>
    </div>
  );
}
