import en from "./en.json";
import vi from "./vi.json";

type Messages = typeof en;

const locales: Record<string, Messages> = { en, vi };

let currentLang = $state("en");

/** Get a nested translation value using dot notation: t("session.idle") */
export function t(key: string): string {
  const messages = locales[currentLang] ?? locales.en;
  const keys = key.split(".");
  let result: unknown = messages;

  for (const k of keys) {
    if (result && typeof result === "object" && k in result) {
      result = (result as Record<string, unknown>)[k];
    } else {
      return key;
    }
  }

  return typeof result === "string" ? result : key;
}

/** Set the current UI language. */
export function setLocale(lang: string) {
  if (lang in locales) {
    currentLang = lang;
  }
}

/** Get the current locale. */
export function getLocale(): string {
  return currentLang;
}

/** Get all available locale codes. */
export function getAvailableLocales(): string[] {
  return Object.keys(locales);
}
