import { FolderOpen, Plus, Trash, X } from "lucide-react";
import { useId, useState, type KeyboardEvent, type ReactNode } from "react";
import { useT } from "../../i18n";
import { pickDirectory, pickFile, type PickFileOptions } from "../../lib/dialogs";
import { joinArgs, splitArgs } from "../../lib/format";
import { useToasts } from "../../state/toasts";
import { Button, IconButton } from "./Button";

// ------------------------------------------------------------------ Field

interface FieldProps {
  label: ReactNode;
  hint?: ReactNode;
  error?: string | null;
  /** Render the control. Pass `id` and `describedBy` to the input. */
  children: (id: string, describedBy: string | undefined, invalid: boolean) => ReactNode;
  className?: string;
  inline?: boolean;
}

export function Field({ label, hint, error, children, className, inline }: FieldProps) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errId = `${id}-err`;
  const describedBy = [hint ? hintId : null, error ? errId : null].filter(Boolean).join(" ") || undefined;
  return (
    <div className={["field", inline ? "field-inline" : null, error ? "has-error" : null, className].filter(Boolean).join(" ")}>
      <label className="field-label" htmlFor={id}>
        {label}
      </label>
      <div className="field-control">{children(id, describedBy, !!error)}</div>
      {hint && !error && (
        <div className="field-hint" id={hintId}>
          {hint}
        </div>
      )}
      {error && (
        <div className="field-error" id={errId} role="alert">
          {error}
        </div>
      )}
    </div>
  );
}

// -------------------------------------------------------------- TextField

interface TextFieldProps {
  label: ReactNode;
  value: string;
  onChange: (v: string) => void;
  hint?: ReactNode;
  error?: string | null;
  placeholder?: string;
  type?: "text" | "password" | "url" | "search";
  mono?: boolean;
  disabled?: boolean;
  autoFocus?: boolean;
  lang?: string;
  onEnter?: () => void;
  autoComplete?: string;
}

export function TextField({ label, value, onChange, hint, error, placeholder, type = "text", mono, disabled, autoFocus, lang, onEnter, autoComplete }: TextFieldProps) {
  return (
    <Field label={label} hint={hint} error={error}>
      {(id, describedBy, invalid) => (
        <input
          id={id}
          className={mono ? "input mono" : "input"}
          type={type}
          value={value}
          placeholder={placeholder}
          disabled={disabled}
          autoFocus={autoFocus}
          lang={lang}
          spellCheck={false}
          autoComplete={autoComplete ?? "off"}
          aria-describedby={describedBy}
          aria-invalid={invalid || undefined}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onEnter ? (e) => e.key === "Enter" && onEnter() : undefined}
        />
      )}
    </Field>
  );
}

/** Nullable string: empty input = null. */
export function OptionalTextField(props: Omit<TextFieldProps, "value" | "onChange"> & { value: string | null; onChange: (v: string | null) => void }) {
  const { value, onChange, ...rest } = props;
  return <TextField {...rest} value={value ?? ""} onChange={(v) => onChange(v.trim() === "" ? null : v)} />;
}

// ------------------------------------------------------------ NumberField

interface NumberFieldProps {
  label: ReactNode;
  value: number | null;
  onChange: (v: number | null) => void;
  hint?: ReactNode;
  error?: string | null;
  min?: number;
  max?: number;
  step?: number;
  placeholder?: string;
  disabled?: boolean;
  suffix?: string;
}

export function NumberField({ label, value, onChange, hint, error, min, max, step, placeholder, disabled, suffix }: NumberFieldProps) {
  const [text, setText] = useState(value === null ? "" : String(value));
  const [synced, setSynced] = useState<number | null>(value);
  if (synced !== value) {
    // External change (revert, load): resync the visible text.
    setSynced(value);
    setText(value === null ? "" : String(value));
  }
  return (
    <Field label={label} hint={hint} error={error}>
      {(id, describedBy, invalid) => (
        <div className="input-affix">
          <input
            id={id}
            className="input"
            type="number"
            inputMode="decimal"
            value={text}
            min={min}
            max={max}
            step={step}
            placeholder={placeholder}
            disabled={disabled}
            aria-describedby={describedBy}
            aria-invalid={invalid || undefined}
            onChange={(e) => {
              const raw = e.target.value;
              setText(raw);
              if (raw.trim() === "") {
                setSynced(null);
                onChange(null);
                return;
              }
              const n = Number(raw);
              if (Number.isFinite(n)) {
                setSynced(n);
                onChange(n);
              }
            }}
          />
          {suffix && <span className="affix">{suffix}</span>}
        </div>
      )}
    </Field>
  );
}

// ------------------------------------------------------------ SelectField

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

interface SelectFieldProps {
  label: ReactNode;
  value: string;
  onChange: (v: string) => void;
  options: SelectOption[];
  hint?: ReactNode;
  error?: string | null;
  disabled?: boolean;
  placeholder?: string;
}

export function SelectField({ label, value, onChange, options, hint, error, disabled, placeholder }: SelectFieldProps) {
  const hasValue = options.some((o) => o.value === value);
  return (
    <Field label={label} hint={hint} error={error}>
      {(id, describedBy, invalid) => (
        <select id={id} className="input select" value={value} disabled={disabled} aria-describedby={describedBy} aria-invalid={invalid || undefined} onChange={(e) => onChange(e.target.value)}>
          {placeholder !== undefined && (
            <option value="" disabled={value !== ""}>
              {placeholder}
            </option>
          )}
          {!hasValue && value !== "" && <option value={value}>{value}</option>}
          {options.map((o) => (
            <option key={o.value} value={o.value} disabled={o.disabled}>
              {o.label}
            </option>
          ))}
        </select>
      )}
    </Field>
  );
}

// ----------------------------------------------------------------- Switch

interface SwitchProps {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  labelledBy?: string;
  label?: string;
  describedBy?: string;
  id?: string;
}

export function Switch({ checked, onChange, disabled, labelledBy, label, describedBy, id }: SwitchProps) {
  return (
    <button
      id={id}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-labelledby={labelledBy}
      aria-label={labelledBy ? undefined : label}
      aria-describedby={describedBy}
      disabled={disabled}
      className={checked ? "switch is-on" : "switch"}
      onClick={() => onChange(!checked)}
    >
      <span className="switch-thumb" />
    </button>
  );
}

export function SwitchRow({ label, description, checked, onChange, disabled, children }: { label: ReactNode; description?: ReactNode; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; children?: ReactNode }) {
  const id = useId();
  return (
    <div className={disabled ? "switch-row is-disabled" : "switch-row"}>
      <div className="switch-row-text">
        <span className="switch-row-label" id={`${id}-l`} onClick={() => !disabled && onChange(!checked)}>
          {label}
        </span>
        {description && (
          <span className="switch-row-desc" id={`${id}-d`}>
            {description}
          </span>
        )}
        {children}
      </div>
      <Switch checked={checked} onChange={onChange} disabled={disabled} labelledBy={`${id}-l`} describedBy={description ? `${id}-d` : undefined} />
    </div>
  );
}

// -------------------------------------------------------------- Segmented

export function Segmented<V extends string>({ label, value, onChange, options, disabled, hideLabel }: { label: string; value: V; onChange: (v: V) => void; options: { value: V; label: ReactNode; title?: string }[]; disabled?: boolean; hideLabel?: boolean }) {
  const id = useId();
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const idx = options.findIndex((o) => o.value === value);
    let next: number;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (idx + 1) % options.length;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (idx - 1 + options.length) % options.length;
    else return;
    e.preventDefault();
    const opt = options[next];
    if (opt) {
      onChange(opt.value);
      const btn = e.currentTarget.querySelectorAll<HTMLButtonElement>("[role=radio]")[next];
      btn?.focus();
    }
  };
  return (
    <div className="segmented-wrap">
      <span id={id} className={hideLabel ? "sr-only" : "field-label"}>
        {label}
      </span>
      <div className="segmented" role="radiogroup" aria-labelledby={id} onKeyDown={onKey}>
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={o.value === value}
            tabIndex={o.value === value ? 0 : -1}
            title={o.title}
            disabled={disabled}
            className={o.value === value ? "seg is-active" : "seg"}
            onClick={() => onChange(o.value)}
          >
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

// -------------------------------------------------------------- Textarea

export function TextareaField({ label, value, onChange, hint, error, rows = 4, placeholder, lang, mono }: { label: ReactNode; value: string; onChange: (v: string) => void; hint?: ReactNode; error?: string | null; rows?: number; placeholder?: string; lang?: string; mono?: boolean }) {
  return (
    <Field label={label} hint={hint} error={error}>
      {(id, describedBy, invalid) => (
        <textarea id={id} className={mono ? "input textarea mono" : "input textarea"} rows={rows} value={value} placeholder={placeholder} lang={lang} aria-describedby={describedBy} aria-invalid={invalid || undefined} onChange={(e) => onChange(e.target.value)} />
      )}
    </Field>
  );
}

// -------------------------------------------------------------- PathField

interface PathFieldProps {
  label: ReactNode;
  value: string;
  onChange: (v: string) => void;
  kind?: "file" | "directory";
  filters?: PickFileOptions["filters"];
  hint?: ReactNode;
  error?: string | null;
  placeholder?: string;
  clearable?: boolean;
  demoDefault?: string;
}

export function PathField({ label, value, onChange, kind = "file", filters, hint, error, placeholder, clearable, demoDefault }: PathFieldProps) {
  const t = useT();
  const toasts = useToasts();
  const browse = async () => {
    try {
      const picked = kind === "directory" ? await pickDirectory({ defaultPath: value || undefined, demoDefault }) : await pickFile({ filters, defaultPath: value || undefined, demoDefault });
      if (picked) onChange(picked);
    } catch (e) {
      toasts.error(e, t("error.dialog"));
    }
  };
  return (
    <Field label={label} hint={hint} error={error}>
      {(id, describedBy, invalid) => (
        <div className="path-field">
          <input id={id} className="input mono" value={value} placeholder={placeholder} spellCheck={false} aria-describedby={describedBy} aria-invalid={invalid || undefined} onChange={(e) => onChange(e.target.value)} />
          {clearable && value && <IconButton icon={X} label={t("common.clear")} size="sm" onClick={() => onChange("")} />}
          <Button icon={FolderOpen} onClick={() => void browse()}>
            {t("common.browse")}
          </Button>
        </div>
      )}
    </Field>
  );
}

// -------------------------------------------------------------- ArgsField

/** string[] edited as one shell-like line (quotes supported, no shell is used). */
export function ArgsField({ label, value, onChange, hint, placeholder }: { label: ReactNode; value: string[]; onChange: (v: string[]) => void; hint?: ReactNode; placeholder?: string }) {
  const [text, setText] = useState(() => joinArgs(value));
  const [synced, setSynced] = useState(value);
  if (synced !== value) {
    setSynced(value);
    if (JSON.stringify(splitArgs(text)) !== JSON.stringify(value)) setText(joinArgs(value));
  }
  return (
    <Field label={label} hint={hint}>
      {(id, describedBy) => (
        <input
          id={id}
          className="input mono"
          value={text}
          placeholder={placeholder}
          spellCheck={false}
          aria-describedby={describedBy}
          onChange={(e) => {
            setText(e.target.value);
            const args = splitArgs(e.target.value);
            setSynced(args);
            onChange(args);
          }}
        />
      )}
    </Field>
  );
}

// -------------------------------------------------------- KeyValueEditor

interface KvRow {
  key: string;
  value: string;
}

function toRows(map: Record<string, string>): KvRow[] {
  return Object.entries(map).map(([key, value]) => ({ key, value }));
}

function toMap(rows: KvRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const r of rows) if (r.key.trim()) out[r.key.trim()] = r.value;
  return out;
}

function sameMap(a: Record<string, string>, b: Record<string, string>): boolean {
  const ka = Object.keys(a);
  if (ka.length !== Object.keys(b).length) return false;
  return ka.every((k) => b[k] === a[k]);
}

interface KeyValueEditorProps {
  label: string;
  value: Record<string, string>;
  onChange: (v: Record<string, string>) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  /** Return an error message for an invalid key. */
  validateKey?: (key: string) => string | null;
  valueOptions?: string[];
  description?: ReactNode;
}

export function KeyValueEditor({ label, value, onChange, keyPlaceholder, valuePlaceholder, validateKey, valueOptions, description }: KeyValueEditorProps) {
  const t = useT();
  const listId = useId();
  const [rows, setRows] = useState<KvRow[]>(() => toRows(value));
  const [synced, setSynced] = useState(value);
  if (synced !== value) {
    setSynced(value);
    if (!sameMap(toMap(rows), value)) setRows(toRows(value));
  }
  const update = (next: KvRow[]) => {
    setRows(next);
    const map = toMap(next);
    setSynced(map);
    onChange(map);
  };
  const keyCounts = new Map<string, number>();
  for (const r of rows) if (r.key.trim()) keyCounts.set(r.key.trim(), (keyCounts.get(r.key.trim()) ?? 0) + 1);
  return (
    <div className="kv-editor" role="group" aria-label={label}>
      <div className="kv-editor-head">
        <span className="field-label">{label}</span>
        {description && <span className="field-hint">{description}</span>}
      </div>
      {rows.length === 0 && <div className="kv-empty">{t("common.noEntries")}</div>}
      {valueOptions && (
        <datalist id={listId}>
          {valueOptions.map((o) => (
            <option key={o} value={o} />
          ))}
        </datalist>
      )}
      {rows.map((row, i) => {
        const keyErr = row.key.trim() ? (validateKey?.(row.key.trim()) ?? ((keyCounts.get(row.key.trim()) ?? 0) > 1 ? t("validation.duplicateKey") : null)) : null;
        return (
          <div className="kv-editor-row" key={i}>
            <input
              className="input mono"
              aria-label={`${label}: ${t("common.key")} ${i + 1}`}
              aria-invalid={keyErr ? true : undefined}
              placeholder={keyPlaceholder}
              value={row.key}
              spellCheck={false}
              onChange={(e) => update(rows.map((r, j) => (j === i ? { ...r, key: e.target.value } : r)))}
            />
            <span className="kv-eq">=</span>
            <input
              className="input mono"
              aria-label={`${label}: ${t("common.value")} ${i + 1}`}
              placeholder={valuePlaceholder}
              value={row.value}
              list={valueOptions ? listId : undefined}
              spellCheck={false}
              onChange={(e) => update(rows.map((r, j) => (j === i ? { ...r, value: e.target.value } : r)))}
            />
            <IconButton icon={Trash} size="sm" label={t("common.remove")} onClick={() => update(rows.filter((_, j) => j !== i))} />
            {keyErr && <div className="field-error kv-row-error">{keyErr}</div>}
          </div>
        );
      })}
      <div>
        <Button size="sm" variant="ghost" icon={Plus} onClick={() => setRows([...rows, { key: "", value: "" }])}>
          {t("common.add")}
        </Button>
      </div>
    </div>
  );
}

// ------------------------------------------------------- StringListEditor

export function StringListEditor({ label, value, onChange, placeholder, browse }: { label: string; value: string[]; onChange: (v: string[]) => void; placeholder?: string; browse?: "file" | "directory" }) {
  const t = useT();
  const toasts = useToasts();
  const pick = async (i: number | null) => {
    try {
      const p = browse === "directory" ? await pickDirectory({}) : await pickFile({});
      if (!p) return;
      if (i === null) onChange([...value, p]);
      else onChange(value.map((v, j) => (j === i ? p : v)));
    } catch (e) {
      toasts.error(e, t("error.dialog"));
    }
  };
  return (
    <div className="kv-editor" role="group" aria-label={label}>
      <span className="field-label">{label}</span>
      {value.length === 0 && <div className="kv-empty">{t("common.noEntries")}</div>}
      {value.map((v, i) => (
        <div className="list-editor-row" key={i}>
          <input className="input mono" aria-label={`${label} ${i + 1}`} value={v} placeholder={placeholder} spellCheck={false} onChange={(e) => onChange(value.map((x, j) => (j === i ? e.target.value : x)))} />
          {browse && <IconButton icon={FolderOpen} size="sm" label={t("common.browse")} onClick={() => void pick(i)} />}
          <IconButton icon={Trash} size="sm" label={t("common.remove")} onClick={() => onChange(value.filter((_, j) => j !== i))} />
        </div>
      ))}
      <div className="row gap-sm">
        <Button size="sm" variant="ghost" icon={Plus} onClick={() => onChange([...value, ""])}>
          {t("common.add")}
        </Button>
        {browse && (
          <Button size="sm" variant="ghost" icon={FolderOpen} onClick={() => void pick(null)}>
            {t("common.addBrowse")}
          </Button>
        )}
      </div>
    </div>
  );
}
