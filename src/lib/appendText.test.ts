// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/appendText.test.ts
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import type { AppendChoice, AppendKey, AppendPart } from "../bindings";
import {
  KEPT_KEYS,
  NO_KEY,
  appendKeyLabel,
  describeAppend,
  keyOptions,
  keysOfOption,
  selectedKeyOption,
} from "./appendText";
import en from "../i18n/locales/en/translation.json";
import de from "../i18n/locales/de/translation.json";

type Tree = { [key: string]: unknown };

/** i18next's lookup and {{name}} interpolation, enough for these strings. */
const translator =
  (tree: Tree) =>
  (key: string, options: Record<string, unknown> = {}): string => {
    const found = key
      .split(".")
      .reduce<unknown>(
        (node, part) =>
          node && typeof node === "object" ? (node as Tree)[part] : undefined,
        tree,
      );
    if (typeof found !== "string") return `MISSING ${key}`;
    return found.replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
      String(options[name]),
    );
  };

const t = translator(en as Tree);
const tDe = translator(de as Tree);

const space: AppendPart = { kind: "space" };
const enter: AppendPart = { kind: "key", key: "enter" };

/** What append_key_options returns on each system (append_text.rs). */
const MAC_KEYS: AppendKey[] = [
  "enter",
  "shift_enter",
  "cmd_enter",
  "ctrl_enter",
  "tab",
];
const PC_KEYS: AppendKey[] = ["enter", "shift_enter", "ctrl_enter", "tab"];

const choice = (keys: AppendKey[], kept = false): AppendChoice => ({
  text: "",
  keys,
  kept,
});

const cases: Array<[string, () => void]> = [
  [
    "an empty field says that nothing is appended",
    () => assert.equal(describeAppend([], t, "macos"), "Nothing is appended"),
  ],
  [
    "a space and Enter, as the design's example",
    () => {
      assert.equal(
        describeAppend([space, enter], t, "macos", "en"),
        "Space, then the Enter key",
      );
      assert.equal(
        describeAppend([space, enter], tDe, "macos", "de"),
        "Leerzeichen, dann Taste Enter",
      );
    },
  ],
  [
    "text, new line and tab are named in their order",
    () =>
      assert.equal(
        describeAppend(
          [
            { kind: "text", text: "MfG" },
            { kind: "new_line" },
            { kind: "tab" },
          ],
          t,
          "macos",
          "en",
        ),
        "“MfG”, then new line, then tab character",
      ),
  ],
  [
    "a key alone starts with a capital letter",
    () =>
      assert.equal(
        describeAppend([{ kind: "key", key: "tab" }], t, "windows", "en"),
        "The Tab key",
      ),
  ],
  [
    "each key press is labelled as the app shows shortcuts on the system",
    () => {
      const labels = (os: "macos" | "windows" | "linux") =>
        (
          ["enter", "shift_enter", "cmd_enter", "ctrl_enter", "tab"] as const
        ).map((key) => appendKeyLabel(key, os));
      assert.deepEqual(labels("macos"), [
        "Enter",
        "⇧ Enter",
        "⌘ Enter",
        "⌃ Enter",
        "Tab",
      ]);
      for (const os of ["windows", "linux"] as const) {
        assert.deepEqual(
          labels(os),
          ["Enter", "Shift + Enter", "⊞ + Enter", "Ctrl + Enter", "Tab"],
          os,
        );
      }
    },
  ],
  [
    "the preview names a key press with the same label",
    () => {
      const cmd: AppendPart = { kind: "key", key: "cmd_enter" };
      const ctrl: AppendPart = { kind: "key", key: "ctrl_enter" };
      assert.equal(describeAppend([cmd], t, "macos", "en"), "The ⌘ Enter key");
      assert.equal(describeAppend([cmd], tDe, "macos", "de"), "Taste ⌘ Enter");
      assert.equal(
        describeAppend([space, ctrl], t, "windows", "en"),
        "Space, then the Ctrl + Enter key",
      );
    },
  ],
  [
    "the selection offers no key press first, then this system's",
    () => {
      const mac = keyOptions(MAC_KEYS, choice([]), t, "macos");
      assert.deepEqual(
        mac.map((option) => option.value),
        [NO_KEY, ...MAC_KEYS],
      );
      assert.deepEqual(
        mac.map((option) => option.label),
        ["No key press", "Enter", "⇧ Enter", "⌘ Enter", "⌃ Enter", "Tab"],
      );
      const windows = keyOptions(PC_KEYS, choice([]), tDe, "windows");
      assert.deepEqual(
        windows.map((option) => option.label),
        ["Keine Taste", "Enter", "Shift + Enter", "Ctrl + Enter", "Tab"],
      );
      assert.ok(windows.every((option) => !option.description));
    },
  ],
  [
    "none or one offered key press selects its own entry",
    () => {
      assert.equal(selectedKeyOption(choice([])), NO_KEY);
      assert.equal(selectedKeyOption(choice(["enter"])), "enter");
      assert.equal(selectedKeyOption(choice(["cmd_enter"])), "cmd_enter");
      assert.deepEqual(keysOfOption(NO_KEY, choice(["enter"])), []);
      assert.deepEqual(keysOfOption("shift_enter", choice([])), [
        "shift_enter",
      ]);
    },
  ],
  [
    "stored key presses the selection doesn't offer are an extra entry",
    () => {
      const several = choice(["tab", "enter"], true);
      const options = keyOptions(PC_KEYS, several, t, "linux");
      assert.equal(options.length, PC_KEYS.length + 2);
      const kept = options[options.length - 1];
      assert.equal(kept.value, KEPT_KEYS);
      assert.equal(kept.label, "Tab, then Enter");
      assert.ok(kept.description && !kept.description.startsWith("MISSING"));
      assert.equal(selectedKeyOption(several), KEPT_KEYS);
      assert.deepEqual(keysOfOption(KEPT_KEYS, several), ["tab", "enter"]);

      const win = choice(["cmd_enter"], true);
      const winOptions = keyOptions(PC_KEYS, win, tDe, "windows");
      assert.equal(winOptions[winOptions.length - 1].label, "⊞ + Enter");
      assert.equal(selectedKeyOption(win), KEPT_KEYS);
    },
  ],
  [
    "every locale describes the text's escapes and no key syntax",
    () => {
      const dir = new URL("../i18n/locales/", import.meta.url);
      const locales = readdirSync(dir);
      assert.equal(locales.length, 26);
      for (const locale of locales) {
        const tree = JSON.parse(
          readFileSync(new URL(`${locale}/translation.json`, dir), "utf8"),
        ) as Tree;
        const translate = translator(tree);
        const description = translate(
          `settings.advanced.appendText.description`,
        );
        for (const escape of ["\\s", "\\n", "\\t", "\\\\"]) {
          assert.ok(description.includes(escape), `${locale} ${escape}`);
        }
        assert.doesNotMatch(
          description,
          /\{[A-Za-z+]+\}|\\\{|\\\}|\{\{|MISSING/,
          locale,
        );
        for (const key of ["keyNone", "keyKept", "noAccessibility"]) {
          const text = translate(`settings.advanced.appendText.${key}`);
          assert.ok(
            text.trim() && !text.startsWith("MISSING"),
            `${locale} ${key}`,
          );
        }
        const node = ((tree.settings as Tree).advanced as Tree)
          .appendText as Tree;
        for (const gone of ["errors", "notSaved"]) {
          assert.ok(!(gone in node), `${locale} still has ${gone}`);
        }
      }
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
  console.error(`appendText: ${failed} of ${cases.length} checks failed`);
  process.exit(1);
}
console.log(`appendText: all ${cases.length} checks passed`);
