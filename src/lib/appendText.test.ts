// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/appendText.test.ts
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import type { AppendKeyNames, AppendPart, KeyPlatform } from "../bindings";
import {
  appendDescription,
  appendErrorMessage,
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
const enter: AppendPart = { kind: "key", key: "enter", name: "Enter" };

/** What append_key_names returns on each system (append_text.rs). */
const NAMES: Record<KeyPlatform, AppendKeyNames> = {
  macos: { platform: "macos", cmd_enter: "Cmd+Enter" },
  windows: { platform: "windows", cmd_enter: "Win+Enter" },
  linux: { platform: "linux", cmd_enter: "Super+Enter" },
};
const META = { macos: "Cmd", windows: "Win", linux: "Super" };
const PLATFORMS = Object.keys(NAMES) as KeyPlatform[];

const cases: Array<[string, () => void]> = [
  [
    "an empty field says that nothing is appended",
    () => assert.equal(describeAppend([], t), "Nothing is appended"),
  ],
  [
    "a space and Enter, as the design's example",
    () => {
      assert.equal(
        describeAppend([space, enter], t, "en"),
        "Space, then the Enter key",
      );
      assert.equal(
        describeAppend([space, enter], tDe, "de"),
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
          "en",
        ),
        "“MfG”, then new line, then tab character",
      ),
  ],
  [
    "a key alone starts with a capital letter",
    () =>
      assert.equal(
        describeAppend([{ kind: "key", key: "tab", name: "Tab" }], t, "en"),
        "The Tab key",
      ),
  ],
  [
    "a key press is named as Rust names it for the system",
    () => {
      for (const platform of PLATFORMS) {
        const name = NAMES[platform].cmd_enter;
        const part: AppendPart = { kind: "key", key: "cmd_enter", name };
        assert.equal(describeAppend([part], t, "en"), `The ${name} key`);
        assert.equal(describeAppend([part], tDe, "de"), `Taste ${name}`);
      }
    },
  ],
  [
    "every error has its text, with what it is about",
    () => {
      const message = (code: string, detail = "") =>
        appendErrorMessage(
          { code, detail, suggestion: "" } as Parameters<
            typeof appendErrorMessage
          >[0],
          t,
          "macos",
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
  [
    "a name from another system names this system's key and the fix",
    () => {
      const foreign = (detail: string, suggestion: string) => ({
        code: "foreign_key_name" as const,
        detail,
        suggestion,
      });
      assert.equal(
        appendErrorMessage(foreign("Cmd", "Cmd+Enter"), t, "macos"),
        "On the Mac this key is called Cmd: {Cmd+Enter}",
      );
      assert.equal(
        appendErrorMessage(foreign("Cmd", "Cmd+Enter"), tDe, "macos"),
        "Auf dem Mac heißt diese Taste Cmd: {Cmd+Enter}",
      );
      assert.equal(
        appendErrorMessage(foreign("Win", "Win+Enter"), tDe, "windows"),
        "Unter Windows heißt diese Taste Win: {Win+Enter}",
      );
      assert.equal(
        appendErrorMessage(foreign("Enter", "Shift+Enter"), t, "windows"),
        "On Windows this key is called Enter: {Shift+Enter}",
      );
      assert.equal(
        appendErrorMessage(foreign("Super", "Super+Enter"), tDe, "linux"),
        "Unter Linux heißt diese Taste Super: {Super+Enter}",
      );
    },
  ],
  [
    "the description names this system's key press and no other",
    () => {
      for (const platform of PLATFORMS) {
        for (const [language, translate] of [
          ["en", t],
          ["de", tDe],
        ] as const) {
          const text = appendDescription(translate, NAMES[platform]);
          const label = `${language} on ${platform}`;
          assert.ok(text.includes(`{${META[platform]}+Enter}`), label);
          for (const other of PLATFORMS.filter((p) => p !== platform)) {
            assert.ok(!text.includes(META[other]), `${label}: ${other}`);
          }
          assert.doesNotMatch(text, /Windows|Linux|\{\{/, label);
        }
      }
    },
  ],
  [
    "every locale fills the Cmd key press from the system",
    () => {
      const dir = new URL("../i18n/locales/", import.meta.url);
      const locales = readdirSync(dir);
      assert.equal(locales.length, 26);
      for (const locale of locales) {
        const tree = JSON.parse(
          readFileSync(new URL(`${locale}/translation.json`, dir), "utf8"),
        ) as Tree;
        const translate = translator(tree);
        const raw = translate("settings.advanced.appendText.description", {
          cmdEnter: "{{cmdEnter}}",
        });
        assert.equal(raw.split("{{cmdEnter}}").length, 2, locale);
        assert.doesNotMatch(raw, /Cmd\+|Super\+|Win\+/, locale);
        for (const platform of PLATFORMS) {
          const text = translate(
            `settings.advanced.appendText.errors.foreignKeyName.${platform}`,
            { name: "NAME", key: "KEY" },
          );
          assert.doesNotMatch(text, /MISSING/, `${locale} ${platform}`);
          assert.match(text, /NAME[\s\S]*KEY/, `${locale} ${platform}`);
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
