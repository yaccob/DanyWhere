// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/whats-new/releaseNoteFiles.test.ts
import assert from "node:assert";
import { readdirSync } from "node:fs";
import { LANGUAGE_METADATA } from "../../i18n/languages";
import {
  parseReleaseNotePath,
  pickReleaseNoteMarkdown,
} from "./releaseNoteFiles";

let failures = 0;
let checks = 0;
const check = (name: string, actual: unknown, expected: unknown) => {
  checks += 1;
  try {
    assert.deepStrictEqual(actual, expected);
  } catch {
    failures += 1;
    console.error(
      `FAIL ${name}: got ${JSON.stringify(actual)}, want ${JSON.stringify(expected)}`,
    );
  }
};

const dir = "../../content/release-notes";

// Handy's release notes (named by Handy's version) are never shown as
// DanyWhere news, whatever DanyWhere's own version is.
check("Handy note", parseReleaseNotePath(`${dir}/0.9.7.md`), null);
check("Handy-like name", parseReleaseNotePath(`${dir}/0.1.0.md`), null);
check("README", parseReleaseNotePath(`${dir}/README.md`), null);

// DanyWhere's own notes are named danywhere-<version>.md (English, the
// fallback for every language) ...
check("English note", parseReleaseNotePath(`${dir}/danywhere-0.1.1.md`), {
  version: "0.1.1",
  language: null,
});
check("major version", parseReleaseNotePath(`${dir}/danywhere-1.0.0.md`), {
  version: "1.0.0",
  language: null,
});
// ... and danywhere-<version>.<language>.md for a translation.
check("German note", parseReleaseNotePath(`${dir}/danywhere-0.1.4.de.md`), {
  version: "0.1.4",
  language: "de",
});
check(
  "regional language",
  parseReleaseNotePath(`${dir}/danywhere-0.1.4.zh-TW.md`),
  { version: "0.1.4", language: "zh-TW" },
);
check("not a version", parseReleaseNotePath(`${dir}/danywhere-draft.md`), null);

// The note in the app's language, else its base language, else English.
const notes = { "": "english", de: "deutsch", zh: "chinese" };
check("exact language", pickReleaseNoteMarkdown(notes, "de"), "deutsch");
check("language case", pickReleaseNoteMarkdown(notes, "DE"), "deutsch");
check("regional variant", pickReleaseNoteMarkdown(notes, "de-AT"), "deutsch");
check("base of zh-TW", pickReleaseNoteMarkdown(notes, "zh-TW"), "chinese");
check("untranslated", pickReleaseNoteMarkdown(notes, "fr"), "english");
check("English itself", pickReleaseNoteMarkdown(notes, "en"), "english");
check("no language", pickReleaseNoteMarkdown(notes, ""), "english");
check(
  "regional file wins",
  pickReleaseNoteMarkdown({ "": "english", "zh-TW": "traditional" }, "zh-TW"),
  "traditional",
);
check(
  "no English file",
  pickReleaseNoteMarkdown({ de: "deutsch" }, "fr"),
  null,
);

// The bundled notes themselves: every translation has its English file and
// names a language the app offers.
const bundled = readdirSync(new URL(`${dir}/`, import.meta.url))
  .map((name) => parseReleaseNotePath(`${dir}/${name}`))
  .filter((file) => file !== null);
const englishVersions = new Set(
  bundled.filter((f) => f.language === null).map((f) => f.version),
);
for (const file of bundled.filter((f) => f.language !== null)) {
  check(
    `danywhere-${file.version}.${file.language}.md has an English file`,
    englishVersions.has(file.version),
    true,
  );
  check(
    `danywhere-${file.version}.${file.language}.md names an app language`,
    file.language! in LANGUAGE_METADATA,
    true,
  );
}

if (failures > 0) {
  console.error(`release note files: ${failures} of ${checks} checks failed`);
  process.exit(1);
}
console.log(`release note files: all ${checks} checks passed`);
