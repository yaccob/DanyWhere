// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/pasteMethodDescription.test.ts
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { pasteMethodDescriptionKey } from "./pasteMethodDescription";

const english = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL("../i18n/locales/en/translation.json", import.meta.url),
    ),
    "utf8",
  ),
);

/** The English text under `key`, or `undefined`. */
const text = (key: string): unknown =>
  key
    .split(".")
    .reduce<unknown>(
      (node, part) =>
        node && typeof node === "object"
          ? (node as Record<string, unknown>)[part]
          : undefined,
      english,
    );

const MACOS = "settings.advanced.pasteMethod.descriptionMacos";
const OTHERS = "settings.advanced.pasteMethod.description";

const cases: Array<[string, () => void]> = [
  [
    "macOS gets a text of its own",
    () => assert.equal(pasteMethodDescriptionKey("macos"), MACOS),
  ],
  [
    "Windows keeps Handy's key",
    () => assert.equal(pasteMethodDescriptionKey("windows"), OTHERS),
  ],
  [
    "Linux keeps Handy's key",
    () => assert.equal(pasteMethodDescriptionKey("linux"), OTHERS),
  ],
  [
    "an unknown system keeps Handy's key",
    () => assert.equal(pasteMethodDescriptionKey("unknown"), OTHERS),
  ],
  [
    "every key has an English text",
    () => {
      for (const os of ["macos", "windows", "linux", "unknown"] as const) {
        const value = text(pasteMethodDescriptionKey(os));
        assert.equal(typeof value, "string", `${os}: no text`);
        assert.ok((value as string).trim().length > 0, `${os}: empty text`);
      }
    },
  ],
  [
    // macOS offers only the clipboard (Cmd+V) and None.
    "the macOS text explains its two options and not Direct",
    () => {
      const value = String(text(pasteMethodDescriptionKey("macos")));
      assert.match(value, /Clipboard \(Cmd\+V\)/);
      assert.match(value, /None/);
      assert.match(value, /Accessibility/);
      assert.doesNotMatch(value, /Direct/);
    },
  ],
  [
    "the Windows/Linux text explains every option offered there",
    () => {
      const value = String(text(pasteMethodDescriptionKey("windows")));
      for (const option of [
        "Clipboard (Ctrl+V)",
        "Ctrl+Shift+V",
        "Shift+Insert",
        "Direct",
        "External Script (Linux only)",
        "None",
      ]) {
        assert.ok(value.includes(option), `does not name ${option}`);
      }
      assert.doesNotMatch(value, /Accessibility/);
    },
  ],
];

let failed = 0;
for (const [name, check] of cases) {
  try {
    check();
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${name}: ${(error as Error).message.split("\n")[0]}`);
  }
}
if (failed > 0) {
  console.error(
    `pasteMethodDescription: ${failed} of ${cases.length} checks failed`,
  );
  process.exit(1);
}
console.log(`pasteMethodDescription: all ${cases.length} checks passed`);
