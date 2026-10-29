import { Save, Undo2 } from "lucide-react";
import { useState } from "react";
import { GraphicsEditor, validateGraphics } from "../../components/GraphicsEditor";
import { LanguageSelect, runnerLabel } from "../../components/pickers";
import { Button } from "../../components/ui/Button";
import { Callout, ErrorState, LoadingBlock, Section } from "../../components/ui/feedback";
import { PathField, Segmented, SelectField, SwitchRow } from "../../components/ui/form";
import type { AppError, AppSettings, Theme } from "../../contract";
import { UI_LANGUAGES, useI18n } from "../../i18n";
import { normalizeError } from "../../lib/api";
import { useCommand } from "../../lib/hooks";
import { useAppSettings } from "../../state/app";
import { useToasts } from "../../state/toasts";
import { providerLabel } from "../game/TranslationTab";

export function SettingsPage() {
  const { t } = useI18n();
  const { appSettings, error, reload } = useAppSettings();
  if (error && !appSettings) {
    return (
      <div className="page">
        <h1>{t("settings.title")}</h1>
        <ErrorState error={error} onRetry={() => void reload()} />
      </div>
    );
  }
  if (!appSettings) return <LoadingBlock />;
  return <SettingsForm saved={appSettings} />;
}

const BLUR_OPTIONS: Array<[string, number | null]> = [
  ["never", null],
  ["suggestive", 1],
  ["explicit", 2],
  ["all", 0.5],
];

function SettingsForm({ saved }: { saved: AppSettings }) {
  const { t } = useI18n();
  const toasts = useToasts();
  const { save } = useAppSettings();
  const runners = useCommand("runtime_list_runners", {});
  const providers = useCommand("translation_providers", {});
  const [draft, setDraft] = useState(saved);
  const [base, setBase] = useState(saved);
  if (base !== saved) {
    setBase(saved);
    setDraft(saved);
  }
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<AppError | null>(null);
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);
  const errors = validateGraphics(draft.defaultGraphics, "defaultGraphics", t);
  const errorCount = Object.keys(errors).length;
  const set = (patch: Partial<AppSettings>) => setDraft((d) => ({ ...d, ...patch }));

  const doSave = async () => {
    if (errorCount) return;
    setSaving(true);
    setSaveError(null);
    try {
      await save(draft);
      toasts.success(t("form.saved"));
    } catch (e) {
      setSaveError(normalizeError(e));
      toasts.error(e, t("settings.saveFailed"));
    } finally {
      setSaving(false);
    }
  };

  const blurValue = BLUR_OPTIONS.find(([, v]) => v === draft.vndb.blurSexualThreshold)?.[0] ?? "custom";

  return (
    <div className="page settings-page">
      <header className="page-head">
        <div>
          <h1>{t("settings.title")}</h1>
          <p className="page-sub">{t("settings.subtitle")}</p>
        </div>
      </header>

      <Section title={t("settings.appearance")}>
        <Segmented<Theme>
          label={t("settings.theme")}
          value={draft.theme}
          onChange={(theme) => set({ theme })}
          options={[
            { value: "system", label: t("settings.theme.system") },
            { value: "dark", label: t("settings.theme.dark") },
            { value: "light", label: t("settings.theme.light") },
          ]}
        />
        <SelectField
          label={t("settings.uiLanguage")}
          value={draft.uiLanguage ?? ""}
          onChange={(v) => set({ uiLanguage: v || null })}
          hint={t("settings.uiLanguageHint")}
          options={[{ value: "", label: t("settings.followSystem") }, ...UI_LANGUAGES.map((l) => ({ value: l.code, label: l.label }))]}
        />
      </Section>

      <Section title={t("settings.runtime")}>
        {runners.error && <ErrorState error={runners.error} compact />}
        <SelectField
          label={t("settings.defaultRunner")}
          value={draft.defaultRunner?.id ?? ""}
          onChange={(v) => set({ defaultRunner: v ? { id: v } : null })}
          hint={t("settings.defaultRunnerHint")}
          options={[{ value: "", label: t("settings.noDefaultRunner") }, ...(runners.data ?? []).map((r) => ({ value: r.id, label: `${runnerLabel(r)} · ${t(`runner.kind.${r.kind}`)}` }))]}
        />
        <PathField label={t("settings.prefixesDir")} kind="directory" clearable value={draft.prefixesDir ?? ""} onChange={(v) => set({ prefixesDir: v.trim() ? v : null })} placeholder={t("settings.prefixesDirDefault")} hint={t("settings.prefixesDirHint")} />
      </Section>

      <Section title={t("settings.translation")} description={t("settings.translationDesc")}>
        {!draft.translation.targetLanguage && (
          <Callout tone="warning" role="alert" title={t("tr.chooseTarget")}>
            {t("settings.noTargetText")}
          </Callout>
        )}
        <div className="grid-3">
          <LanguageSelect label={t("tr.source")} value={draft.translation.sourceLanguage || null} onChange={(v) => set({ translation: { ...draft.translation, sourceLanguage: v ?? draft.translation.sourceLanguage } })} />
          <LanguageSelect label={t("settings.defaultTarget")} value={draft.translation.targetLanguage} onChange={(targetLanguage) => set({ translation: { ...draft.translation, targetLanguage } })} unsetLabel={t("settings.targetNotSet")} hint={t("settings.defaultTargetHint")} />
          <SelectField
            label={t("settings.defaultProvider")}
            value={draft.translation.providerId ?? ""}
            onChange={(v) => set({ translation: { ...draft.translation, providerId: v || null } })}
            options={[{ value: "", label: t("settings.noProvider") }, ...(providers.data ?? []).map((p) => ({ value: p.id, label: providerLabel(p, t) }))]}
          />
        </div>
      </Section>

      <Section title={t("settings.vndb")} description={t("settings.vndbDesc")}>
        <SwitchRow label={t("settings.vndbEnabled")} description={t("settings.vndbEnabledHint")} checked={draft.vndb.enabled} onChange={(enabled) => set({ vndb: { ...draft.vndb, enabled } })} />
        <SelectField
          label={t("settings.blur")}
          value={blurValue}
          onChange={(v) => {
            const opt = BLUR_OPTIONS.find(([k]) => k === v);
            if (opt) set({ vndb: { ...draft.vndb, blurSexualThreshold: opt[1] } });
          }}
          hint={t("settings.blurHint")}
          options={[
            ...BLUR_OPTIONS.map(([k]) => ({ value: k, label: t(`settings.blur.${k as "never" | "suggestive" | "explicit" | "all"}`) })),
            ...(blurValue === "custom" ? [{ value: "custom", label: `≥ ${draft.vndb.blurSexualThreshold}` }] : []),
          ]}
        />
      </Section>

      <Section title={t("settings.defaultGraphics")} description={t("settings.defaultGraphicsDesc")}>
        <GraphicsEditor value={draft.defaultGraphics} onChange={(defaultGraphics) => set({ defaultGraphics })} errors={errors} runnerId={draft.defaultRunner?.id} errorPrefix="defaultGraphics" />
      </Section>

      {(dirty || saving) && (
        <div className="save-bar" role="region" aria-label={t("form.unsavedRegion")}>
          <div className="save-bar-text">
            <strong>{t("form.unsaved")}</strong>
            {errorCount > 0 && <span className="text-danger">{t("form.problems", { n: errorCount })}</span>}
            {saveError && <span className="text-danger">{saveError.message}</span>}
          </div>
          <div className="row gap-sm">
            <Button variant="ghost" icon={Undo2} onClick={() => setDraft(saved)} disabled={saving}>
              {t("form.revert")}
            </Button>
            <Button variant="primary" icon={Save} onClick={() => void doSave()} loading={saving} disabled={errorCount > 0}>
              {saving ? t("form.saving") : t("form.save")}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
