// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/i18n/plurals.test.ts
import assert from "node:assert";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import i18next from "i18next";

type Tree = { [key: string]: unknown };

const localesDir = fileURLToPath(new URL("locales", import.meta.url));
const load = (lang: string): Tree =>
  JSON.parse(readFileSync(join(localesDir, lang, "translation.json"), "utf8"));
const locales = readdirSync(localesDir).filter((name) =>
  existsSync(join(localesDir, name, "translation.json")),
);

const keys = (node: Tree, prefix = ""): string[] =>
  Object.entries(node).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return value && typeof value === "object"
      ? keys(value as Tree, path)
      : [path];
  });
const english = new Set(keys(load("en")));

// A counted text has all six plural forms in English, so every locale can
// give the forms its language needs; i18next falls back to English for a
// form the locale lacks ("2 modela" in Slovenian, "3 модели" in Russian).
const FORMS = ["zero", "one", "two", "few", "many", "other"];
const counted = new Set(
  [...english]
    .map((key) => key.match(/^(.+)_(zero|one|two|few|many|other)$/)?.[1])
    .filter((base): base is string => base !== undefined),
);
// Texts that pick the form themselves (`_one` or `_other` by the count).
const PICKED_BY_CODE = new Set([
  "secureInput.blockedWithCulprit",
  "secureInput.blockedNoCulprit",
]);
assert.deepStrictEqual(
  [...counted].filter(
    (base) =>
      !PICKED_BY_CODE.has(base) &&
      FORMS.some((form) => !english.has(`${base}_${form}`)),
  ),
  [],
  "counted texts without all six plural forms",
);

// And i18next finds a counted text in the locale itself for every count.
// Without a fallback language it answers with the key itself when the
// locale lacks the form it needs.
const i18n = i18next.createInstance();
await i18n.init({
  resources: Object.fromEntries(
    locales.map((lang) => [lang, { translation: load(lang) }]),
  ),
  fallbackLng: false,
  lng: "en",
});
const leaking: string[] = [];
for (const lang of locales.filter((l) => l !== "en")) {
  for (const base of counted) {
    if (PICKED_BY_CODE.has(base)) continue;
    for (const count of [0, 1, 2, 3, 4, 5, 11, 21, 22, 25, 101, 102, 103]) {
      const text = i18n.t(base, { lng: lang, count, languages: "X" });
      if (text === base) leaking.push(`${lang} ${base} ${count}`);
    }
  }
}
assert.deepStrictEqual(leaking, [], "counted texts that fall back to English");

console.log(
  `plurals: ${counted.size} counted texts in ${locales.length} languages`,
);
