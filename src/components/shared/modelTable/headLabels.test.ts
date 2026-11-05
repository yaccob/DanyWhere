import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { COLUMN_PX, headCells, layoutOf } from "./headLabels";

// The heads are 11px semibold capitals with 0.02em tracking: about 7.7px per
// Latin, Cyrillic, Greek, Arabic, Hebrew or Devanagari character, 11px per
// CJK or Hangul character. A text wider than its column runs into its
// neighbours (seen 2026-11-05: German "Geschwindigkeit" over "EN" and
// "Größe").
const WIDE =
  /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;
const widthPx = (text: string) =>
  [...text.toUpperCase()].reduce((w, ch) => w + (WIDE.test(ch) ? 11 : 7.7), 0);

const localesDir = fileURLToPath(
  new URL("../../../i18n/locales", import.meta.url),
);
const failures: string[] = [];

for (const locale of readdirSync(localesDir)) {
  const strings = JSON.parse(
    readFileSync(join(localesDir, locale, "translation.json"), "utf8"),
  );
  const t = (key: string) =>
    key.split(".").reduce((o, k) => o?.[k], strings) ?? key;
  for (const langs of [
    ["de", "en"],
    ["de", "en", "fr", "es"],
    ["de", "en", "fr", "es", "it", "pt"],
  ]) {
    const layout = layoutOf(langs);
    for (const cell of headCells(langs, t, (code) => code)) {
      if (cell.text === null) continue;
      const width = COLUMN_PX[layout][cell.column];
      assert.ok(width, `${layout} has a width for ${cell.column}`);
      if (widthPx(cell.text) > width) {
        failures.push(
          `${locale} ${layout} ${cell.column}: "${cell.text}" ~${Math.round(widthPx(cell.text))}px > ${width}px`,
        );
      }
    }
  }
}

assert.deepEqual(
  failures,
  [],
  `column heads wider than their column:\n${failures.join("\n")}`,
);
