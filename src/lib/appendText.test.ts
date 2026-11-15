// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/appendText.test.ts
import assert from "node:assert/strict";
import type { AppendPart } from "../bindings";
import {
  appendErrorMessage,
  appendKeyLabel,
  describeAppend,
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
          "linux",
          "en",
        ),
        "“MfG”, then new line, then tab character",
      ),
  ],
  [
    "a key alone starts with a capital letter",
    () =>
      assert.equal(
        describeAppend([{ kind: "key", key: "tab" }], t, "macos", "en"),
        "The Tab key",
      ),
  ],
  [
    "Cmd is Super off macOS, as Handy's auto submit",
    () => {
      assert.equal(appendKeyLabel("cmd_enter", "macos"), "Cmd+Enter");
      assert.equal(appendKeyLabel("cmd_enter", "windows"), "Super+Enter");
      assert.equal(appendKeyLabel("cmd_enter", "linux"), "Super+Enter");
      assert.equal(appendKeyLabel("shift_enter", "windows"), "Shift+Enter");
      assert.equal(appendKeyLabel("ctrl_enter", "macos"), "Ctrl+Enter");
    },
  ],
  [
    "every error has its text, with what it is about",
    () => {
      const message = (code: string, detail = "") =>
        appendErrorMessage(
          { code, detail } as Parameters<typeof appendErrorMessage>[0],
          t,
        );
      assert.match(message("unknown_escape", "\\x"), /\\x/);
      assert.match(message("unknown_key", "Foo"), /\{Foo\}/);
      for (const code of [
        "trailing_backslash",
        "unclosed_key",
        "unmatched_brace",
        "text_after_key",
      ]) {
        assert.doesNotMatch(message(code), /MISSING/, code);
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
