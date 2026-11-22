// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/history/versionsWording.test.ts
// DanyWhere: the words History names its versions with
// (`settings.history.versions`). The version the user sees is the current
// one, and one an action makes a new one; "top" only where nothing else says
// it ("Move to the Top": everything goes on top, and the top is the entry's
// result). No word for "newest": it meant the top, while "new" meant a
// version still to come.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const LOCALES = join(
  fileURLToPath(import.meta.url),
  "../../../../i18n/locales",
);

// Words for "newest" (or "latest") in each locale, which the top must never
// be called; the same table as the sidecar's check of the translations.
const NEWEST: Record<string, RegExp> = {
  en: /newest|latest/iu,
  de: /neueste|jüngste/iu,
  ar: /أحدث/u,
  bg: /най-нов/iu,
  ca: /més recent|més nov/iu,
  cs: /nejnověj/iu,
  da: /nyeste|seneste/iu,
  es: /más reciente|más nuev/iu,
  fr: /plus récent|plus nouv/iu,
  he: /חדש\p{L}* ביותר|האחרונה/u,
  hi: /सबसे न[याएई]|नवीनतम/u,
  id: /terbaru/iu,
  it: /più recent|più nuov/iu,
  ja: /最新/u,
  ko: /최신|가장 새/u,
  ne: /सबैभन्दा नयाँ|नवीनतम/u,
  nl: /nieuwste|recentste/iu,
  pl: /najnowsz/iu,
  pt: /mais recente|mais nov/iu,
  ru: /новейш|сам\p{L}* нов/iu,
  sv: /nyaste|senaste/iu,
  tr: /(?<!\p{L})en yeni|(?<!\p{L})en son(?!\p{L})/iu,
  uk: /найнов/iu,
  vi: /mới nhất/iu,
  zh: /最新/u,
  "zh-TW": /最新/u,
};

// How English and German may name a version: as the current, the previous
// or a new one, by count ("3 versions"), or as the versions above the
// current one (or above the transcription just named).
const NAMED = {
  en: {
    allowed:
      /\b(?:current|previous|new) versions?\b|\bversions? above (?:the current one|it)\b|\{\{count\}\} versions?\b/giu,
    noun: /\bversions?\b/iu,
    vague: /\b(?:this|that) (?:version|one)\b/iu,
    top: /\btop\b/iu,
  },
  de: {
    allowed:
      /\b(?:aktuelle[nrs]?|vorige[nrs]?|neue[nrs]?) Fassung(?:en)?\b|\bFassung(?:en)? (?:über der aktuellen|darüber)(?![\p{L}])|\{\{count\}\} Fassung(?:en)?\b/giu,
    noun: /\bFassung/iu,
    vague: /\b(?:diese[mnrs]?|jene[mnrs]?)\b/iu,
    top: /\boberste/iu,
  },
};

// The one string that has to say "top": moving the current version there
// makes it the entry's result.
const TOP_ALLOWED = new Set(["moveToTop"]);

type Strings = [key: string, text: string][];

const versionStrings = (locale: string): Strings => {
  const file = join(LOCALES, locale, "translation.json");
  const json = JSON.parse(readFileSync(file, "utf8"));
  const node = json?.settings?.history?.versions;
  assert.ok(node, `${locale}: no settings.history.versions`);
  const found: Strings = [];
  const walk = (value: unknown, path: string) => {
    if (typeof value === "string") found.push([path, value]);
    else if (value && typeof value === "object") {
      for (const [key, child] of Object.entries(value)) {
        walk(child, path ? `${path}.${key}` : key);
      }
    }
  };
  walk(node, "");
  return found;
};

let checks = 0;
let failures = 0;
const check = (name: string, run: () => void) => {
  checks += 1;
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

const offending = (strings: Strings, bad: (text: string) => boolean) =>
  strings
    .filter(([, text]) => bad(text))
    .map(([key, text]) => `${key}: ${text}`);

const locales = readdirSync(LOCALES, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort();

check("every locale has its word for newest in the table", () => {
  assert.deepEqual(locales, Object.keys(NEWEST).sort());
});

check("no English string says newest", () => {
  assert.deepEqual(
    offending(versionStrings("en"), (text) => /newest/iu.test(text)),
    [],
  );
});

check("no German string says neueste", () => {
  assert.deepEqual(
    offending(versionStrings("de"), (text) => /neueste/iu.test(text)),
    [],
  );
});

for (const locale of locales) {
  const newest = NEWEST[locale];
  if (!newest) continue;
  check(`${locale}: no word for newest or latest`, () => {
    assert.deepEqual(
      offending(versionStrings(locale), (text) => newest.test(text)),
      [],
    );
  });
}

for (const [locale, rules] of Object.entries(NAMED)) {
  const strings = versionStrings(locale);
  check(`${locale}: every version named is current, previous or new`, () => {
    assert.deepEqual(
      offending(strings, (text) =>
        rules.noun.test(text.replace(rules.allowed, "")),
      ),
      [],
    );
  });
  check(`${locale}: no version is pointed at with "this" or "that"`, () => {
    assert.deepEqual(
      offending(strings, (text) => rules.vague.test(text)),
      [],
    );
  });
  check(`${locale}: "top" only where it can't be avoided`, () => {
    assert.deepEqual(
      strings
        .filter(([key, text]) => !TOP_ALLOWED.has(key) && rules.top.test(text))
        .map(([key, text]) => `${key}: ${text}`),
      [],
    );
  });
}

if (failures > 0) {
  console.error(`versionsWording: ${failures} of ${checks} check(s) failed`);
  process.exit(1);
}
console.log(`versionsWording: all ${checks} checks passed`);
