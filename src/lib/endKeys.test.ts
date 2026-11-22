// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/endKeys.test.ts
import assert from "node:assert/strict";
import type { EndKey } from "../bindings";
import {
  PLAIN_ID,
  POST_PROCESS_ID,
  defaultEndOf,
  endKeyErrorKey,
  endKeyName,
  endKeyOptions,
  newEndKey,
  removeEndKey,
  startsEndingWith,
  updateEndKey,
} from "./endKeys";
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

const end = (id: string, name = "", shortcut = ""): EndKey => ({
  id,
  name,
  shortcut,
  append_text: "",
  post_process: id === POST_PROCESS_ID,
  send_context: false,
});

const builtin = [end(PLAIN_ID), end(POST_PROCESS_ID)];
const defaults = {
  transcribe: PLAIN_ID,
  transcribe_with_post_process: POST_PROCESS_ID,
};

const cases: Array<[string, () => void]> = [
  [
    "the built-in ones show their translated names, the others their own",
    () => {
      assert.equal(endKeyName(end(PLAIN_ID), t), "Plain");
      assert.equal(endKeyName(end(POST_PROCESS_ID), t), "Post-process");
      assert.equal(endKeyName(end(PLAIN_ID), tDe), "Normal");
      assert.equal(endKeyName(end(POST_PROCESS_ID), tDe), "Nachbearbeiten");
      assert.equal(endKeyName(end("send", "Senden"), t), "Senden");
      // A built-in one the user renamed keeps the user's name.
      assert.equal(endKeyName(end(PLAIN_ID, "Leise"), t), "Leise");
    },
  ],
  [
    "a new end shortcut gets an id and a name no other one has",
    () => {
      const first = newEndKey(builtin, "New End Shortcut", 1000);
      assert.equal(first.id, "end_1000");
      assert.equal(first.name, "New End Shortcut");
      assert.equal(first.shortcut, "");
      assert.equal(first.post_process, false);
      const second = newEndKey([...builtin, first], "New End Shortcut", 1000);
      assert.notEqual(second.id, first.id);
      assert.equal(second.name, "New End Shortcut 2");
    },
  ],
  [
    "changing or removing one entry leaves the others as they are",
    () => {
      const list = [...builtin, end("send", "Send", "ctrl+enter")];
      const changed = updateEndKey(list, "send", { append_text: "{Enter}" });
      assert.equal(changed[2].append_text, "{Enter}");
      assert.deepEqual(changed.slice(0, 2), builtin);
      assert.equal(list[2].append_text, "", "the list given stays as it was");
      assert.deepEqual(removeEndKey(changed, "send"), builtin);
    },
  ],
  [
    "an end shortcut in use as a default end is named with its start shortcuts",
    () => {
      assert.deepEqual(startsEndingWith(PLAIN_ID, defaults), ["transcribe"]);
      assert.deepEqual(startsEndingWith("send", defaults), []);
      assert.deepEqual(
        startsEndingWith(PLAIN_ID, {
          transcribe: PLAIN_ID,
          transcribe_with_post_process: PLAIN_ID,
        }),
        ["transcribe", "transcribe_with_post_process"],
      );
    },
  ],
  [
    "a start shortcut without an entry ends with its built-in default end",
    () => {
      assert.equal(defaultEndOf("transcribe", {}), PLAIN_ID);
      assert.equal(
        defaultEndOf("transcribe_with_post_process", {}),
        POST_PROCESS_ID,
      );
      assert.equal(defaultEndOf("transcribe", { transcribe: "send" }), "send");
    },
  ],
  [
    "the default end can be chosen from every end shortcut, by its name",
    () => {
      const options = endKeyOptions([...builtin, end("send", "Send")], t);
      assert.deepEqual(
        options.map((option) => [option.value, option.label]),
        [
          [PLAIN_ID, "Plain"],
          [POST_PROCESS_ID, "Post-process"],
          ["send", "Send"],
        ],
      );
    },
  ],
  [
    "the backend's refusals have a text of their own",
    () => {
      assert.equal(
        endKeyErrorKey("end-key-needs-name:end_1"),
        "settings.endKeys.errors.needsName",
      );
      assert.equal(
        endKeyErrorKey("end-key-shortcut-twice:ctrl+enter"),
        "settings.endKeys.errors.shortcutTwice",
      );
      assert.equal(
        endKeyErrorKey("end-key-in-use:plain"),
        "settings.endKeys.inUse",
      );
      assert.equal(endKeyErrorKey("something else"), null);
      for (const key of [
        "settings.endKeys.errors.needsName",
        "settings.endKeys.errors.shortcutTwice",
        "settings.endKeys.inUse",
      ]) {
        assert.ok(!t(key).startsWith("MISSING"), key);
        assert.ok(!tDe(key).startsWith("MISSING"), key);
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
  console.error(`endKeys: ${failed} of ${cases.length} checks failed`);
  process.exit(1);
}
console.log(`endKeys: all ${cases.length} checks passed`);
