import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { CARD_LINES_COLUMNS } from "./modelCardGrid";

// The labels are 13px regular text: about 6.5px per Latin, Cyrillic, Greek,
// Arabic, Hebrew or Devanagari character, 13px per CJK or Hangul character.
// A label wider than its column runs into the grades or speed bars beside it
// (seen 2026-11-06: German "Geschwindigkeit" over the speed bars).
const WIDE =
  /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;
const widthPx = (text: string) =>
  [...text].reduce((w, ch) => w + (WIDE.test(ch) ? 13 : 6.5), 0);

const labelTrack = /^grid-cols-\[([^_\]]+)_/.exec(CARD_LINES_COLUMNS)?.[1];
assert.ok(labelTrack, `a label column in ${CARD_LINES_COLUMNS}`);
const fixedPx = /^(\d+)px$/.exec(labelTrack)?.[1];

// A column sized to its content fits every label by construction; a fixed
// one must be wide enough for the longest label of every locale.
const failures: string[] = [];
if (fixedPx) {
  const width = Number(fixedPx);
  const localesDir = fileURLToPath(
    new URL("../../../i18n/locales", import.meta.url),
  );
  for (const locale of readdirSync(localesDir)) {
    const strings = JSON.parse(
      readFileSync(join(localesDir, locale, "translation.json"), "utf8"),
    );
    for (const key of ["accuracy", "speed"]) {
      const label: string = strings.modelTable?.[key] ?? key;
      if (widthPx(label) > width) {
        failures.push(
          `${locale} ${key}: "${label}" ~${Math.round(widthPx(label))}px > ${width}px`,
        );
      }
    }
  }
}

assert.deepEqual(
  failures,
  [],
  `model card labels wider than their column (${failures.length}):\n${failures.join("\n")}`,
);
