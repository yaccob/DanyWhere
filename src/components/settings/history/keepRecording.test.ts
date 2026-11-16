// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/history/keepRecording.test.ts
// DanyWhere: the History star keeps an entry's recording beyond the storage
// for recordings, and says so (user, 2026-11-16: "Aufnahme dauerhaft
// behalten", with the explanation below it). The Debug page's storage limit
// names those recordings with the same words.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { keepRecordingTooltip } from "./keepRecording";

const LOCALES = join(
  fileURLToPath(import.meta.url),
  "../../../../i18n/locales",
);

const lookup =
  (json: Record<string, unknown>) =>
  (key: string): string => {
    const value = key
      .split(".")
      .reduce<unknown>(
        (node, part) =>
          node && typeof node === "object"
            ? (node as Record<string, unknown>)[part]
            : undefined,
        json,
      );
    return typeof value === "string" ? value : key;
  };

const load = (locale: string) =>
  JSON.parse(
    readFileSync(join(LOCALES, locale, "translation.json"), "utf8"),
  ) as Record<string, unknown>;

let checks = 0;
const failures: string[] = [];
const check = (fn: () => void) => {
  checks += 1;
  try {
    fn();
  } catch (error) {
    failures.push(error instanceof Error ? error.message : String(error));
  }
};

// English and German, word for word.
const en = lookup(load("en"));
const de = lookup(load("de"));
check(() =>
  assert.equal(
    keepRecordingTooltip(false, de),
    "Aufnahme dauerhaft behalten\nWird nie gelöscht, auch wenn der Speicherplatz für Aufnahmen voll ist.",
  ),
);
check(() =>
  assert.equal(
    keepRecordingTooltip(true, de),
    "Aufnahme wird dauerhaft behalten\nEin Klick hebt das auf: Dann kann sie gelöscht werden, wenn der Speicherplatz für Aufnahmen voll ist.",
  ),
);
check(() =>
  assert.equal(
    keepRecordingTooltip(false, en),
    "Keep recording permanently\nNever deleted, even when the storage for recordings is full.",
  ),
);
check(() =>
  assert.equal(
    keepRecordingTooltip(true, en),
    "Recording kept permanently\nA click undoes this: it can then be deleted when the storage for recordings is full.",
  ),
);
check(() =>
  assert.equal(
    de("settings.debug.recordingStorage.description"),
    "Brauchen alle Aufnahmen zusammen mehr Platz, werden die ältesten gelöscht. Ihre Transkripte bleiben im Verlauf. Dauerhaft behaltene Aufnahmen (★) und transkribierte Dateien werden nie automatisch gelöscht.",
  ),
);
check(() =>
  assert.equal(
    de("settings.debug.recordingStorage.protectedOverLimit"),
    "Dauerhaft behaltene Aufnahmen und transkribierte Dateien allein belegen {{size}}, mehr als dieses Limit. Von den übrigen Aufnahmen bleibt nur die neueste erhalten.",
  ),
);
check(() =>
  assert.equal(
    en("settings.debug.recordingStorage.description"),
    "When all recordings together take more space than this, the oldest are deleted. Their transcripts stay in History. Recordings kept permanently (★) and transcribed files are never deleted automatically.",
  ),
);
check(() =>
  assert.equal(
    en("settings.debug.recordingStorage.protectedOverLimit"),
    "Recordings kept permanently and transcribed files alone take {{size}}, more than this limit. Of the other recordings, only the newest is kept.",
  ),
);

// Every locale: two lines each, neither of them Handy's old tooltip, and the
// Debug page points at the star.
const locales = readdirSync(LOCALES);
assert.equal(locales.length, 26, "26 locales");
for (const locale of locales) {
  const t = lookup(load(locale));
  for (const saved of [false, true]) {
    check(() => {
      const lines = keepRecordingTooltip(saved, t).split("\n");
      assert.equal(lines.length, 2, `${locale} saved=${saved}: two lines`);
      for (const line of lines) {
        assert.ok(
          line.trim() && !line.startsWith("settings."),
          `${locale} saved=${saved}: a translated line, got "${line}"`,
        );
        assert.notEqual(line, t("settings.history.save"), locale);
        assert.notEqual(line, t("settings.history.unsave"), locale);
      }
    });
  }
  check(() =>
    assert.ok(
      t("settings.debug.recordingStorage.description").includes("★"),
      `${locale}: the storage limit names the star`,
    ),
  );
}

if (failures.length > 0) {
  for (const failure of failures) console.error(failure.split("\n")[0]);
  console.error(`keepRecording: ${failures.length} of ${checks} checks failed`);
  process.exit(1);
}
console.log(`keepRecording: ${checks} checks passed`);
