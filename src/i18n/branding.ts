import { BRAND_NAME } from "../lib/constants/branding";

// The locale files (maintained upstream, edited there constantly) name the
// app "Handy". Instead of rewriting ~25 translation files and fighting merge
// conflicts on every upstream sync, the product name is swapped when the
// resources are loaded.
//
// Matches "Handy" as a whole word, plus the Danish genitive "Handys". Leaves
// identifiers alone ("HandyKeys", "HANDY_DISABLE_UPDATER", "com.pais.handy").
const PRODUCT_NAME_PATTERN = /\bHandy(?=s?\b)/g;

// Keys whose text refers to the upstream project rather than to the running
// app; they keep the name "Handy". Keep this list small and explicit.
export const UPSTREAM_REFERENCE_KEYS: ReadonlySet<string> = new Set([
  // "Help us continue building Handy": the Donate button funds upstream.
  "settings.about.supportDevelopment.description",
]);

export const rebrandText = (text: string): string =>
  text.replace(PRODUCT_NAME_PATTERN, BRAND_NAME);

type TranslationTree = { [key: string]: unknown };

/** Returns a copy of a translation tree with the product name replaced. */
export const rebrandTranslation = (
  tree: TranslationTree,
  prefix = "",
): TranslationTree => {
  const result: TranslationTree = {};
  for (const [key, value] of Object.entries(tree)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof value === "string") {
      result[key] = UPSTREAM_REFERENCE_KEYS.has(path)
        ? value
        : rebrandText(value);
    } else if (value && typeof value === "object" && !Array.isArray(value)) {
      result[key] = rebrandTranslation(value as TranslationTree, path);
    } else {
      result[key] = value;
    }
  }
  return result;
};
