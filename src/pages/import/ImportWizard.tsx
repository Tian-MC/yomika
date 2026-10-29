import { ArrowLeft, ArrowRight, Check, FileSearch, FolderOpen, Download, PackageOpen, Search, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { PrefixChoiceEditor, RunnerSelect } from "../../components/pickers";
import { Button } from "../../components/ui/Button";
import { Badge, Callout, ErrorState, KeyValueList, ProgressBar, Section } from "../../components/ui/feedback";
import { SwitchRow, TextField } from "../../components/ui/form";
import { VndbSearch } from "../../components/VndbSearch";
import type { AppError, ExeCandidate, ImportScan, InstallerRun, PrefixChoice, RunnerRef, TaskProgress, VndbSearchResult } from "../../contract";
import { useI18n, type MessageKey } from "../../i18n";
import { call, normalizeError } from "../../lib/api";
import { defaultGameSettings } from "../../lib/defaults";
import { EXE_FILTERS, pickDirectory, pickFile } from "../../lib/dialogs";
import { formatBytes } from "../../lib/format";
import { useCommand, useEvent } from "../../lib/hooks";
import { useAppSettings, useLibrary } from "../../state/app";
import { useToasts } from "../../state/toasts";

type Step = "source" | "installer" | "candidates" | "details" | "runtime";
const STEP_ORDER: Exclude<Step, "installer">[] = ["source", "candidates", "details", "runtime"];

const KNOWN_HINTS: Record<string, MessageKey> = {
  largest: "import.hint.largest",
  matchesFolderName: "import.hint.matchesFolderName",
  hasGameData: "import.hint.hasGameData",
  configTool: "import.hint.configTool",
  uninstaller: "import.hint.uninstaller",
  installer: "import.hint.installer",
};

function dirname(p: string): string {
  const i = p.lastIndexOf("/");
  return i > 0 ? p.slice(0, i) : p;
}

export function ImportWizard() {
  const { t } = useI18n();
  const navigate = useNavigate();
  const toasts = useToasts();
  const library = useLibrary();
  const { appSettings } = useAppSettings();
  const runners = useCommand("runtime_list_runners", {});
  const prefixes = useCommand("runtime_list_prefixes", {});

  const [step, setStep] = useState<Step>("source");
  const [path, setPath] = useState("");
  const [scan, setScan] = useState<ImportScan | null>(null);
  const [scanning, setScanning] = useState(false);
  const [scanError, setScanError] = useState<AppError | null>(null);
  const [exePath, setExePath] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [vndb, setVndb] = useState<VndbSearchResult | null>(null);
  const [fetchMeta, setFetchMeta] = useState(true);
  const [runner, setRunner] = useState<RunnerRef | null>(null);
  const [prefix, setPrefix] = useState<PrefixChoice>({ type: "dedicated" });
  const [jpLocale, setJpLocale] = useState(true);
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<AppError | null>(null);

  // installer
  const [installerRunner, setInstallerRunner] = useState<RunnerRef | null>(null);
  const [installerPrefix, setInstallerPrefix] = useState<PrefixChoice>({ type: "dedicated" });
  const [run, setRun] = useState<InstallerRun | null>(null);
  const [task, setTask] = useState<TaskProgress | null>(null);
  const [installerError, setInstallerError] = useState<AppError | null>(null);

  const doScan = async (p = path) => {
    const target = p.trim();
    if (!target) return;
    setScanning(true);
    setScanError(null);
    try {
      const result = await call("library_scan_path", { path: target });
      setScan(result);
      const best = result.candidates.find((c) => !c.isInstaller) ?? result.candidates[0];
      setExePath(best?.path ?? null);
      setTitle((cur) => (cur && run ? cur : result.suggestedTitle));
      setStep(result.isInstaller ? "installer" : "candidates");
    } catch (e) {
      setScanError(normalizeError(e));
    } finally {
      setScanning(false);
    }
  };

  const browse = async (kind: "file" | "directory") => {
    try {
      const picked = kind === "file" ? await pickFile({ title: t("import.pickExe"), filters: EXE_FILTERS }) : await pickDirectory({ title: t("import.pickFolder") });
      if (picked) {
        setPath(picked);
        await doScan(picked);
      }
    } catch (e) {
      toasts.error(e, t("error.dialog"));
    }
  };

  // Installer progress (vnl://task with our task id).
  useEvent("vnl://task", (p) => {
    if (!run || p.taskId !== run.taskId) return;
    setTask(p);
    if (p.state === "done") {
      setPrefix({ type: "shared", id: run.prefix.id });
      setPath(run.driveC);
      void doScan(run.driveC);
    }
  });

  const runInstaller = async () => {
    const installerPath = scan?.candidates.find((c) => c.isInstaller)?.path ?? path.trim();
    const runnerId = installerRunner?.id ?? appSettings?.defaultRunner?.id ?? runners.data?.find((r) => r.kind !== "native")?.id;
    if (!runnerId) {
      setInstallerError({ code: "invalidInput", message: t("import.installerNeedsRunner"), details: null });
      return;
    }
    setInstallerError(null);
    setTask(null);
    try {
      const r = await call("runtime_run_installer", { request: { installerPath, runnerId, prefix: installerPrefix, nameHint: title.trim() || scan?.suggestedTitle || "Game" } });
      setRun(r);
      setRunner(installerRunner);
      setTask({ taskId: r.taskId, label: t("import.installing"), state: "running", progress: null, message: null });
    } catch (e) {
      setInstallerError(normalizeError(e));
    }
  };

  const doImport = async () => {
    if (!scan || !exePath) return;
    setImporting(true);
    setImportError(null);
    try {
      const custom = runner !== null || prefix.type !== "dedicated" || !jpLocale;
      let settings = null;
      if (custom) {
        settings = defaultGameSettings(appSettings);
        settings.runtime.runner = runner;
        settings.runtime.prefix = prefix;
        settings.runtime.locale.japaneseLocale = jpLocale;
      }
      const installDir = /\/drive_c\/?$/.test(scan.root) || !exePath.startsWith(scan.root) ? dirname(exePath) : scan.root;
      let game = await call("library_import_game", { request: { title: title.trim(), exePath, installDir, vndbId: vndb?.id ?? null, settings } });
      if (vndb && fetchMeta) {
        try {
          game = await call("vndb_apply", { gameId: game.id, vndbId: vndb.id });
        } catch (e) {
          toasts.error(e, t("import.vndbApplyFailed"));
        }
      }
      library.upsert(game);
      toasts.success(t("import.done", { title: game.title }));
      void navigate(`/game/${game.id}`);
    } catch (e) {
      setImportError(normalizeError(e));
      setImporting(false);
    }
  };

  const stepIndex = STEP_ORDER.indexOf(step === "installer" ? "candidates" : step);
  const selected = scan?.candidates.find((c) => c.path === exePath) ?? null;
  const prefixInvalid = (prefix.type === "shared" && !prefix.id) || (prefix.type === "custom" && !prefix.path.trim().startsWith("/"));

  return (
    <div className="page import-page">
      <header className="page-head">
        <div>
          <Link to="/" className="back-link">
            <ArrowLeft size={16} aria-hidden /> {t("nav.library")}
          </Link>
          <h1>{t("import.title")}</h1>
          <p className="page-sub">{t("import.subtitle")}</p>
        </div>
      </header>

      <ol className="stepper" aria-label={t("import.steps")}>
        {STEP_ORDER.map((s, i) => (
          <li key={s} className={i < stepIndex ? "is-done" : i === stepIndex ? "is-current" : ""} aria-current={i === stepIndex ? "step" : undefined}>
            <span className="step-num">{i < stepIndex ? <Check size={13} aria-hidden /> : i + 1}</span>
            <span>{t(`import.step.${s}`)}</span>
          </li>
        ))}
      </ol>

      {step === "source" && (
        <Section title={t("import.source")} description={t("import.sourceDesc")}>
          <div className="import-choices">
            <button type="button" className="choice-card" onClick={() => void browse("file")}>
              <FileSearch size={26} aria-hidden />
              <strong>{t("import.chooseExe")}</strong>
              <span>{t("import.chooseExeHint")}</span>
            </button>
            <button type="button" className="choice-card" onClick={() => void browse("directory")}>
              <FolderOpen size={26} aria-hidden />
              <strong>{t("import.chooseFolder")}</strong>
              <span>{t("import.chooseFolderHint")}</span>
            </button>
          </div>
          <form
            className="row gap-sm align-end"
            onSubmit={(e) => {
              e.preventDefault();
              void doScan();
            }}
          >
            <div className="grow">
              <TextField label={t("import.pathLabel")} value={path} onChange={setPath} placeholder="/home/you/Games/VN/…" mono />
            </div>
            <Button type="submit" variant="primary" icon={Search} loading={scanning} disabled={!path.trim()}>
              {t("import.scan")}
            </Button>
          </form>
          {scanError && <ErrorState error={scanError} />}
        </Section>
      )}

      {step === "installer" && scan && (
        <Section title={t("import.installerTitle")} description={t("import.installerDesc")}>
          <Callout tone="warning" icon={PackageOpen} title={t("import.looksLikeInstaller")}>
            {t("import.looksLikeInstallerText")}
          </Callout>
          {!run && (
            <>
              <div className="grid-2">
                <RunnerSelect label={t("compat.runner")} value={installerRunner} onChange={setInstallerRunner} runners={runners.data?.filter((r) => r.kind !== "native")} defaultRunnerId={appSettings?.defaultRunner?.id} />
                <TextField label={t("import.gameTitle")} value={title} onChange={setTitle} hint={t("import.nameHintHint")} />
              </div>
              <PrefixChoiceEditor value={installerPrefix} onChange={setInstallerPrefix} prefixes={prefixes.data} />
            </>
          )}
          {run && task && (
            <div className="installer-progress" aria-live="polite">
              <div className="row space-between">
                <strong>{task.label}</strong>
                <Badge tone={task.state === "done" ? "success" : task.state === "failed" ? "danger" : "accent"}>{t(`task.state.${task.state}`)}</Badge>
              </div>
              {task.state === "running" && <ProgressBar value={task.progress} label={task.label} />}
              {task.message && <p className="muted">{task.message}</p>}
              <p className="muted small">{t("import.installerPrefix", { path: run.prefix.path })}</p>
              {task.state === "failed" && <Callout tone="danger">{t("import.installerFailed")}</Callout>}
              {scanning && <p className="muted">{t("import.scanningDriveC")}</p>}
            </div>
          )}
          {installerError && <ErrorState error={installerError} />}
          {scanError && <ErrorState error={scanError} />}
          <div className="wizard-nav">
            <Button variant="ghost" icon={ArrowLeft} onClick={() => { setStep("source"); setRun(null); setTask(null); }} disabled={task?.state === "running"}>
              {t("common.back")}
            </Button>
            <div className="row gap-sm">
              <Button onClick={() => setStep("candidates")} disabled={task?.state === "running"}>
                {t("import.notInstaller")}
              </Button>
              {(!run || task?.state === "failed") && (
                <Button variant="primary" icon={Download} onClick={() => void runInstaller()}>
                  {t("import.runInstaller")}
                </Button>
              )}
            </div>
          </div>
        </Section>
      )}

      {step === "candidates" && scan && (
        <Section title={t("import.pickExeTitle")} description={t("import.pickExeDesc", { root: scan.root })}>
          {scan.existingGameId !== null && (
            <Callout tone="warning" title={t("import.alreadyInLibrary")} action={<Link className="btn btn-secondary btn-sm" to={`/game/${scan.existingGameId}`}>{t("import.openExisting")}</Link>}>
              {t("import.alreadyInLibraryText")}
            </Callout>
          )}
          {scan.candidates.length === 0 ? (
            <Callout tone="warning" title={t("import.noCandidates")}>
              {t("import.noCandidatesText")}
            </Callout>
          ) : (
            <CandidateList candidates={scan.candidates} value={exePath} onChange={setExePath} />
          )}
          {selected?.isInstaller && (
            <Callout tone="warning" icon={TriangleAlert} action={<Button size="sm" onClick={() => setStep("installer")}>{t("import.runInstaller")}</Button>}>
              {t("import.selectedInstaller")}
            </Callout>
          )}
          <div className="wizard-nav">
            <Button variant="ghost" icon={ArrowLeft} onClick={() => setStep("source")}>
              {t("common.back")}
            </Button>
            <Button variant="primary" iconRight={ArrowRight} disabled={!exePath} onClick={() => setStep("details")}>
              {t("common.continue")}
            </Button>
          </div>
        </Section>
      )}

      {step === "details" && scan && (
        <Section title={t("import.details")}>
          <TextField label={t("import.gameTitle")} value={title} onChange={setTitle} error={title.trim() ? null : t("validation.required")} autoFocus />
          {appSettings?.vndb.enabled !== false ? (
            <div className="subsection">
              <h3>{t("import.vndbOptional")}</h3>
              <p className="muted small">{t("import.vndbHint")}</p>
              <VndbSearch initialQuery={title} selectedId={vndb?.id ?? null} onSelect={(r) => setVndb(vndb?.id === r.id ? null : r)} />
              {vndb && (
                <div className="stack gap-sm">
                  <div className="row gap-sm wrap">
                    <Badge tone="accent">{t("import.vndbSelected", { title: vndb.title })}</Badge>
                    {vndb.title !== title && (
                      <Button size="sm" variant="ghost" onClick={() => setTitle(vndb.title)}>
                        {t("import.useVndbTitle")}
                      </Button>
                    )}
                    <Button size="sm" variant="ghost" onClick={() => setVndb(null)}>
                      {t("import.clearVndb")}
                    </Button>
                  </div>
                  <SwitchRow label={t("import.fetchMeta")} description={t("import.fetchMetaHint")} checked={fetchMeta} onChange={setFetchMeta} />
                </div>
              )}
            </div>
          ) : (
            <p className="muted small">{t("vndb.disabled")}</p>
          )}
          <div className="wizard-nav">
            <Button variant="ghost" icon={ArrowLeft} onClick={() => setStep("candidates")}>
              {t("common.back")}
            </Button>
            <Button variant="primary" iconRight={ArrowRight} disabled={!title.trim()} onClick={() => setStep("runtime")}>
              {t("common.continue")}
            </Button>
          </div>
        </Section>
      )}

      {step === "runtime" && scan && exePath && (
        <Section title={t("import.runtime")} description={t("import.runtimeDesc")}>
          {runners.error && <ErrorState error={runners.error} compact />}
          <RunnerSelect label={t("compat.runner")} value={runner} onChange={setRunner} runners={runners.data} defaultRunnerId={appSettings?.defaultRunner?.id} />
          <PrefixChoiceEditor value={prefix} onChange={setPrefix} prefixes={prefixes.data} error={prefixInvalid ? t("validation.choosePrefix") : null} />
          <SwitchRow label={t("compat.jpLocale")} description={t("compat.jpLocaleHint")} checked={jpLocale} onChange={setJpLocale} />
          <div className="summary-card">
            <KeyValueList
              items={[
                [t("import.gameTitle"), title],
                [t("game.exe"), <code key="e">{exePath}</code>],
                ["VNDB", vndb ? `${vndb.title} (${vndb.id})` : "—"],
              ]}
            />
          </div>
          {importError && <ErrorState error={importError} />}
          <div className="wizard-nav">
            <Button variant="ghost" icon={ArrowLeft} onClick={() => setStep("details")} disabled={importing}>
              {t("common.back")}
            </Button>
            <Button variant="primary" icon={Check} loading={importing} disabled={prefixInvalid || !title.trim()} onClick={() => void doImport()}>
              {t("import.import")}
            </Button>
          </div>
        </Section>
      )}
    </div>
  );
}

function CandidateList({ candidates, value, onChange }: { candidates: ExeCandidate[]; value: string | null; onChange: (p: string) => void }) {
  const { t } = useI18n();
  const max = Math.max(1, ...candidates.map((c) => c.score));
  return (
    <div className="candidate-list" role="radiogroup" aria-label={t("import.candidates")}>
      {candidates.map((c, i) => {
        const checked = c.path === value;
        return (
          <label key={c.path} className={checked ? "candidate is-selected" : "candidate"}>
            <input type="radio" name="exe-candidate" value={c.path} checked={checked} onChange={() => onChange(c.path)} />
            <span className="candidate-main">
              <span className="candidate-path mono">{c.relativePath}</span>
              <span className="candidate-hints">
                {i === 0 && !c.isInstaller && <Badge tone="success">{t("import.bestMatch")}</Badge>}
                {c.isInstaller && <Badge tone="warning">{t("import.installer")}</Badge>}
                {c.hints.filter((h) => h !== "installer").map((h) => (
                  <Badge key={h}>{KNOWN_HINTS[h] ? t(KNOWN_HINTS[h]) : h}</Badge>
                ))}
              </span>
            </span>
            <span className="candidate-side">
              <span className="score" title={t("import.score", { score: c.score })}>
                <span className="score-bar" style={{ width: `${Math.round((c.score / max) * 100)}%` }} />
              </span>
              <span className="muted small">{formatBytes(c.sizeBytes)}</span>
            </span>
          </label>
        );
      })}
    </div>
  );
}
