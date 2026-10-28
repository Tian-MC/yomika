// Tiny typed i18n for the UI language (NOT the translation target language).
import { createContext, useContext, useMemo, type ReactNode } from "react";
import { en, type MessageKey } from "./en";
import { it } from "./it";

export type { MessageKey };
export type UiLang = "en" | "it";
export type Vars = Record<string, string | number>;
export type TFunction = (key: MessageKey, vars?: Vars) => string;

const dictionaries: Record<UiLang, Record<MessageKey, string>> = { en, it };

export const UI_LANGUAGES: { code: UiLang; label: string }[] = [
  { code: "en", label: "English" },
  { code: "it", label: "Italiano" },
];

/** Map an AppSettings.uiLanguage (or the system locale when null) to a supported UI language. */
export function resolveUiLanguage(setting: string | null | undefined, systemLocale?: string): UiLang {
  const candidates = [setting, systemLocale ?? (typeof navigator !== "undefined" ? navigator.language : undefined)];
  for (const c of candidates) {
    if (!c) continue;
    const base = c.toLowerCase().split(/[-_]/)[0];
    if (base === "it") return "it";
    if (base === "en") return "en";
  }
  return "en";
}

export function translate(lang: UiLang, key: MessageKey, vars?: Vars): string {
  const template = dictionaries[lang][key] ?? en[key] ?? key;
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (m, name: string) => (name in vars ? String(vars[name]) : m));
}

interface I18nValue {
  lang: UiLang;
  t: TFunction;
}

const I18nContext = createContext<I18nValue>({ lang: "en", t: (k, v) => translate("en", k, v) });

export function I18nProvider({ lang, children }: { lang: UiLang; children: ReactNode }) {
  const value = useMemo<I18nValue>(() => ({ lang, t: (k, v) => translate(lang, k, v) }), [lang]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  return useContext(I18nContext);
}

export function useT(): TFunction {
  return useContext(I18nContext).t;
}
