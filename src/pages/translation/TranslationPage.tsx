import { BookMarked, Cpu, FlaskConical, Globe, History, KeyRound, Languages, Lock, Plus, RefreshCw, Save, ScanText, Trash } from "lucide-react";
import { useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { LanguageSelect } from "../../components/pickers";
import { Button, IconButton } from "../../components/ui/Button";
import { Badge, Callout, EmptyState, ErrorState, LoadingBlock, Section } from "../../components/ui/feedback";
import { Field, NumberField, SelectField, Switch, SwitchRow, TextareaField, TextField } from "../../components/ui/form";
import { ConfirmDialog } from "../../components/ui/Modal";
import { Tabs, tabPanelProps } from "../../components/ui/Tabs";
import type { AppError, BackendInfo, GlossaryEntry, GlossaryKind, ProviderConfigField, ProviderInfo, TranslationLine } from "../../contract";
import { useI18n } from "../../i18n";
import { call, normalizeError } from "../../lib/api";
import { formatDateTime } from "../../lib/format";
import { useCommand, useEvent } from "../../lib/hooks";
import { useAppSettings, useLanguages, useLibrary } from "../../state/app";
import { useToasts } from "../../state/toasts";
import { LineView, providerLabel } from "../game/TranslationTab";

type TrTab = "translate" | "history" | "glossary" | "providers" | "sources";
const TABS: TrTab[] = ["translate", "history", "glossary", "providers", "sources"];

export function TranslationPage() {
  const { t } = useI18n();
  const [params, setParams] = useSearchParams();
  const p = params.get("tab") as TrTab | null;
  const tab: TrTab = p && TABS.includes(p) ? p : "translate";
  const icons = { translate: Languages, history: History, glossary: BookMarked, providers: Globe, sources: ScanText } as const;
  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>{t("trp.title")}</h1>
          <p className="page-sub">{t("trp.subtitle")}</p>
        </div>
      </header>
      <Tabs<TrTab> label={t("trp.title")} idBase="trp" value={tab} onChange={(v) => setParams(v === "translate" ? {} : { tab: v }, { replace: true })} tabs={TABS.map((v) => ({ value: v, label: t(`trp.tab.${v}`), icon: icons[v] }))} />
      <div className="tab-panel" {...tabPanelProps("trp", tab)}>
        {tab === "translate" && <ManualTranslate />}
        {tab === "history" && <HistoryPanel />}
        {tab === "glossary" && <GlossaryPanel />}
        {tab === "providers" && <ProvidersPanel />}
        {tab === "sources" && <SourcesPanel />}
      </div>
    </div>
  );
}

// ------------------------------------------------------------ translate

function ManualTranslate() {
  const { t } = useI18n();
  const { appSettings } = useAppSettings();
  const { nameFor } = useLanguages();
  const providers = useCommand("translation_providers", {});
  const [text, setText] = useState("");
  const [source, setSource] = useState<string | null>(null);
  const [target, setTarget] = useState<string | null>(null);
  const [providerId, setProviderId] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<TranslationLine | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const appTarget = appSettings?.translation.targetLanguage ?? null;
  const appSource = appSettings?.translation.sourceLanguage ?? null;
  const appProvider = providers.data?.find((p) => p.id === appSettings?.translation.providerId);
  const effectiveTarget = target ?? appTarget;
  const effectiveProvider = providers.data?.find((p) => p.id === (providerId || appSettings?.translation.providerId));

  const translate = async () => {
    setBusy(true);
    setError(null);
    try {
      setResult(await call("translation_translate", { request: { text, sourceLanguage: source, targetLanguage: target, providerId: providerId || null } }));
    } catch (e) {
      setError(normalizeError(e));
      setResult(null);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title={t("trp.manual")} description={t("trp.manualDesc")}>
      {!effectiveTarget && (
        <Callout tone="warning" role="alert" icon={Languages} title={t("tr.chooseTarget")}>
          {t("trp.chooseTargetText")}
        </Callout>
      )}
      <div className="grid-3">
        <LanguageSelect label={t("tr.source")} value={source} onChange={setSource} defaultOptionLabel={appSource ? t("tr.useAppDefaultNamed", { name: nameFor(appSource) ?? appSource }) : t("tr.useAppDefaultUnset")} />
        <LanguageSelect label={t("tr.target")} value={target} onChange={setTarget} defaultOptionLabel={appTarget ? t("tr.useAppDefaultNamed", { name: nameFor(appTarget) ?? appTarget }) : t("tr.useAppDefaultUnset")} />
        <SelectField label={t("tr.provider")} value={providerId} onChange={setProviderId} options={[{ value: "", label: appProvider ? t("tr.useAppDefaultNamed", { name: appProvider.name }) : t("tr.useAppDefaultUnset") }, ...(providers.data ?? []).map((p) => ({ value: p.id, label: providerLabel(p, t) }))]} />
      </div>
      {effectiveProvider?.sendsDataRemotely && (
        <Callout tone="warning" icon={Globe} title={t("provider.remoteTitle")}>
          {t("provider.remoteText", { name: effectiveProvider.name })}
        </Callout>
      )}
      <TextareaField label={t("trp.text")} value={text} onChange={setText} rows={5} lang={source ?? appSource ?? "ja"} placeholder="「またここで会ったね」" />
      <div className="row gap-sm">
        <Button variant="primary" icon={Languages} onClick={() => void translate()} loading={busy} disabled={!text.trim() || !effectiveTarget}>
          {t("trp.translate")}
        </Button>
      </div>
      {error && <ErrorState error={error} />}
      {result && (
        <div className="result-card" aria-live="polite">
          <LineView line={result} />
          {result.providerId && <p className="muted small">{t("trp.translatedBy", { provider: providers.data?.find((p) => p.id === result.providerId)?.name ?? result.providerId })}</p>}
        </div>
      )}
    </Section>
  );
}

// -------------------------------------------------------------- history

function GameScopeSelect({ value, onChange, allowAll, label }: { value: number | null | "all"; onChange: (v: number | null | "all") => void; allowAll?: boolean; label: string }) {
  const { t } = useI18n();
  const { games } = useLibrary();
  return (
    <label className="compact-field">
      <span>{label}</span>
      <select
        className="input select input-sm"
        value={value === null ? "global" : String(value)}
        onChange={(e) => {
          const v = e.target.value;
          onChange(v === "global" ? null : v === "all" ? "all" : Number(v));
        }}
      >
        {allowAll && <option value="all">{t("trp.allEntries")}</option>}
        <option value="global">{allowAll ? t("trp.noGame") : t("trp.global")}</option>
        {(games ?? []).map((g) => (
          <option key={g.id} value={g.id}>
            {g.title}
          </option>
        ))}
      </select>
    </label>
  );
}

function HistoryPanel() {
  const { t, lang } = useI18n();
  const toasts = useToasts();
  const library = useLibrary();
  const [scope, setScope] = useState<number | null | "all">("all");
  const history = useCommand("translation_history", { gameId: scope === "all" ? null : scope, limit: 300 });
  const [confirm, setConfirm] = useState(false);
  const [clearing, setClearing] = useState(false);

  useEvent("vnl://translation", (line) => {
    if (scope === "all" || line.gameId === scope) history.mutate((list) => [line, ...(list ?? [])].slice(0, 300));
  });

  const clear = async () => {
    setClearing(true);
    try {
      await call("translation_history_clear", { gameId: scope === "all" ? null : scope });
      history.mutate([]);
      setConfirm(false);
      toasts.success(t("trp.historyCleared"));
    } catch (e) {
      toasts.error(e, t("trp.historyClearFailed"));
    } finally {
      setClearing(false);
    }
  };

  const list = scope === null ? (history.data ?? []).filter((l) => l.gameId === null) : (history.data ?? []);

  return (
    <Section
      title={t("trp.history")}
      actions={
        <div className="row gap-sm">
          <GameScopeSelect label={t("trp.scope")} value={scope} onChange={setScope} allowAll />
          <IconButton icon={RefreshCw} label={t("common.refresh")} size="sm" onClick={() => void history.reload()} />
          <Button size="sm" variant="danger" icon={Trash} onClick={() => setConfirm(true)} disabled={!list.length}>
            {t("trp.clearHistory")}
          </Button>
        </div>
      }
    >
      {history.error && <ErrorState error={history.error} onRetry={() => void history.reload()} />}
      {history.loading && <LoadingBlock />}
      {history.data && list.length === 0 && <EmptyState icon={History} title={t("trp.noHistory")} />}
      {list.length > 0 && (
        <ul className="history-list">
          {list.map((l) => (
            <li key={l.id} className="history-item">
              <div className="history-meta muted small">
                {formatDateTime(Math.floor(l.tsMs / 1000), lang)}
                {l.gameId !== null && ` · ${library.byId(l.gameId)?.title ?? `#${l.gameId}`}`}
              </div>
              <LineView line={l} />
            </li>
          ))}
        </ul>
      )}
      <ConfirmDialog open={confirm} title={t("trp.clearHistoryTitle")} confirmLabel={t("trp.clearHistory")} variant="danger" busy={clearing} onCancel={() => setConfirm(false)} onConfirm={() => void clear()}>
        <p>{scope === "all" ? t("trp.clearAllText") : t("trp.clearScopeText")}</p>
      </ConfirmDialog>
    </Section>
  );
}

// ------------------------------------------------------------- glossary

function GlossaryPanel() {
  const { t } = useI18n();
  const toasts = useToasts();
  const [scope, setScope] = useState<number | null>(null);
  const glossary = useCommand("translation_glossary_list", { gameId: scope });
  const [kindFilter, setKindFilter] = useState<GlossaryKind | "">("");
  const [draft, setDraft] = useState<GlossaryEntry | null>(null);
  const [saving, setSaving] = useState(false);

  const startNew = (kind: GlossaryKind) => setDraft({ id: 0, gameId: scope, kind, source: "", target: "", note: null, enabled: true });

  const save = async (entry: GlossaryEntry) => {
    setSaving(true);
    try {
      const saved = await call("translation_glossary_upsert", { entry });
      glossary.mutate((list) => {
        const l = list ?? [];
        return l.some((e) => e.id === saved.id) ? l.map((e) => (e.id === saved.id ? saved : e)) : [...l, saved];
      });
      setDraft(null);
    } catch (e) {
      toasts.error(e, t("trp.glossarySaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  const remove = async (id: number) => {
    try {
      await call("translation_glossary_delete", { id });
      glossary.mutate((list) => (list ?? []).filter((e) => e.id !== id));
    } catch (e) {
      toasts.error(e, t("trp.glossaryDeleteFailed"));
    }
  };

  const entries = (glossary.data ?? []).filter((e) => (scope === null ? e.gameId === null : e.gameId === scope)).filter((e) => !kindFilter || e.kind === kindFilter);

  return (
    <Section
      title={t("trp.glossary")}
      description={t("trp.glossaryDesc")}
      actions={
        <div className="row gap-sm wrap">
          <GameScopeSelect label={t("trp.scope")} value={scope} onChange={(v) => setScope(v === "all" ? null : v)} />
          <label className="compact-field">
            <span>{t("trp.kind")}</span>
            <select className="input select input-sm" value={kindFilter} onChange={(e) => setKindFilter(e.target.value as GlossaryKind | "")}>
              <option value="">{t("trp.allKinds")}</option>
              <option value="term">{t("trp.kind.term")}</option>
              <option value="characterName">{t("trp.kind.characterName")}</option>
            </select>
          </label>
          <Button size="sm" icon={Plus} onClick={() => startNew("term")}>
            {t("trp.addTerm")}
          </Button>
          <Button size="sm" icon={Plus} onClick={() => startNew("characterName")}>
            {t("trp.addName")}
          </Button>
        </div>
      }
    >
      {glossary.error && <ErrorState error={glossary.error} onRetry={() => void glossary.reload()} />}
      {glossary.loading && <LoadingBlock />}
      {draft && <GlossaryEditor entry={draft} onCancel={() => setDraft(null)} onSave={(e) => void save(e)} saving={saving} />}
      {glossary.data && entries.length === 0 && !draft && <EmptyState icon={BookMarked} title={t("trp.noGlossary")}>{t("trp.noGlossaryHint")}</EmptyState>}
      {entries.length > 0 && (
        <table className="table">
          <thead>
            <tr>
              <th>{t("trp.source")}</th>
              <th>{t("trp.targetText")}</th>
              <th>{t("trp.kind")}</th>
              <th>{t("trp.note")}</th>
              <th>{t("trp.enabled")}</th>
              <th className="sr-only">{t("common.actions")}</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <tr key={e.id} className={e.enabled ? undefined : "is-disabled"}>
                <td lang="ja" className="jp-cell">
                  {e.source}
                </td>
                <td>{e.target}</td>
                <td>
                  <Badge tone={e.kind === "characterName" ? "rose" : "neutral"}>{t(`trp.kind.${e.kind}`)}</Badge>
                </td>
                <td className="muted small">{e.note}</td>
                <td>
                  <Switch checked={e.enabled} label={t("trp.enabled")} onChange={(enabled) => void save({ ...e, enabled })} />
                </td>
                <td className="num">
                  <div className="row gap-xs">
                    <Button size="sm" variant="ghost" onClick={() => setDraft(e)}>
                      {t("common.edit")}
                    </Button>
                    <IconButton icon={Trash} size="sm" label={t("common.remove")} onClick={() => void remove(e.id)} />
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Section>
  );
}

function GlossaryEditor({ entry, onCancel, onSave, saving }: { entry: GlossaryEntry; onCancel: () => void; onSave: (e: GlossaryEntry) => void; saving: boolean }) {
  const { t } = useI18n();
  const [e, setE] = useState(entry);
  return (
    <form
      className="glossary-editor"
      onSubmit={(ev) => {
        ev.preventDefault();
        if (e.source.trim()) onSave({ ...e, source: e.source.trim(), target: e.target.trim() });
      }}
    >
      <div className="grid-3">
        <TextField label={t("trp.source")} value={e.source} onChange={(source) => setE({ ...e, source })} lang="ja" autoFocus placeholder={e.kind === "characterName" ? "岡部倫太郎" : "先輩"} />
        <TextField label={t("trp.targetText")} value={e.target} onChange={(target) => setE({ ...e, target })} />
        <SelectField label={t("trp.kind")} value={e.kind} onChange={(k) => setE({ ...e, kind: k as GlossaryKind })} options={[{ value: "term", label: t("trp.kind.term") }, { value: "characterName", label: t("trp.kind.characterName") }]} />
      </div>
      <TextField label={t("trp.note")} value={e.note ?? ""} onChange={(note) => setE({ ...e, note: note || null })} />
      <div className="row gap-sm">
        <Button variant="ghost" onClick={onCancel}>
          {t("common.cancel")}
        </Button>
        <Button type="submit" variant="primary" icon={Save} loading={saving} disabled={!e.source.trim()}>
          {t("common.save")}
        </Button>
      </div>
    </form>
  );
}

// ------------------------------------------------------------ providers

function ProvidersPanel() {
  const { t } = useI18n();
  const providers = useCommand("translation_providers", {});
  return (
    <div className="stack gap-lg">
      <Callout tone="info" icon={Lock} title={t("provider.privacyTitle")}>
        {t("provider.privacyText")}
      </Callout>
      {providers.error && <ErrorState error={providers.error} onRetry={() => void providers.reload()} />}
      {providers.loading && <LoadingBlock />}
      {providers.data?.length === 0 && <EmptyState icon={Globe} title={t("provider.none")} />}
      {providers.data?.map((p) => (
        <ProviderCard key={p.id} provider={p} onSaved={(np) => providers.mutate((list) => (list ?? []).map((x) => (x.id === np.id ? np : x)))} />
      ))}
    </div>
  );
}

function initialConfig(p: ProviderInfo): Record<string, unknown> {
  const c: Record<string, unknown> = {};
  for (const f of p.fields) c[f.key] = p.config[f.key] ?? f.default ?? (f.kind === "boolean" ? false : f.kind === "number" ? null : "");
  for (const [k, v] of Object.entries(p.config)) if (!(k in c)) c[k] = v;
  return c;
}

function ProviderCard({ provider, onSaved }: { provider: ProviderInfo; onSaved: (p: ProviderInfo) => void }) {
  const { t } = useI18n();
  const toasts = useToasts();
  const { appSettings } = useAppSettings();
  const [config, setConfig] = useState(() => initialConfig(provider));
  const [base, setBase] = useState(provider);
  if (base !== provider) {
    setBase(provider);
    setConfig(initialConfig(provider));
  }
  /** UI state: undefined = keep stored key, null = delete it, string = new key.
   *  Wire format (contract): omitted/null = keep, "" = delete, non-empty = store. */
  const [apiKey, setApiKey] = useState<string | null | undefined>(undefined);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testTarget, setTestTarget] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<TranslationLine | null>(null);
  const [testError, setTestError] = useState<AppError | null>(null);
  const dirty = JSON.stringify(config) !== JSON.stringify(initialConfig(provider)) || apiKey !== undefined;
  const missing = provider.fields.filter((f) => f.required && (config[f.key] === null || config[f.key] === undefined || config[f.key] === ""));
  const effectiveTestTarget = testTarget ?? appSettings?.translation.targetLanguage ?? null;

  const save = async () => {
    setSaving(true);
    try {
      const args: { providerId: string; config: Record<string, unknown>; apiKey?: string | null } = { providerId: provider.id, config };
      if (apiKey === null) args.apiKey = "";
      else if (typeof apiKey === "string" && apiKey.length > 0) args.apiKey = apiKey;
      const updated = await call("translation_configure_provider", args);
      setApiKey(undefined);
      onSaved(updated);
      toasts.success(t("provider.saved", { name: provider.name }));
    } catch (e) {
      toasts.error(e, t("provider.saveFailed", { name: provider.name }));
    } finally {
      setSaving(false);
    }
  };

  const test = async () => {
    if (!effectiveTestTarget) return;
    setTesting(true);
    setTestError(null);
    setTestResult(null);
    try {
      setTestResult(await call("translation_test_provider", { providerId: provider.id, targetLanguage: effectiveTestTarget }));
    } catch (e) {
      setTestError(normalizeError(e));
    } finally {
      setTesting(false);
    }
  };

  return (
    <article className="provider-card" aria-labelledby={`prov-${provider.id}`}>
      <header className="provider-head">
        <div>
          <h3 id={`prov-${provider.id}`}>{provider.name}</h3>
          {provider.notes && <p className="muted small">{provider.notes}</p>}
        </div>
        <div className="row gap-xs wrap">
          {provider.sendsDataRemotely ? (
            <Badge tone="warning" icon={Globe}>
              {t("provider.remote")}
            </Badge>
          ) : (
            <Badge tone="success" icon={Cpu}>
              {t("provider.local")}
            </Badge>
          )}
          {provider.available ? provider.configured ? <Badge tone="success">{t("provider.configured")}</Badge> : <Badge tone="warning">{t("provider.notConfigured")}</Badge> : <Badge tone="danger">{t("common.unavailable")}</Badge>}
          {appSettings?.translation.providerId === provider.id && <Badge tone="accent">{t("runtime.default")}</Badge>}
        </div>
      </header>
      {provider.sendsDataRemotely && (
        <Callout tone="warning" icon={Globe} title={t("provider.remoteTitle")}>
          {t("provider.remoteText", { name: provider.name })}
        </Callout>
      )}
      <form
        className="stack gap-md"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        {provider.fields.length > 0 && (
          <div className="grid-2">
            {provider.fields.map((f) => (
              <ProviderField key={f.key} field={f} value={config[f.key]} onChange={(v) => setConfig({ ...config, [f.key]: v })} />
            ))}
          </div>
        )}
        {provider.requiresApiKey && (
          <div className="api-key">
            <Field label={<span className="row gap-xs"><KeyRound size={14} aria-hidden /> {t("provider.apiKey")}</span>} hint={apiKey === null ? t("provider.keyWillDelete") : provider.hasApiKey ? t("provider.keyStored") : t("provider.keyNone")}>
              {(id, describedBy) => (
                <div className="row gap-sm">
                  <input
                    id={id}
                    className="input mono grow"
                    type="password"
                    autoComplete="new-password"
                    aria-describedby={describedBy}
                    placeholder={provider.hasApiKey ? "••••••••••••" : t("provider.keyPlaceholder")}
                    value={typeof apiKey === "string" ? apiKey : ""}
                    disabled={apiKey === null}
                    onChange={(e) => setApiKey(e.target.value === "" ? undefined : e.target.value)}
                  />
                  {provider.hasApiKey && (
                    <Button size="sm" variant={apiKey === null ? "secondary" : "ghost"} onClick={() => setApiKey(apiKey === null ? undefined : null)}>
                      {apiKey === null ? t("provider.keepKey") : t("provider.deleteKey")}
                    </Button>
                  )}
                </div>
              )}
            </Field>
          </div>
        )}
        {missing.length > 0 && <p className="field-error">{t("provider.missingFields", { fields: missing.map((f) => f.label).join(", ") })}</p>}
        <div className="row gap-sm wrap align-end">
          <Button type="submit" variant="primary" icon={Save} loading={saving} disabled={!dirty}>
            {t("common.save")}
          </Button>
          <span className="grow" />
          <div className="test-target">
            <LanguageSelect label={t("provider.testTarget")} value={testTarget} onChange={setTestTarget} defaultOptionLabel={appSettings?.translation.targetLanguage ? t("tr.useAppDefaultNamed", { name: appSettings.translation.targetLanguage }) : t("tr.useAppDefaultUnset")} />
          </div>
          <Button icon={FlaskConical} onClick={() => void test()} loading={testing} disabled={!effectiveTestTarget || !provider.available} title={!effectiveTestTarget ? t("tr.chooseTarget") : undefined}>
            {t("provider.test")}
          </Button>
        </div>
      </form>
      {testError && <ErrorState error={testError} compact />}
      {testResult && <LineView line={testResult} />}
    </article>
  );
}

function ProviderField({ field, value, onChange }: { field: ProviderConfigField; value: unknown; onChange: (v: unknown) => void }) {
  const { t } = useI18n();
  const label = field.required ? `${field.label} *` : field.label;
  switch (field.kind) {
    case "boolean":
      return <SwitchRow label={field.label} checked={value === true} onChange={onChange} />;
    case "number":
      return <NumberField label={label} value={typeof value === "number" ? value : null} onChange={onChange} placeholder={field.default !== null ? String(field.default) : undefined} />;
    case "select":
      return <SelectField label={label} value={typeof value === "string" ? value : ""} onChange={onChange} placeholder={t("common.choose")} options={(field.options ?? []).map((o) => ({ value: o, label: o }))} />;
    default:
      return <TextField label={label} type={field.kind === "url" ? "url" : "text"} mono={field.kind === "url"} value={typeof value === "string" ? value : typeof value === "number" || typeof value === "boolean" ? String(value) : value === null || value === undefined ? "" : JSON.stringify(value)} onChange={onChange} placeholder={field.default !== null ? String(field.default) : undefined} />;
  }
}

// -------------------------------------------------------------- sources

function SourcesPanel() {
  const { t } = useI18n();
  const ocr = useCommand("translation_ocr_backends", {});
  const hook = useCommand("translation_hook_backends", {});
  const [capturing, setCapturing] = useState(false);
  const [line, setLine] = useState<TranslationLine | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [backend, setBackend] = useState("");

  const capture = async () => {
    setCapturing(true);
    setError(null);
    try {
      setLine(await call("translation_ocr_capture", { request: { backend: backend || null, translate: true } }));
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setCapturing(false);
    }
  };

  return (
    <div className="stack gap-lg">
      <Section title={t("trp.hookBackends")} description={t("trp.hookBackendsDesc")} actions={<IconButton icon={RefreshCw} size="sm" label={t("common.refresh")} onClick={() => void hook.reload()} />}>
        {hook.error && <ErrorState error={hook.error} compact onRetry={() => void hook.reload()} />}
        {hook.loading && <LoadingBlock />}
        {hook.data && <BackendList list={hook.data} />}
      </Section>
      <Section title={t("trp.ocrBackends")} description={t("trp.ocrBackendsDesc")} actions={<IconButton icon={RefreshCw} size="sm" label={t("common.refresh")} onClick={() => void ocr.reload()} />}>
        {ocr.error && <ErrorState error={ocr.error} compact onRetry={() => void ocr.reload()} />}
        {ocr.loading && <LoadingBlock />}
        {ocr.data && <BackendList list={ocr.data} />}
        <div className="subsection">
          <h3>{t("trp.ocrTest")}</h3>
          <p className="muted small">{t("trp.ocrTestHint")}</p>
          <div className="row gap-sm align-end">
            <SelectField label={t("tr.ocrBackend")} value={backend} onChange={setBackend} options={[{ value: "", label: t("tr.autoBackend") }, ...(ocr.data ?? []).map((b) => ({ value: b.id, label: b.available ? b.name : `${b.name} · ${t("common.unavailable")}`, disabled: !b.available }))]} />
            <Button icon={ScanText} onClick={() => void capture()} loading={capturing}>
              {t("trp.ocrCapture")}
            </Button>
          </div>
          {error && <ErrorState error={error} compact />}
          {line && <LineView line={line} />}
        </div>
      </Section>
      <p className="muted small">
        {t("trp.perGameHint")} <Link to="/">{t("nav.library")}</Link>
      </p>
    </div>
  );
}

function BackendList({ list }: { list: BackendInfo[] }) {
  const { t } = useI18n();
  if (list.length === 0) return <p className="muted">{t("trp.noBackends")}</p>;
  return (
    <ul className="backend-list">
      {list.map((b) => (
        <li key={b.id} className="backend">
          <div className="row gap-sm">
            <strong>{b.name}</strong>
            <code className="small muted">{b.id}</code>
            {b.available ? <Badge tone="success">{t("common.available")}</Badge> : <Badge tone="warning">{t("common.unavailable")}</Badge>}
          </div>
          {b.diagnostics.length > 0 && (
            <ul className="plain-list muted small">
              {b.diagnostics.map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
          )}
        </li>
      ))}
    </ul>
  );
}
