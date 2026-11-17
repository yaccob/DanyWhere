// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/overlay/readyNotice.test.ts
// DanyWhere: the overlay's pill after a dictation that wasn't pasted. On the
// clipboard it names the paste keys; without Accessibility the text waits for
// the "Insert Last Dictation" service instead, and the pill says it is ready
// (with the service's keys, or the Services menu without them), or not ready
// yet when the service was called too early. From 0.1.4 to 0.2.1 every one
// of these pills said "On the clipboard – ⌘V to paste".
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { ReadyNotice } from "./ReadyNotice";
import type { ReadyNoticeState } from "./ReadyNotice";

const LOCALES = join(fileURLToPath(import.meta.url), "../../i18n/locales");

type Translate = (key: string, options?: { keys: string }) => string;

/** A locale's translate function, with i18next's `{{name}}` interpolation. */
const translate = (locale: string): Translate => {
  const json = JSON.parse(
    readFileSync(join(LOCALES, locale, "translation.json"), "utf8"),
  ) as Record<string, unknown>;
  return (key, options) => {
    const value = key
      .split(".")
      .reduce<unknown>(
        (node, part) =>
          node && typeof node === "object"
            ? (node as Record<string, unknown>)[part]
            : undefined,
        json,
      );
    if (typeof value !== "string") return key;
    return value.replace(/\{\{\s*(\w+)\s*\}\}/g, (whole, name: string) =>
      options && name in options
        ? options[name as keyof typeof options]
        : whole,
    );
  };
};

const unescape = (html: string) =>
  html
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#x27;/g, "'")
    .replace(/&amp;/g, "&");

/** The text the pill shows for a state, as React renders it. */
const pill = (
  state: ReadyNoticeState,
  pasteKeys: string,
  insertKeys: string | null,
  t: Translate,
) => {
  const html = renderToStaticMarkup(
    React.createElement(ReadyNotice, { state, pasteKeys, insertKeys, t }),
  );
  const match = html.match(/<span class="swork-label">([\s\S]*?)<\/span>/);
  return match ? unescape(match[1]) : `(no label in ${html})`;
};

const shows = (actual: string, expected: string) =>
  assert.equal(actual, expected, `shows "${actual}", not "${expected}"`);

// Every check runs, so a run reports how many fail, not just the first.
let checks = 0;
const failures: string[] = [];
const check = (name: string, body: () => void) => {
  checks += 1;
  try {
    body();
  } catch (error) {
    failures.push(`${name}: ${(error as Error).message.split("\n")[0]}`);
  }
};

// English and German, word for word.
const wording: Record<string, Record<string, string>> = {
  en: {
    clipboardMac: "On the clipboard – ⌘V to paste",
    clipboardOther: "On the clipboard – Ctrl+V to paste",
    ready: "Ready – ⌃⇧V to insert",
    readyNoShortcut: "Ready – insert it from the Services menu",
    notReady: "Not ready yet",
  },
  de: {
    clipboardMac: "In der Zwischenablage – ⌘V zum Einfügen",
    clipboardOther: "In der Zwischenablage – Ctrl+V zum Einfügen",
    ready: "Bereit – ⌃⇧V zum Einfügen",
    readyNoShortcut: "Bereit – über das Dienste-Menü einfügen",
    notReady: "Noch nicht fertig",
  },
};
for (const [locale, text] of Object.entries(wording)) {
  const t = translate(locale);
  const cases: Array<[string, () => string, string]> = [
    [
      "clipboard, macOS",
      () => pill("clipboard", "⌘V", null, t),
      "clipboardMac",
    ],
    [
      "clipboard, other platforms, whatever the service's keys",
      () => pill("clipboard", "Ctrl+V", "⌃⇧V", t),
      "clipboardOther",
    ],
    ["ready", () => pill("ready", "⌘V", "⌃⇧V", t), "ready"],
    [
      "ready, the service has no shortcut",
      () => pill("ready", "⌘V", null, t),
      "readyNoShortcut",
    ],
    ["not ready", () => pill("not_ready", "⌘V", "⌃⇧V", t), "notReady"],
    [
      "not ready, the service has no shortcut",
      () => pill("not_ready", "⌘V", null, t),
      "notReady",
    ],
  ];
  for (const [name, shown, expected] of cases) {
    check(`${locale} ${name}`, () => shows(shown(), text[expected]));
  }
}

// Every locale: each pill shows its own text, and only the clipboard pill
// talks about the clipboard.
const locales = readdirSync(LOCALES);
assert.equal(locales.length, 26, "26 locales");
for (const locale of locales) {
  const t = translate(locale);
  const clipboard = t("overlay.clipboard", { keys: "⌘V" });
  const own: Array<[string, () => string, string]> = [
    ["clipboard", () => pill("clipboard", "⌘V", "⌃⇧V", t), clipboard],
    [
      "ready",
      () => pill("ready", "⌘V", "⌃⇧V", t),
      t("overlay.ready", { keys: "⌃⇧V" }),
    ],
    [
      "ready without shortcut",
      () => pill("ready", "⌘V", null, t),
      t("overlay.readyNoShortcut"),
    ],
    [
      "not ready",
      () => pill("not_ready", "⌘V", "⌃⇧V", t),
      t("overlay.notReady"),
    ],
  ];
  for (const [name, shown, expected] of own) {
    check(`${locale} ${name}`, () => {
      assert.ok(!expected.startsWith("overlay."), "a translated text");
      if (name !== "clipboard") assert.notEqual(expected, clipboard);
      shows(shown(), expected);
    });
  }
}

// The overlay hands its state and the service's keys to this pill.
check("RecordingOverlay passes state and keys", () => {
  const source = readFileSync(
    new URL("./RecordingOverlay.tsx", import.meta.url),
    "utf8",
  );
  const element = source.match(/<ReadyNotice\b[\s\S]*?\/>/)?.[0] ?? "";
  assert.match(element, /\bstate=\{state\}/);
  assert.match(element, /\binsertKeys=\{insertKeys\}/);
});

if (failures.length > 0) {
  for (const failure of failures) console.error(failure);
  console.error(`readyNotice: ${failures.length} of ${checks} checks failed`);
  process.exit(1);
}
console.log(`readyNotice: ${checks} checks passed`);
