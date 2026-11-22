// DanyWhere: the stand-in for `src/i18n/index.ts` in component tests. That
// module finds the locale files with Vite's `import.meta.glob`, which bun
// doesn't have, and asks the backend for the language. This one reads the
// same files from disk, renames the product as the app does
// (`rebrandTranslation`) and sets up the same i18next instance with the same
// options, in English. So a test sees the real texts, and a placeholder the
// code doesn't fill ("Items: {{number}}") or a missing key shows as it would
// in the app.
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { rebrandTranslation } from "@/i18n/branding";
import { LANGUAGE_METADATA } from "@/i18n/languages";

export { getLanguageDirection, isRTLLanguage } from "@/lib/utils/rtl";

const LOCALES = fileURLToPath(new URL("../i18n/locales/", import.meta.url));

const resources: Record<string, { translation: Record<string, unknown> }> =
  {};
for (const code of readdirSync(LOCALES)) {
  const tree = JSON.parse(
    readFileSync(`${LOCALES}${code}/translation.json`, "utf8"),
  ) as Record<string, unknown>;
  resources[code] = { translation: rebrandTranslation(tree) };
}

/** The language every test starts in. */
export const TEST_LANGUAGE = "en";

// The options of `src/i18n/index.ts`, set up at once.
void i18n.use(initReactI18next).init({
  resources,
  lng: TEST_LANGUAGE,
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  react: { useSuspense: false },
  initAsync: false,
});

export const SUPPORTED_LANGUAGES = Object.keys(resources).map((code) => {
  const meta = LANGUAGE_METADATA[code];
  return {
    code,
    name: meta?.name ?? code,
    nativeName: meta?.nativeName ?? code,
    priority: meta?.priority,
  };
});

export type SupportedLanguageCode = string;

/** A simpler `getSupportedLanguage`: the code itself or its language. */
export const getSupportedLanguage = (
  langCode: string | null | undefined,
): SupportedLanguageCode | null => {
  if (!langCode) return null;
  const normalized = langCode.toLowerCase().replace(/_/g, "-");
  const exact = Object.keys(resources).find(
    (code) => code.toLowerCase() === normalized,
  );
  if (exact) return exact;
  const language = normalized.split("-")[0];
  return Object.keys(resources).find((code) => code === language) ?? null;
};

/** The app reads the language from the settings; a test keeps its own. */
export const syncLanguageFromSettings = async (): Promise<void> => {};

/** Switches the texts to `code` ("de", …) for the rest of the test. */
export const switchTestLanguage = async (code: string): Promise<void> => {
  await i18n.changeLanguage(code);
};

/** Back to English, after every test. */
export const resetTestLanguage = async (): Promise<void> => {
  if (i18n.language !== TEST_LANGUAGE) await i18n.changeLanguage(TEST_LANGUAGE);
};

export default i18n;
