import { Info, MonitorPlay, Sparkles, TriangleAlert, Wand } from "lucide-react";
import type { GamescopeConfig, GamescopeFilter, GamescopeScaler, GraphicsConfig, UpscalerConfig, WindowMode } from "../contract";
import { useT } from "../i18n";
import { useCommand, useDebounced } from "../lib/hooks";
import { Button } from "./ui/Button";
import { Badge, Callout, ErrorState, Section, Spinner } from "./ui/feedback";
import { ArgsField, NumberField, OptionalTextField, Segmented, SelectField, SwitchRow } from "./ui/form";

export type Errors = Record<string, string>;

interface GraphicsEditorProps {
  value: GraphicsConfig;
  onChange: (v: GraphicsConfig) => void;
  errors?: Errors;
  /** Runner for the live preview (Proton vs. Wine changes FSR env). */
  runnerId?: string | null;
  /** Prefix for error keys, e.g. "graphics" or "defaultGraphics". */
  errorPrefix?: string;
}

const RES_PRESETS: Array<[number, number]> = [
  [640, 480],
  [800, 600],
  [1024, 768],
  [1280, 720],
  [1920, 1080],
];

export function GraphicsEditor({ value, onChange, errors = {}, runnerId, errorPrefix = "graphics" }: GraphicsEditorProps) {
  const t = useT();
  const caps = useCommand("graphics_capabilities", {});
  const upscaler = useCommand("graphics_upscaler_info", {});
  const debounced = useDebounced(value, 300);
  const preview = useCommand("graphics_preview", { config: debounced, runnerId: runnerId ?? null });
  const gs = value.gamescope;
  const err = (k: string) => errors[`${errorPrefix}.${k}`] ?? null;

  const setGs = (patch: Partial<GamescopeConfig>) => onChange({ ...value, gamescope: { ...gs, ...patch } });
  const setUp = (patch: Partial<UpscalerConfig>) => onChange({ ...value, upscaler: { ...value.upscaler, ...patch } });

  const gamescopeMissing = caps.data && !caps.data.gamescope;
  const monitors = caps.data?.monitors ?? [];
  const usesSharpness = gs.filter === "fsr" || gs.filter === "nis";
  const upAvailable = upscaler.data?.available ?? false;

  return (
    <div className="stack gap-lg">
      <Section title={t("gfx.gamescope.title")} description={t("gfx.gamescope.desc")} id={`${errorPrefix}-gamescope`}>
        {caps.error && <ErrorState error={caps.error} compact onRetry={() => void caps.reload()} />}
        {gamescopeMissing && (
          <Callout tone="warning" title={t("gfx.gamescope.missing")}>
            {t("gfx.gamescope.missingHint")}
          </Callout>
        )}
        <SwitchRow label={t("gfx.gamescope.enable")} description={caps.data?.gamescope ? t("gfx.gamescope.version", { version: caps.data.gamescope.version ?? "?" }) : undefined} checked={gs.enabled} onChange={(enabled) => setGs({ enabled })} />
        <fieldset className="fieldset" disabled={!gs.enabled}>
          <div className="grid-2">
            <div className="stack gap-sm">
              <span className="field-label">{t("gfx.gameRes")}</span>
              <div className="res-row">
                <NumberField label={t("gfx.width")} value={gs.gameWidth} onChange={(gameWidth) => setGs({ gameWidth })} min={1} error={err("gamescope.gameWidth")} placeholder={t("gfx.auto")} />
                <span className="res-x">×</span>
                <NumberField label={t("gfx.height")} value={gs.gameHeight} onChange={(gameHeight) => setGs({ gameHeight })} min={1} error={err("gamescope.gameHeight")} placeholder={t("gfx.auto")} />
              </div>
              <div className="chip-row" role="group" aria-label={t("gfx.presets")}>
                {RES_PRESETS.map(([w, h]) => (
                  <button key={`${w}x${h}`} type="button" className={gs.gameWidth === w && gs.gameHeight === h ? "chip is-active" : "chip"} onClick={() => setGs({ gameWidth: w, gameHeight: h })}>
                    {w}×{h}
                  </button>
                ))}
              </div>
              <span className="field-hint">{t("gfx.gameResHint")}</span>
            </div>
            <div className="stack gap-sm">
              <span className="field-label">{t("gfx.outputRes")}</span>
              <div className="res-row">
                <NumberField label={t("gfx.width")} value={gs.outputWidth} onChange={(outputWidth) => setGs({ outputWidth })} min={1} error={err("gamescope.outputWidth")} placeholder={t("gfx.auto")} />
                <span className="res-x">×</span>
                <NumberField label={t("gfx.height")} value={gs.outputHeight} onChange={(outputHeight) => setGs({ outputHeight })} min={1} error={err("gamescope.outputHeight")} placeholder={t("gfx.auto")} />
              </div>
              <div className="chip-row" role="group" aria-label={t("gfx.monitorPresets")}>
                {monitors
                  .filter((m) => m.width && m.height)
                  .map((m) => (
                    <button key={m.name} type="button" className={gs.outputWidth === m.width && gs.outputHeight === m.height ? "chip is-active" : "chip"} onClick={() => setGs({ outputWidth: m.width, outputHeight: m.height })}>
                      {m.name} · {m.width}×{m.height}
                    </button>
                  ))}
                <button type="button" className="chip" onClick={() => setGs({ outputWidth: null, outputHeight: null })}>
                  {t("gfx.auto")}
                </button>
              </div>
              <span className="field-hint">{t("gfx.outputResHint")}</span>
            </div>
          </div>
          <Segmented<WindowMode>
            label={t("gfx.windowMode")}
            value={gs.windowMode}
            onChange={(windowMode) => setGs({ windowMode })}
            options={[
              { value: "windowed", label: t("gfx.window.windowed") },
              { value: "borderless", label: t("gfx.window.borderless") },
              { value: "fullscreen", label: t("gfx.window.fullscreen") },
            ]}
          />
          <div className="grid-3">
            <SelectField
              label={t("gfx.scaler")}
              value={gs.scaler}
              onChange={(v) => setGs({ scaler: v as GamescopeScaler })}
              hint={t("gfx.scalerHint")}
              options={(["auto", "integer", "fit", "fill", "stretch"] as const).map((v) => ({ value: v, label: t(`gfx.scaler.${v}`) }))}
            />
            <SelectField
              label={t("gfx.filter")}
              value={gs.filter}
              onChange={(v) => setGs({ filter: v as GamescopeFilter })}
              hint={t("gfx.filterHint")}
              options={(["linear", "nearest", "fsr", "nis", "sgsr", "pixel"] as const).map((v) => ({ value: v, label: t(`gfx.filter.${v}`) }))}
            />
            <NumberField label={t("gfx.sharpness")} value={gs.sharpness} onChange={(sharpness) => setGs({ sharpness })} min={0} max={20} step={1} disabled={!usesSharpness} hint={usesSharpness ? t("gfx.sharpnessHint") : t("gfx.sharpnessOnlyFsr")} error={err("gamescope.sharpness")} placeholder={t("gfx.default")} />
          </div>
          <div className="grid-3">
            <NumberField label={t("gfx.fpsLimit")} value={gs.fpsLimit} onChange={(fpsLimit) => setGs({ fpsLimit })} min={1} suffix="fps" error={err("gamescope.fpsLimit")} placeholder={t("gfx.unlimited")} />
            <NumberField label={t("gfx.fpsUnfocused")} value={gs.fpsLimitUnfocused} onChange={(fpsLimitUnfocused) => setGs({ fpsLimitUnfocused })} min={1} suffix="fps" error={err("gamescope.fpsLimitUnfocused")} placeholder={t("gfx.unlimited")} />
            <SelectField
              label={t("gfx.monitor")}
              value={gs.preferOutput ?? ""}
              onChange={(v) => setGs({ preferOutput: v || null })}
              options={[
                { value: "", label: t("gfx.monitorAuto") },
                ...monitors.map((m) => ({ value: m.name, label: `${m.name}${m.width ? ` · ${m.width}×${m.height}` : ""}${m.refreshHz ? ` @ ${Math.round(m.refreshHz)} Hz` : ""}${m.primary ? ` · ${t("gfx.primary")}` : ""}` })),
              ]}
            />
          </div>
          <div className="grid-2">
            <SwitchRow label={t("gfx.grabCursor")} description={t("gfx.grabCursorHint")} checked={gs.forceGrabCursor} onChange={(forceGrabCursor) => setGs({ forceGrabCursor })} />
            <SwitchRow label={t("gfx.hdr")} description={caps.data && caps.data.sessionType !== "wayland" ? t("gfx.hdrNeedsWayland") : t("gfx.hdrHint")} checked={gs.hdr} onChange={(hdr) => setGs({ hdr })} />
          </div>
          <ArgsField label={t("gfx.extraArgs")} value={gs.extraArgs} onChange={(extraArgs) => setGs({ extraArgs })} hint={t("gfx.extraArgsHint")} placeholder="--adaptive-sync" />
        </fieldset>
        {caps.data && caps.data.notes.length > 0 && (
          <Callout tone="neutral" title={t("gfx.notes")}>
            <ul className="plain-list">
              {caps.data.notes.map((n) => (
                <li key={n}>{n}</li>
              ))}
            </ul>
          </Callout>
        )}
      </Section>

      <Section title={t("gfx.preview.title")} description={t("gfx.preview.desc")} id={`${errorPrefix}-preview`}>
        {preview.error ? (
          <ErrorState error={preview.error} compact onRetry={() => void preview.reload()} />
        ) : !preview.data ? (
          <Spinner label={t("common.loading")} />
        ) : (
          <div className="stack gap-sm">
            <pre className="code-block" aria-label={t("gfx.preview.wrapper")}>
              {preview.data.wrapperArgs.length ? preview.data.wrapperArgs.join(" ") + " <game command>" : t("gfx.preview.noWrapper")}
            </pre>
            {Object.keys(preview.data.env).length > 0 && (
              <pre className="code-block">
                {Object.entries(preview.data.env)
                  .map(([k, v]) => `${k}=${v}`)
                  .join("\n")}
              </pre>
            )}
            {preview.data.commandPreview && <pre className="code-block wrap">{preview.data.commandPreview}</pre>}
            {preview.data.companions?.map((c) => (
              <pre className="code-block wrap" key={c}>
                {c}
              </pre>
            ))}
            {preview.data.warnings.map((w) => (
              <Callout key={w} tone="warning">
                {w}
              </Callout>
            ))}
          </div>
        )}
      </Section>

      <Section
        title={
          <span className="row gap-sm">
            <Wand size={18} aria-hidden /> {t("gfx.wineFsr.title")} <Badge tone="info">{t("gfx.wineFsr.badge")}</Badge>
          </span>
        }
        id={`${errorPrefix}-winefsr`}
      >
        <Callout tone="info" icon={Info} title={t("gfx.wineFsr.notGamescope")}>
          {t("gfx.wineFsr.explain")}
        </Callout>
        <SwitchRow label={t("gfx.wineFsr.enable")} description={t("gfx.wineFsr.enableHint")} checked={value.wineFsr.enabled} onChange={(enabled) => onChange({ ...value, wineFsr: { ...value.wineFsr, enabled } })} />
        <NumberField
          label={t("gfx.wineFsr.strength")}
          value={value.wineFsr.strength}
          onChange={(strength) => onChange({ ...value, wineFsr: { ...value.wineFsr, strength } })}
          min={0}
          max={5}
          step={1}
          disabled={!value.wineFsr.enabled}
          hint={t("gfx.wineFsr.strengthHint")}
          error={err("wineFsr.strength")}
          placeholder={t("gfx.default")}
        />
      </Section>

      <Section
        title={
          <span className="row gap-sm">
            <Sparkles size={18} aria-hidden /> {t("gfx.upscaler.title")}
            {upscaler.data && (upAvailable ? <Badge tone="success">{t("common.available")}</Badge> : <Badge tone="warning">{t("common.unavailable")}</Badge>)}
          </span>
        }
        description={t("gfx.upscaler.desc")}
        id={`${errorPrefix}-upscaler`}
      >
        {upscaler.error && <ErrorState error={upscaler.error} compact onRetry={() => void upscaler.reload()} />}
        {upscaler.data && upscaler.data.limitations.length > 0 && (
          <Callout tone={upAvailable ? "neutral" : "warning"} icon={TriangleAlert} title={t("gfx.upscaler.limitations")}>
            <ul className="plain-list">
              {upscaler.data.limitations.map((l) => (
                <li key={l}>{l}</li>
              ))}
            </ul>
          </Callout>
        )}
        <fieldset className="fieldset" disabled={!upAvailable && !value.upscaler.enabled}>
          <SwitchRow label={t("gfx.upscaler.enable")} checked={value.upscaler.enabled} onChange={(enabled) => setUp({ enabled, backend: enabled ? (value.upscaler.backend ?? "rt-upscaler") : value.upscaler.backend })} disabled={!upAvailable && !value.upscaler.enabled} />
          {!upAvailable && value.upscaler.enabled && (
            <Callout tone="warning" action={<Button size="sm" onClick={() => setUp({ enabled: false })}>{t("gfx.upscaler.disable")}</Button>}>
              {t("gfx.upscaler.enabledButMissing")}
            </Callout>
          )}
          <div className="grid-3">
            <SelectField
              label={t("gfx.upscaler.model")}
              value={value.upscaler.model ?? ""}
              onChange={(v) => setUp({ model: v || null })}
              disabled={!value.upscaler.enabled}
              options={[{ value: "", label: t("gfx.default") }, ...(upscaler.data?.models ?? []).map((m) => ({ value: m.id, label: m.scale ? `${m.name} (×${m.scale})` : m.name }))]}
            />
            <SelectField
              label={t("gfx.upscaler.preset")}
              value={value.upscaler.preset ?? ""}
              onChange={(v) => setUp({ preset: v || null })}
              disabled={!value.upscaler.enabled}
              options={[{ value: "", label: t("gfx.default") }, ...(upscaler.data?.presets ?? []).map((p) => ({ value: p, label: p }))]}
            />
            <NumberField label={t("gfx.upscaler.scale")} value={value.upscaler.scale} onChange={(scale) => setUp({ scale })} min={1} max={8} step={0.5} disabled={!value.upscaler.enabled} error={err("upscaler.scale")} placeholder={t("gfx.auto")} suffix="×" />
          </div>
          <div className="grid-2">
            <OptionalTextField label={t("gfx.upscaler.targetTitle")} value={value.upscaler.targetTitle} onChange={(targetTitle) => setUp({ targetTitle })} disabled={!value.upscaler.enabled} hint={t("gfx.upscaler.targetTitleHint")} />
            <ArgsField label={t("gfx.extraArgs")} value={value.upscaler.extraArgs} onChange={(extraArgs) => setUp({ extraArgs })} />
          </div>
        </fieldset>
        {upscaler.data?.tool && (
          <p className="field-hint">
            <MonitorPlay size={13} aria-hidden /> {upscaler.data.tool.path} {upscaler.data.tool.version ?? ""}
          </p>
        )}
      </Section>
    </div>
  );
}

/** Validation shared by the game settings form and app default graphics. */
export function validateGraphics(g: GraphicsConfig, prefix: string, t: (k: "validation.positiveInt" | "validation.range" | "validation.pair", v?: Record<string, string | number>) => string): Errors {
  const errors: Errors = {};
  const gs = g.gamescope;
  const posInt = (v: number | null, key: string) => {
    if (v !== null && (!Number.isInteger(v) || v <= 0)) errors[`${prefix}.${key}`] = t("validation.positiveInt");
  };
  posInt(gs.gameWidth, "gamescope.gameWidth");
  posInt(gs.gameHeight, "gamescope.gameHeight");
  posInt(gs.outputWidth, "gamescope.outputWidth");
  posInt(gs.outputHeight, "gamescope.outputHeight");
  posInt(gs.fpsLimit, "gamescope.fpsLimit");
  posInt(gs.fpsLimitUnfocused, "gamescope.fpsLimitUnfocused");
  if (gs.enabled) {
    if ((gs.gameWidth === null) !== (gs.gameHeight === null)) errors[`${prefix}.gamescope.${gs.gameWidth === null ? "gameWidth" : "gameHeight"}`] = t("validation.pair");
    if ((gs.outputWidth === null) !== (gs.outputHeight === null)) errors[`${prefix}.gamescope.${gs.outputWidth === null ? "outputWidth" : "outputHeight"}`] = t("validation.pair");
  }
  if (gs.sharpness !== null && (!Number.isInteger(gs.sharpness) || gs.sharpness < 0 || gs.sharpness > 20)) errors[`${prefix}.gamescope.sharpness`] = t("validation.range", { min: 0, max: 20 });
  const st = g.wineFsr.strength;
  if (st !== null && (!Number.isInteger(st) || st < 0 || st > 5)) errors[`${prefix}.wineFsr.strength`] = t("validation.range", { min: 0, max: 5 });
  const sc = g.upscaler.scale;
  if (sc !== null && (sc <= 0 || sc > 8)) errors[`${prefix}.upscaler.scale`] = t("validation.range", { min: 1, max: 8 });
  return errors;
}
