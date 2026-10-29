import { Play, Plus, Trash } from "lucide-react";
import { useState } from "react";
import { PrefixChoiceEditor, RunnerSelect } from "../../components/pickers";
import { Button } from "../../components/ui/Button";
import { Badge, Callout, ErrorState, Section } from "../../components/ui/feedback";
import { ArgsField, KeyValueEditor, NumberField, OptionalTextField, PathField, Segmented, SwitchRow, TextField } from "../../components/ui/form";
import type { AuxProgram, Game, ProtonLauncher, RuntimeConfig } from "../../contract";
import { useT } from "../../i18n";
import { call } from "../../lib/api";
import { newAuxProgram } from "../../lib/defaults";
import { EXE_FILTERS } from "../../lib/dialogs";
import { useCommand } from "../../lib/hooks";
import { useAppSettings, useSessions } from "../../state/app";
import { useToasts } from "../../state/toasts";
import { ENV_KEY_RE, type GameSettingsForm } from "./settingsForm";

const TIMEZONES = ["Asia/Tokyo", "Asia/Seoul", "Asia/Shanghai", "UTC", "Europe/Rome", "Europe/London", "America/New_York"];

export function CompatibilityTab({ game, form }: { game: Game; form: GameSettingsForm }) {
  const t = useT();
  const { appSettings } = useAppSettings();
  const runners = useCommand("runtime_list_runners", {});
  const prefixes = useCommand("runtime_list_prefixes", {});
  const draft = form.draft!;
  const rt = draft.runtime;
  const files = form.files;
  const errors = form.errors;
  const setRt = (patch: Partial<RuntimeConfig>) => form.setDraft((d) => ({ ...d, runtime: { ...d.runtime, ...patch } }));

  const effectiveRunnerId = rt.runner?.id ?? appSettings?.defaultRunner?.id ?? null;
  const effectiveRunner = runners.data?.find((r) => r.id === effectiveRunnerId);
  const isProton = effectiveRunner?.kind === "proton";
  const isNative = effectiveRunner?.kind === "native";

  return (
    <div className="stack gap-lg">
      {files && (
        <Section title={t("compat.files")} description={t("compat.filesDesc")} id="compat-files">
          <PathField label={t("game.exe")} value={files.exePath} onChange={(exePath) => form.setFiles((f) => ({ ...f, exePath }))} filters={EXE_FILTERS} error={errors["files.exePath"]} demoDefault={game.exePath} />
          <div className="grid-2">
            <PathField label={t("compat.workingDir")} kind="directory" clearable value={files.workingDir ?? ""} onChange={(v) => form.setFiles((f) => ({ ...f, workingDir: v.trim() ? v : null }))} placeholder={t("compat.workingDirDefault", { dir: game.installDir })} demoDefault={game.installDir} />
            <ArgsField label={t("compat.launchArgs")} value={files.launchArgs} onChange={(launchArgs) => form.setFiles((f) => ({ ...f, launchArgs }))} hint={t("compat.argsHint")} placeholder="-windowed" />
          </div>
        </Section>
      )}

      <Section title={t("compat.runnerPrefix")} id="compat-runner">
        {runners.error && <ErrorState error={runners.error} compact onRetry={() => void runners.reload()} />}
        {prefixes.error && <ErrorState error={prefixes.error} compact onRetry={() => void prefixes.reload()} />}
        <RunnerSelect label={t("compat.runner")} value={rt.runner} onChange={(runner) => setRt({ runner })} runners={runners.data} defaultRunnerId={appSettings?.defaultRunner?.id} hint={effectiveRunner ? t("compat.runnerResolved", { name: effectiveRunner.name, path: effectiveRunner.path }) : t("compat.runnerHint")} />
        {!effectiveRunner && runners.data && (
          <Callout tone="warning" title={t("compat.noRunner")}>
            {t("compat.noRunnerHint")}
          </Callout>
        )}
        {!isNative && <PrefixChoiceEditor value={rt.prefix} onChange={(prefix) => setRt({ prefix })} prefixes={prefixes.data} error={errors["runtime.prefix"]} />}
        <SwitchRow label={t("compat.waitWineserver")} description={t("compat.waitWineserverHint")} checked={rt.waitForWineserver} onChange={(waitForWineserver) => setRt({ waitForWineserver })} disabled={isNative} />
      </Section>

      <Section title={t("compat.locale")} description={t("compat.localeDesc")} id="compat-locale">
        <SwitchRow label={t("compat.jpLocale")} description={t("compat.jpLocaleHint")} checked={rt.locale.japaneseLocale} onChange={(japaneseLocale) => setRt({ locale: { ...rt.locale, japaneseLocale } })} disabled={!!rt.locale.locale} />
        <div className="grid-2">
          <OptionalTextField label={t("compat.customLocale")} value={rt.locale.locale} onChange={(locale) => setRt({ locale: { ...rt.locale, locale } })} placeholder="ja_JP.UTF-8" mono hint={t("compat.customLocaleHint")} />
          <div>
            <OptionalTextField label={t("compat.timezone")} value={rt.locale.timezone} onChange={(timezone) => setRt({ locale: { ...rt.locale, timezone } })} placeholder="Asia/Tokyo" mono error={errors["runtime.locale.timezone"]} hint={t("compat.timezoneHint")} />
            <div className="chip-row">
              {TIMEZONES.map((tz) => (
                <button key={tz} type="button" className={rt.locale.timezone === tz ? "chip is-active" : "chip"} onClick={() => setRt({ locale: { ...rt.locale, timezone: tz } })}>
                  {tz}
                </button>
              ))}
            </div>
          </div>
        </div>
      </Section>

      <Section title={t("compat.environment")} id="compat-env">
        <KeyValueEditor label={t("compat.envVars")} description={t("compat.envVarsHint")} value={rt.env} onChange={(env) => setRt({ env })} keyPlaceholder="DXVK_HUD" valuePlaceholder="fps" validateKey={(k) => (ENV_KEY_RE.test(k) ? null : t("validation.envKey", { key: k }))} />
        {errors["runtime.env"] && <p className="field-error">{errors["runtime.env"]}</p>}
        {!isNative && (
          <>
            <KeyValueEditor label={t("compat.dllOverrides")} description={t("compat.dllOverridesHint")} value={rt.dllOverrides} onChange={(dllOverrides) => setRt({ dllOverrides })} keyPlaceholder="d3d9" valuePlaceholder="n,b" valueOptions={["n,b", "b,n", "n", "b", ""]} />
            {errors["runtime.dllOverrides"] && <p className="field-error">{errors["runtime.dllOverrides"]}</p>}
          </>
        )}
      </Section>

      <Section
        title={
          <span className="row gap-sm">
            {t("compat.proton")} {effectiveRunner && (isProton ? <Badge tone="accent">{t("compat.protonActive")}</Badge> : <Badge>{t("compat.protonInactive")}</Badge>)}
          </span>
        }
        description={t("compat.protonDesc")}
        id="compat-proton"
      >
        <fieldset className="fieldset" disabled={!isProton && !!effectiveRunner}>
          <Segmented<ProtonLauncher>
            label={t("compat.protonLauncher")}
            value={rt.proton.launcher}
            onChange={(launcher) => setRt({ proton: { ...rt.proton, launcher } })}
            options={[
              { value: "auto", label: t("compat.launcher.auto"), title: t("compat.launcher.autoHint") },
              { value: "umu", label: "umu-run" },
              { value: "direct", label: t("compat.launcher.direct") },
            ]}
          />
          <div className="grid-2">
            <SwitchRow label={t("compat.wined3d")} description="PROTON_USE_WINED3D=1" checked={rt.proton.useWined3d} onChange={(useWined3d) => setRt({ proton: { ...rt.proton, useWined3d } })} />
            <SwitchRow label={t("compat.protonLog")} description="PROTON_LOG=1" checked={rt.proton.enableLog} onChange={(enableLog) => setRt({ proton: { ...rt.proton, enableLog } })} />
            <SwitchRow label={t("compat.noEsync")} description="PROTON_NO_ESYNC=1" checked={rt.proton.noEsync} onChange={(noEsync) => setRt({ proton: { ...rt.proton, noEsync } })} />
            <SwitchRow label={t("compat.noFsync")} description="PROTON_NO_FSYNC=1" checked={rt.proton.noFsync} onChange={(noFsync) => setRt({ proton: { ...rt.proton, noFsync } })} />
          </div>
          <OptionalTextField label={t("compat.umuGameId")} value={rt.proton.umuGameId} onChange={(umuGameId) => setRt({ proton: { ...rt.proton, umuGameId } })} placeholder="umu-default" mono hint={t("compat.umuGameIdHint")} />
        </fieldset>
      </Section>

      <AuxSection game={game} form={form} />
    </div>
  );
}

function AuxSection({ game, form }: { game: Game; form: GameSettingsForm }) {
  const t = useT();
  const toasts = useToasts();
  const { activeFor, upsert } = useSessions();
  const session = activeFor(game.id);
  const aux = form.draft!.runtime.auxiliary;
  const savedIds = new Set(form.saved?.runtime.auxiliary.map((a) => a.id) ?? []);
  const [starting, setStarting] = useState<string | null>(null);
  const setAux = (next: AuxProgram[]) => form.setDraft((d) => ({ ...d, runtime: { ...d.runtime, auxiliary: next } }));
  const patch = (i: number, p: Partial<AuxProgram>) => setAux(aux.map((a, j) => (j === i ? { ...a, ...p } : a)));

  const startNow = async (auxId: string) => {
    if (!session) return;
    setStarting(auxId);
    try {
      upsert(await call("session_start_aux", { sessionId: session.id, auxId }));
    } catch (e) {
      toasts.error(e, t("aux.startFailed"));
    } finally {
      setStarting(null);
    }
  };

  return (
    <Section
      title={t("aux.title")}
      description={t("aux.desc")}
      id="compat-aux"
      actions={
        <Button size="sm" icon={Plus} onClick={() => setAux([...aux, newAuxProgram()])}>
          {t("aux.add")}
        </Button>
      }
    >
      {aux.length === 0 && <p className="muted">{t("aux.empty")}</p>}
      {aux.map((a, i) => {
        const live = session?.aux.find((x) => x.auxId === a.id);
        const err = (k: string) => form.errors[`runtime.auxiliary.${i}.${k}`] ?? null;
        return (
          <div className="aux-card" key={a.id} role="group" aria-label={a.name || t("aux.unnamed", { n: i + 1 })}>
            <div className="aux-head">
              <strong>{a.name || t("aux.unnamed", { n: i + 1 })}</strong>
              {live && (live.running ? <Badge tone="success">{t("aux.running", { pid: live.pid ?? "?" })}</Badge> : <Badge>{t("aux.notRunning")}</Badge>)}
              <span className="grow" />
              {session?.state === "running" && (
                <Button size="sm" icon={Play} onClick={() => void startNow(a.id)} loading={starting === a.id} disabled={!savedIds.has(a.id) || live?.running} title={!savedIds.has(a.id) ? t("aux.saveFirst") : undefined}>
                  {t("aux.startNow")}
                </Button>
              )}
              <Button size="sm" variant="ghost" icon={Trash} onClick={() => setAux(aux.filter((_, j) => j !== i))}>
                {t("common.remove")}
              </Button>
            </div>
            <div className="grid-2">
              <TextField label={t("aux.name")} value={a.name} onChange={(name) => patch(i, { name })} error={err("name")} placeholder="Textractor" />
              <Segmented<"windows" | "native">
                label={t("aux.kind")}
                value={a.windows ? "windows" : "native"}
                onChange={(v) => patch(i, { windows: v === "windows" })}
                options={[
                  { value: "windows", label: t("aux.windows") },
                  { value: "native", label: t("aux.native") },
                ]}
              />
            </div>
            <PathField label={t("aux.exe")} value={a.exePath} onChange={(exePath) => patch(i, { exePath })} filters={a.windows ? EXE_FILTERS : undefined} error={err("exePath")} />
            <div className="grid-2">
              <ArgsField label={t("aux.args")} value={a.args} onChange={(args) => patch(i, { args })} />
              <PathField label={t("compat.workingDir")} kind="directory" clearable value={a.workingDir ?? ""} onChange={(v) => patch(i, { workingDir: v.trim() ? v : null })} />
            </div>
            <div className="grid-3">
              <SwitchRow label={t("aux.autostart")} checked={a.autostart} onChange={(autostart) => patch(i, { autostart })} />
              <NumberField label={t("aux.delay")} value={a.delayMs} onChange={(v) => patch(i, { delayMs: v ?? 0 })} min={0} step={500} suffix="ms" error={err("delayMs")} disabled={!a.autostart} />
              <SwitchRow label={t("aux.stopWithGame")} checked={a.stopWithGame} onChange={(stopWithGame) => patch(i, { stopWithGame })} />
            </div>
            <details className="aux-env">
              <summary>{t("aux.env", { n: Object.keys(a.env).length })}</summary>
              <KeyValueEditor label={t("compat.envVars")} value={a.env} onChange={(env) => patch(i, { env })} validateKey={(k) => (ENV_KEY_RE.test(k) ? null : t("validation.envKey", { key: k }))} />
            </details>
          </div>
        );
      })}
    </Section>
  );
}
