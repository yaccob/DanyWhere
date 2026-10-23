// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/i18n/branding.test.ts
import assert from "node:assert";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  rebrandText,
  rebrandTranslation,
  UPSTREAM_REFERENCE_KEYS,
} from "./branding";
import { BRAND_NAME, UPSTREAM_NAME } from "../lib/constants/branding";

// Product name as a word, possessive, genitive, and next to non-ASCII text.
assert.strictEqual(rebrandText("Handy needs"), `${BRAND_NAME} needs`);
assert.strictEqual(rebrandText("Handy's data"), `${BRAND_NAME}'s data`);
assert.strictEqual(rebrandText("Handys ikon"), `${BRAND_NAME}s ikon`);
assert.strictEqual(rebrandText("Handy가 필요"), `${BRAND_NAME}가 필요`);
assert.strictEqual(rebrandText("«Handy»"), `«${BRAND_NAME}»`);

// Identifiers stay untouched.
for (const text of [
  "HandyKeys",
  "(HANDY_DISABLE_UPDATER)",
  "com.pais.handy",
  "handy_keys",
]) {
  assert.strictEqual(rebrandText(text), text);
}

// Nested keys are rewritten, exception keys and non-strings are kept.
const tree = rebrandTranslation({
  a: { b: "Open Handy", n: 3 },
  settings: {
    about: {
      supportDevelopment: { description: "Help us continue building Handy" },
    },
  },
});
assert.deepStrictEqual(tree, {
  a: { b: `Open ${BRAND_NAME}`, n: 3 },
  settings: {
    about: {
      supportDevelopment: { description: "Help us continue building Handy" },
    },
  },
});

// Against the real locale files: every remaining "Handy" is in an exception
// key, and every exception key still names the upstream project.
const flatten = (
  node: Record<string, unknown>,
  prefix = "",
): [string, unknown][] =>
  Object.entries(node).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return value && typeof value === "object"
      ? flatten(value as Record<string, unknown>, path)
      : [[path, value] as [string, unknown]];
  });

const localesDir = fileURLToPath(new URL("locales", import.meta.url));
for (const lang of readdirSync(localesDir)) {
  const source = JSON.parse(
    readFileSync(join(localesDir, lang, "translation.json"), "utf8"),
  );
  const branded = new Map(flatten(rebrandTranslation(source)));
  for (const [key, value] of branded) {
    if (typeof value !== "string" || UPSTREAM_REFERENCE_KEYS.has(key)) {
      continue;
    }
    assert.ok(
      !/\bHandys?\b/.test(value),
      `${lang}: ${key} still names Handy: ${value}`,
    );
  }
  for (const key of UPSTREAM_REFERENCE_KEYS) {
    const value = branded.get(key);
    assert.ok(
      typeof value === "string" && value.includes(UPSTREAM_NAME),
      `${lang}: exception ${key} no longer names ${UPSTREAM_NAME}: ${value}`,
    );
  }
}

console.log("branding: all checks passed");
