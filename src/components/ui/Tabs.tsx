import type { LucideIcon } from "lucide-react";
import { useId, type KeyboardEvent, type ReactNode } from "react";

export interface TabDef<V extends string> {
  value: V;
  label: ReactNode;
  icon?: LucideIcon;
  badge?: ReactNode;
}

interface TabsProps<V extends string> {
  tabs: TabDef<V>[];
  value: V;
  onChange: (v: V) => void;
  label: string;
  idBase?: string;
}

/** Accessible tab list (arrow keys / Home / End). Panels use `tabPanelProps`. */
export function Tabs<V extends string>({ tabs, value, onChange, label, idBase }: TabsProps<V>) {
  const generated = useId();
  const base = idBase ?? generated;
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const idx = tabs.findIndex((t) => t.value === value);
    let next: number;
    if (e.key === "ArrowRight") next = (idx + 1) % tabs.length;
    else if (e.key === "ArrowLeft") next = (idx - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = tabs.length - 1;
    else return;
    e.preventDefault();
    const tab = tabs[next];
    if (!tab) return;
    onChange(tab.value);
    document.getElementById(`${base}-tab-${tab.value}`)?.focus();
  };
  return (
    <div className="tabs" role="tablist" aria-label={label} onKeyDown={onKey}>
      {tabs.map(({ value: v, label: l, icon: Icon, badge }) => (
        <button
          key={v}
          id={`${base}-tab-${v}`}
          type="button"
          role="tab"
          aria-selected={v === value}
          aria-controls={`${base}-panel-${v}`}
          tabIndex={v === value ? 0 : -1}
          className={v === value ? "tab is-active" : "tab"}
          onClick={() => onChange(v)}
        >
          {Icon && <Icon size={15} aria-hidden />}
          <span>{l}</span>
          {badge}
        </button>
      ))}
    </div>
  );
}

export function tabPanelProps(base: string, value: string) {
  return { id: `${base}-panel-${value}`, role: "tabpanel" as const, "aria-labelledby": `${base}-tab-${value}`, tabIndex: 0 };
}
