import { Eye, EyeOff, Globe, Languages, RefreshCw, ScanText } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { LanguageSelect } from "../../components/pickers";
import { Button } from "../../components/ui/Button";
import { Badge, Callout, ErrorState, Section } from "../../components/ui/feedback";
import { NumberField, OptionalTextField, Segmented, SelectField, SwitchRow } from "../../components/ui/form";
import type { BackendInfo, ExtractionMode, Game, ProviderInfo, TranslationConfig, TranslationLine } from "../../contract";
import { useT } from "../../i18n";
import { call } from "../../lib/api";
import { useCommand } from "../../lib/hooks";
import { useAppSettings, useLanguages, useSessions } from "../../state/app";
import { useToasts } from "../../state/toasts";
import type { GameSettingsForm } from "./settingsForm";

export function providerLabel(p: ProviderInfo, t: ReturnType<typeof useT>): string {
  const flags = [p.sendsDataRemotely ? t("provider.remoteShort") : t("provider.localShort")];
  if (!p.available) flags.push(t("common.unavailable"));
  else if (!p.configured) flags.push(t("provider.notConfigured"));
  return `${p.name} · ${flags.join(", ")}`;
}

function backendOptions(list: BackendInfo[] | undefined, t: ReturnType<typeof useT>, defaultLabel: string) {
  return [{ value: "", label: defaultLabel }, ...(list ?? []).map((b) => ({ value: b.id, label: b.available ? b.name : `${b.name} · ${t("common.unavailable")}` }))];
}

export function TranslationTab({ game, form }: { game: Game; form: GameSettingsForm }) {
  const t = useT();
  const toasts = useToasts();
  const { appSettings } = useAppSettings();
  const { nameFor } = useLanguages();
  const providers = useCommand("translation_providers", {});
  const hookBackends = useCommand("translation_hook_backends", {});
  const ocrBackends = useCommand("translation_ocr_backends", {});
  const { activeFor } = useSessions();
  const session = activeFor(game.id);
  const tr = form.draft!.translation;
  const errors = form.errors;
  const set = (patch: Partial<TranslationConfig>) => form.setDraft((d) => ({ ...d, translation: { ...d.translation, ...patch } }));

  const appTarget = appSettings?.translation.targetLanguage ?? null;
  const appSource = appSettings?.translation.sourceLanguage ?? null;
  const appProviderId = appSettings?.translation.providerId ?? null;
  const effectiveTarget = tr.targetLanguage ?? appTarget;
  const effectiveProviderId = tr.providerId ?? appProviderId;
  const effectiveProvider = providers.data?.find((p) => p.id === effectiveProviderId);
  const appProvider = providers.data?.find((p) => p.id === appProviderId);
  const usesHook = tr.extraction === "hook" || tr.extraction === "hookAndOcr";
  const usesOcr = tr.extraction === "ocr" || tr.extraction === "hookAndOcr";

  return (
    <div className="stack gap-lg">
      <Section title={t("tr.title")} description={t("tr.desc")} id="tr-main">
        <SwitchRow label={t("tr.enable")} description={t("tr.enableHint")} checked={tr.enabled} onChange={(enabled) => set({ enabled })} />
      </Section>

      <fieldset className="fieldset stack gap-lg" disabled={!tr.enabled} aria-label={t("tr.settings")}>
        <Section title={t("tr.languages")} id="tr-langs">
          {!effectiveTarget && (
            <Callout tone="warning" role="alert" icon={Languages} title={t("tr.chooseTarget")} action={<Link to="/settings" className="btn btn-secondary btn-sm">{t("tr.openAppSettings")}</Link>}>
              {t("tr.chooseTargetText")}
            </Callout>
          )}
          <div className="grid-2">
            <LanguageSelect
              label={t("tr.source")}
              value={tr.sourceLanguage}
              onChange={(sourceLanguage) => set({ sourceLanguage })}
              defaultOptionLabel={appSource ? t("tr.useAppDefaultNamed", { name: nameFor(appSource) ?? appSource }) : t("tr.useAppDefaultUnset")}
            />
            <LanguageSelect
              label={t("tr.target")}
              value={tr.targetLanguage}
              onChange={(targetLanguage) => set({ targetLanguage })}
              defaultOptionLabel={appTarget ? t("tr.useAppDefaultNamed", { name: nameFor(appTarget) ?? appTarget }) : t("tr.useAppDefaultUnset")}
              error={tr.enabled && !effectiveTarget ? t("tr.targetMissing") : null}
              hint={effectiveTarget ? t("tr.targetResolved", { name: nameFor(effectiveTarget) ?? effectiveTarget }) : undefined}
            />
          </div>
        </Section>

        <Section title={t("tr.provider")} id="tr-provider">
          {providers.error && <ErrorState error={providers.error} compact onRetry={() => void providers.reload()} />}
          <SelectField
            label={t("tr.provider")}
            value={tr.providerId ?? ""}
            onChange={(v) => set({ providerId: v || null })}
            options={[{ value: "", label: appProvider ? t("tr.useAppDefaultNamed", { name: appProvider.name }) : t("tr.useAppDefaultUnset") }, ...(providers.data ?? []).map((p) => ({ value: p.id, label: providerLabel(p, t) }))]}
            hint={<Link to="/translation?tab=providers">{t("tr.configureProviders")}</Link>}
          />
          {effectiveProvider?.sendsDataRemotely && (
            <Callout tone="warning" icon={Globe} title={t("provider.remoteTitle")}>
              {t("provider.remoteText", { name: effectiveProvider.name })}
            </Callout>
          )}
          {effectiveProvider && !effectiveProvider.configured && <Callout tone="warning">{t("provider.needsConfig", { name: effectiveProvider.name })}</Callout>}
          {!effectiveProviderId && providers.data && <Callout tone="info">{t("tr.noProvider")}</Callout>}
        </Section>

        <Section title={t("tr.extraction")} description={t("tr.extractionDesc")} id="tr-extraction">
          <Segmented<ExtractionMode>
            label={t("tr.extractionMode")}
            value={tr.extraction}
            onChange={(extraction) => set({ extraction })}
            options={[
              { value: "disabled", label: t("tr.mode.disabled") },
              { value: "hook", label: t("tr.mode.hook") },
              { value: "ocr", label: t("tr.mode.ocr") },
              { value: "hookAndOcr", label: t("tr.mode.hookAndOcr") },
            ]}
          />
          {usesHook && (
            <div className="subsection">
              <h3>{t("tr.hook")}</h3>
              <BackendDiagnostics list={hookBackends.data} selected={tr.hook.backend} />
              <div className="grid-3">
                <SelectField label={t("tr.hookBackend")} value={tr.hook.backend ?? ""} onChange={(v) => set({ hook: { ...tr.hook, backend: v || null } })} options={backendOptions(hookBackends.data, t, t("tr.autoBackend"))} />
                <OptionalTextField label={t("tr.hookCode")} value={tr.hook.hookCode} onChange={(hookCode) => set({ hook: { ...tr.hook, hookCode } })} placeholder="/HS-8@4A2B0:game.exe" mono hint={t("tr.hookCodeHint")} />
                <NumberField label={t("tr.attachDelay")} value={tr.hook.attachDelayMs} onChange={(v) => set({ hook: { ...tr.hook, attachDelayMs: v ?? 0 } })} min={0} step={500} suffix="ms" error={errors["translation.hook.attachDelayMs"]} />
              </div>
            </div>
          )}
          {usesOcr && (
            <div className="subsection">
              <h3>{t("tr.ocr")}</h3>
              <BackendDiagnostics list={ocrBackends.data} selected={tr.ocr.backend} />
              <div className="grid-2">
                <SelectField label={t("tr.ocrBackend")} value={tr.ocr.backend ?? ""} onChange={(v) => set({ ocr: { ...tr.ocr, backend: v || null } })} options={backendOptions(ocrBackends.data, t, t("tr.autoBackend"))} />
                <NumberField label={t("tr.ocrInterval")} value={tr.ocr.intervalMs} onChange={(v) => set({ ocr: { ...tr.ocr, intervalMs: v ?? 0 } })} min={0} step={250} suffix="ms" hint={t("tr.ocrIntervalHint")} error={errors["translation.ocr.intervalMs"]} />
              </div>
              <SwitchRow label={t("tr.ocrRegion")} description={t("tr.ocrRegionHint")} checked={tr.ocr.region !== null} onChange={(on) => set({ ocr: { ...tr.ocr, region: on ? { x: 0, y: 0, width: 1280, height: 200 } : null } })} />
              {tr.ocr.region && (
                <div className="grid-4">
                  {(["x", "y", "width", "height"] as const).map((k) => (
                    <NumberField key={k} label={t(`tr.region.${k}`)} value={tr.ocr.region![k]} onChange={(v) => set({ ocr: { ...tr.ocr, region: { ...tr.ocr.region!, [k]: Math.round(v ?? 0) } } })} min={k === "width" || k === "height" ? 1 : undefined} suffix="px" />
                  ))}
                  {errors["translation.ocr.region"] && <p className="field-error">{errors["translation.ocr.region"]}</p>}
                </div>
              )}
            </div>
          )}
          <SwitchRow label={t("tr.keepHistory")} description={t("tr.keepHistoryHint")} checked={tr.keepHistory} onChange={(keepHistory) => set({ keepHistory })} />
        </Section>

        <Section
          title={t("tr.overlay")}
          description={t("tr.overlayDesc")}
          id="tr-overlay"
          actions={
            <div className="row gap-sm">
              <Button size="sm" icon={Eye} onClick={() => call("overlay_show", { config: tr.overlay }).catch((e: unknown) => toasts.error(e, t("tr.overlayFailed")))}>
                {t("tr.showOverlay")}
              </Button>
              <Button size="sm" variant="ghost" icon={EyeOff} onClick={() => call("overlay_hide", {}).catch((e: unknown) => toasts.error(e, t("tr.overlayFailed")))}>
                {t("tr.hideOverlay")}
              </Button>
            </div>
          }
        >
          <SwitchRow label={t("tr.overlayEnable")} description={t("tr.overlayEnableHint")} checked={tr.overlay.enabled} onChange={(enabled) => set({ overlay: { ...tr.overlay, enabled } })} />
          <div className="grid-3">
            <NumberField label={t("tr.fontSize")} value={tr.overlay.fontSize} onChange={(v) => set({ overlay: { ...tr.overlay, fontSize: Math.round(v ?? 22) } })} min={8} max={96} suffix="px" error={errors["translation.overlay.fontSize"]} />
            <div className="field">
              <label className="field-label" htmlFor="overlay-opacity">
                {t("tr.opacity")} · {Math.round(tr.overlay.opacity * 100)}%
              </label>
              <input id="overlay-opacity" className="range" type="range" min={0.1} max={1} step={0.05} value={tr.overlay.opacity} onChange={(e) => set({ overlay: { ...tr.overlay, opacity: Number(e.target.value) } })} />
            </div>
            <SwitchRow label={t("tr.showOriginal")} checked={tr.overlay.showOriginal} onChange={(showOriginal) => set({ overlay: { ...tr.overlay, showOriginal } })} />
          </div>
          <div className="overlay-sample" style={{ fontSize: Math.min(tr.overlay.fontSize, 40), background: `rgba(10, 12, 18, ${tr.overlay.opacity})` }} aria-hidden>
            {tr.overlay.showOriginal && <div className="ov-original" lang="ja">「またここで会ったね」</div>}
            <div className="ov-translated">{effectiveTarget ? t("tr.overlaySample") : t("tr.chooseTarget")}</div>
          </div>
        </Section>
      </fieldset>

      {session && <SessionTranslationStatus sessionId={session.id} gameId={game.id} />}
    </div>
  );
}

function BackendDiagnostics({ list, selected }: { list: BackendInfo[] | undefined; selected: string | null }) {
  const t = useT();
  if (!list) return null;
  const relevant = selected ? list.filter((b) => b.id === selected) : list;
  const anyAvailable = list.some((b) => b.available);
  const problems = relevant.filter((b) => !b.available && b.diagnostics.length);
  if (!problems.length && (selected || anyAvailable)) return null;
  return (
    <Callout tone="warning" title={!anyAvailable ? t("tr.noBackend") : t("tr.backendIssues")}>
      <ul className="plain-list">
        {problems.flatMap((b) => b.diagnostics.map((d) => <li key={`${b.id}-${d}`}>{`${b.name}: ${d}`}</li>))}
      </ul>
    </Callout>
  );
}

function SessionTranslationStatus({ sessionId, gameId }: { sessionId: string; gameId: number }) {
  const t = useT();
  const toasts = useToasts();
  const status = useCommand("translation_session_status", { sessionId });
  const [capturing, setCapturing] = useState(false);
  const [last, setLast] = useState<TranslationLine | null>(null);
  const tone = (s: string) => (s === "active" ? "success" : s === "failed" ? "danger" : s === "starting" ? "warning" : "neutral");
  return (
    <Section
      title={t("tr.liveStatus")}
      actions={
        <div className="row gap-sm">
          <Button size="sm" variant="ghost" icon={RefreshCw} onClick={() => void status.reload()}>
            {t("common.refresh")}
          </Button>
          <Button
            size="sm"
            icon={ScanText}
            loading={capturing}
            onClick={async () => {
              setCapturing(true);
              try {
                setLast(await call("translation_ocr_capture", { request: { sessionId, gameId, translate: true } }));
              } catch (e) {
                toasts.error(e, t("tr.ocrCaptureFailed"));
              } finally {
                setCapturing(false);
              }
            }}
          >
            {t("tr.ocrCaptureNow")}
          </Button>
        </div>
      }
    >
      {status.error && <ErrorState error={status.error} compact />}
      {status.data && (
        <div className="stack gap-sm">
          <div className="row gap-sm">
            <Badge tone={tone(status.data.hook)}>{t("tr.hookState", { state: t(`tr.state.${status.data.hook}`) })}</Badge>
            <Badge tone={tone(status.data.ocr)}>{t("tr.ocrState", { state: t(`tr.state.${status.data.ocr}`) })}</Badge>
          </div>
          {status.data.diagnostics.length > 0 && (
            <ul className="plain-list muted">
              {status.data.diagnostics.map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      {last && <LineView line={last} />}
    </Section>
  );
}

export function LineView({ line }: { line: TranslationLine }) {
  const t = useT();
  return (
    <div className="line-view">
      <p className="line-src" lang={line.sourceLanguage ?? "ja"}>
        {line.sourceText}
      </p>
      {line.translatedText && (
        <p className="line-dst" lang={line.targetLanguage ?? undefined}>
          {line.translatedText}
        </p>
      )}
      {line.error && <p className="text-danger">{line.error}</p>}
      <p className="line-meta">{[line.origin, line.providerId, line.sourceLanguage && line.targetLanguage ? `${line.sourceLanguage} → ${line.targetLanguage}` : null].filter(Boolean).join(" · ") || t("common.none")}</p>
    </div>
  );
}
