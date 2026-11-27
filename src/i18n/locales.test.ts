// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/i18n/locales.test.ts
import assert from "node:assert";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { LANGUAGE_METADATA } from "./languages";

type Tree = { [key: string]: unknown };

const localesDir = fileURLToPath(new URL("locales", import.meta.url));
const load = (lang: string): Tree =>
  JSON.parse(readFileSync(join(localesDir, lang, "translation.json"), "utf8"));

const leaves = (node: Tree, prefix = ""): [string, string][] =>
  Object.entries(node).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return value && typeof value === "object"
      ? leaves(value as Tree, path)
      : [[path, String(value)] as [string, string]];
  });

const english = new Map(leaves(load("en")));
const locales = readdirSync(localesDir).filter((name) =>
  existsSync(join(localesDir, name, "translation.json")),
);

// Every locale folder has its metadata, and every metadata entry its folder.
for (const lang of locales) {
  assert.ok(LANGUAGE_METADATA[lang], `${lang}: missing in languages.ts`);
}
for (const lang of Object.keys(LANGUAGE_METADATA)) {
  assert.ok(locales.includes(lang), `${lang}: no locale folder`);
}

// Slovenian is offered, under its own name, with every text of the app.
assert.deepStrictEqual(
  {
    name: LANGUAGE_METADATA.sl?.name,
    nativeName: LANGUAGE_METADATA.sl?.nativeName,
  },
  { name: "Slovenian", nativeName: "Slovenščina" },
);
assert.ok(locales.includes("sl"), "sl: no locale folder");
const slovenian = new Map(leaves(load("sl")));
const missing = [...english.keys()].filter((key) => !slovenian.has(key));
const extra = [...slovenian.keys()].filter((key) => !english.has(key));
assert.deepStrictEqual(
  { missing: missing.length, extra: extra.length },
  { missing: 0, extra: 0 },
  `sl: ${missing.length} missing, ${extra.length} extra keys`,
);

// Each translated text keeps the placeholders of its English source.
const placeholders = (text: string) =>
  (text.match(/\{\{[^}]+\}\}/g) ?? []).sort();
for (const lang of ["sl"]) {
  for (const [key, value] of leaves(load(lang))) {
    const source = english.get(key);
    if (source === undefined) continue;
    assert.deepStrictEqual(
      placeholders(value),
      placeholders(source),
      `${lang}: ${key} changes the placeholders: ${value}`,
    );
  }
}

console.log(`locales: ${locales.length} languages, ${english.size} keys`);
