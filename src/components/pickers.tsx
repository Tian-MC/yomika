import type { ReactNode } from "react";
import type { PrefixChoice, PrefixInfo, Runner, RunnerRef } from "../contract";
import { useT } from "../i18n";
import { useLanguages } from "../state/app";
import { PathField, Segmented, SelectField, type SelectOption } from "./ui/form";

// --------------------------------------------------------- LanguageSelect

interface LanguageSelectProps {
  label: ReactNode;
  value: string | null;
  onChange: (code: string | null) => void;
  /**
   * When set, an extra first option maps to `null` ("use app default").
   * The label should mention what the default resolves to.
   */
  defaultOptionLabel?: string;
  /** Label of the empty option when there is no "default" option (value null). */
  unsetLabel?: string;
  hint?: ReactNode;
  error?: string | null;
  exclude?: string[];
}

/**
 * Language picker fed by `translation_languages`. Never pre-selects a
 * language on its own: `null` stays `null` until the user picks.
 */
export function LanguageSelect({ label, value, onChange, defaultOptionLabel, unsetLabel, hint, error, exclude }: LanguageSelectProps) {
  const t = useT();
  const { languages } = useLanguages();
  const options: SelectOption[] = [];
  if (defaultOptionLabel !== undefined) options.push({ value: "", label: defaultOptionLabel });
  else options.push({ value: "", label: unsetLabel ?? t("lang.chooseLanguage"), disabled: value !== null });
  for (const l of languages ?? []) {
    if (exclude?.includes(l.code)) continue;
    options.push({ value: l.code, label: l.nativeName && l.nativeName !== l.name ? `${l.name} — ${l.nativeName}` : l.name });
  }
  return <SelectField label={label} value={value ?? ""} onChange={(v) => onChange(v === "" ? null : v)} options={options} hint={hint} error={error} disabled={languages === null} />;
}

// ----------------------------------------------------------- RunnerSelect

export function runnerLabel(r: Runner): string {
  return r.version && !r.name.includes(r.version) ? `${r.name} (${r.version})` : r.name;
}

export function RunnerSelect({ label, value, onChange, runners, defaultRunnerId, allowDefault = true, hint, error }: { label: ReactNode; value: RunnerRef | null; onChange: (v: RunnerRef | null) => void; runners: Runner[] | undefined; defaultRunnerId?: string | null; allowDefault?: boolean; hint?: ReactNode; error?: string | null }) {
  const t = useT();
  const def = runners?.find((r) => r.id === defaultRunnerId);
  const options: SelectOption[] = [];
  if (allowDefault) options.push({ value: "", label: def ? t("runner.appDefaultNamed", { name: runnerLabel(def) }) : t("runner.appDefault") });
  for (const kind of ["proton", "wine", "native"] as const) {
    for (const r of runners ?? []) if (r.kind === kind) options.push({ value: r.id, label: `${runnerLabel(r)} · ${t(`runner.kind.${kind}`)}` });
  }
  return <SelectField label={label} value={value?.id ?? ""} onChange={(v) => onChange(v ? { id: v } : null)} options={options} hint={hint} error={error} placeholder={allowDefault ? undefined : t("runner.choose")} />;
}

// ---------------------------------------------------- PrefixChoiceEditor

export function PrefixChoiceEditor({ value, onChange, prefixes, error }: { value: PrefixChoice; onChange: (v: PrefixChoice) => void; prefixes: PrefixInfo[] | undefined; error?: string | null }) {
  const t = useT();
  const shared = (prefixes ?? []).filter((p) => p.kind.type !== "dedicated");
  return (
    <div className="stack gap-sm">
      <Segmented
        label={t("prefix.choice")}
        value={value.type}
        onChange={(type) => {
          if (type === value.type) return;
          if (type === "dedicated") onChange({ type: "dedicated" });
          else if (type === "shared") onChange({ type: "shared", id: shared[0]?.id ?? "" });
          else onChange({ type: "custom", path: "" });
        }}
        options={[
          { value: "dedicated", label: t("prefix.dedicated") },
          { value: "shared", label: t("prefix.shared") },
          { value: "custom", label: t("prefix.custom") },
        ]}
      />
      <p className="field-hint">
        {value.type === "dedicated" ? t("prefix.dedicatedHint") : value.type === "shared" ? t("prefix.sharedHint") : t("prefix.customHint")}
      </p>
      {value.type === "shared" && (
        <SelectField
          label={t("prefix.sharedSelect")}
          value={value.id}
          onChange={(id) => onChange({ type: "shared", id })}
          placeholder={t("prefix.choosePrefix")}
          error={error}
          options={shared.map((p) => ({ value: p.id, label: `${p.name} · ${p.arch}${p.managed ? "" : ` · ${t("prefix.external")}`}` }))}
        />
      )}
      {value.type === "custom" && <PathField label={t("prefix.customPath")} kind="directory" value={value.path} onChange={(path) => onChange({ type: "custom", path })} error={error} demoDefault="/home/demo/Games/wine-prefixes/custom" />}
    </div>
  );
}
