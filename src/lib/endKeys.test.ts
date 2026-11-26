// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/endKeys.test.ts
//
// DanyWhere: start and end shortcuts apart (qa 2026-11-22_2134). Every end
// has a key combination; without an own name it is called by it. A start
// shortcut ends a recording only through an end with its combination; a
// hold shortcut ends it when released (qa 2026-11-26_1810; its own checks:
// startKeys.test.ts). No key says any more whether a dictation is
// post-processed: an instruction shortcut or the profile's mode does
// ("Schnittstelle Phase 2").
import assert from "node:assert/strict";
import type { EndKey } from "../bindings";
import {
  canRemove,
  endKeyErrorKey,
  endKeyName,
  nameToStore,
  newEndKey,
  postProcessingOn,
  removeEndKey,
  updateEndKey,
} from "./endKeys";
import { formatKeyCombination } from "./utils/keyboard";
import en from "../i18n/locales/en/translation.json";
import de from "../i18n/locales/de/translation.json";

type Tree = { [key: string]: unknown };

/** The value at `key` in `tree`, or undefined. */
const lookup = (tree: Tree, key: string): unknown =>
  key
    .split(".")
    .reduce<unknown>(
      (node, part) =>
        node && typeof node === "object" ? (node as Tree)[part] : undefined,
      tree,
    );

/** i18next's lookup and {{name}} interpolation, enough for these strings. */
const translator =
  (tree: Tree) =>
  (key: string, options: Record<string, unknown> = {}): string => {
    const found = lookup(tree, key);
    if (typeof found !== "string") return `MISSING ${key}`;
    return found.replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
      String(options[name]),
    );
  };

const t = translator(en as Tree);
const tDe = translator(de as Tree);

/** The combination as the shortcut controls write it, on a Mac. */
const macEn = (keys: string) => formatKeyCombination(keys, "macos", t);
const macDe = (keys: string) => formatKeyCombination(keys, "macos", tDe);
const winDe = (keys: string) => formatKeyCombination(keys, "windows", tDe);

const end = (id: string, shortcut: string, name = ""): EndKey => ({
  id,
  name,
  shortcut,
  append_text: "",
  send_context: false,
});

/** Out of the box: both ends nameless, with the start shortcuts' keys. */
const factory = [
  end("plain", "ctrl+space"),
  end("post_process", "ctrl+shift+space"),
];

const cases: Array<[string, () => void]> = [
  [
    "an end without an own name is called by its combination, in the interface's language",
    () => {
      const plain = factory[0];
      assert.equal(endKeyName(plain, macEn), macEn("ctrl+space"));
      assert.equal(endKeyName(plain, macDe), macDe("ctrl+space"));
      assert.notEqual(
        macEn("ctrl+space"),
        macDe("ctrl+space"),
        "the key's name follows the language",
      );
      assert.equal(endKeyName(plain, winDe), winDe("ctrl+space"));
      // The ids of the two out of the box are nothing special any more.
      assert.equal(endKeyName(factory[1], macEn), macEn("ctrl+shift+space"));
      // It follows the combination when that changes.
      const changed = updateEndKey(factory, "plain", {
        shortcut: "ctrl+enter",
      })[0];
      assert.equal(endKeyName(changed, macDe), macDe("ctrl+enter"));
    },
  ],
  [
    "an own name is shown as typed",
    () => {
      assert.equal(
        endKeyName(end("send", "ctrl+enter", "Senden"), macEn),
        "Senden",
      );
      assert.equal(
        endKeyName(end("plain", "ctrl+space", "Leise"), macDe),
        "Leise",
      );
    },
  ],
  [
    "a blank name or one equal to the combination is stored as no own name",
    () => {
      // What the name follows without one of its own: the combination.
      const plainEn = macEn(factory[0].shortcut);
      const plainDe = macDe(factory[0].shortcut);
      assert.equal(nameToStore("", plainEn), "");
      assert.equal(nameToStore("   ", plainEn), "");
      assert.equal(nameToStore(macEn("ctrl+space"), plainEn), "");
      assert.equal(nameToStore(` ${macDe("ctrl+space")} `, plainDe), "");
      assert.equal(nameToStore(" Senden ", plainEn), "Senden");
      // Emptying an own name makes it follow the combination again.
      assert.equal(nameToStore("", macEn("ctrl+enter")), "");
    },
  ],
  [
    "a new end has the captured combination, an id none has and no own name",
    () => {
      // Hold and instruction shortcuts have the same shape, with ids of
      // their own.
      assert.equal(newEndKey(factory, "ctrl+h", 1000, "hold").id, "hold_1000");
      assert.deepEqual(newEndKey(factory, "tab", 1000, "instruction"), {
        id: "instruction_1000",
        name: "",
        shortcut: "tab",
        append_text: "",
        send_context: false,
      });
      const first = newEndKey(factory, "ctrl+enter", 1000);
      assert.deepEqual(first, {
        id: "end_1000",
        name: "",
        shortcut: "ctrl+enter",
        append_text: "",
        send_context: false,
      });
      const second = newEndKey([...factory, first], "ctrl+alt+enter", 1000);
      assert.equal(second.id, "end_1000_2");
      assert.equal(second.shortcut, "ctrl+alt+enter");
      const third = newEndKey([...factory, first, second], "f5", 1000);
      assert.equal(third.id, "end_1000_3");
    },
  ],
  [
    "changing or removing one entry leaves the others as they are",
    () => {
      const list = [...factory, end("send", "ctrl+enter", "Send")];
      const changed = updateEndKey(list, "send", { append_text: "{Enter}" });
      assert.equal(changed[2].append_text, "{Enter}");
      assert.deepEqual(changed.slice(0, 2), factory);
      assert.equal(list[2].append_text, "", "the list given stays as it was");
      assert.deepEqual(removeEndKey(changed, "send"), factory);
    },
  ],
  [
    "the last end can't go",
    () => {
      assert.equal(canRemove(factory), true);
      assert.equal(canRemove([factory[0]]), false);
      assert.equal(canRemove([]), false);
    },
  ],
  [
    "the backend's refusals have a text of their own",
    () => {
      const expected: Array<[string, string]> = [
        [
          "end-key-needs-shortcut:end_1",
          "settings.endKeys.errors.needsShortcut",
        ],
        [
          "end-key-shortcut-twice:ctrl+enter",
          "settings.endKeys.errors.shortcutTwice",
        ],
        ["end-key-last", "settings.endKeys.errors.last"],
        ["end-key-cannot-end", "settings.endKeys.errors.cannotEnd"],
      ];
      for (const [error, key] of expected) {
        assert.equal(endKeyErrorKey(error), key, error);
        // As the command gives it, inside a longer message.
        assert.equal(endKeyErrorKey(`Invalid: ${error}`), key, error);
        assert.ok(!t(key).startsWith("MISSING"), key);
        assert.ok(!tDe(key).startsWith("MISSING"), key);
      }
      assert.equal(endKeyErrorKey("conflict:cancel"), null);
      assert.equal(endKeyErrorKey("something else"), null);
    },
  ],
  [
    "the texts of start and end shortcuts are there in English and German, the old ones gone",
    () => {
      const present = [
        "settings.endKeys.name",
        "settings.endKeys.sendContext",
        "settings.endKeys.addPressKeys",
        "settings.endKeys.lastOne",
        "settings.endKeys.linuxNoShortcuts",
        "settings.endKeys.sameAsHold",
      ];
      for (const key of present) {
        assert.ok(!t(key).startsWith("MISSING"), `en ${key}`);
        assert.ok(!tDe(key).startsWith("MISSING"), `de ${key}`);
      }
      assert.equal(
        t("settings.endKeys.held", { shortcut: "⌃ Space" }),
        "⌃ Space held",
      );
      assert.equal(
        tDe("settings.endKeys.held", { shortcut: "⌃ Leertaste" }),
        "⌃ Leertaste gehalten",
      );
      // Start shortcuts have no names: the hint says which kind it is.
      assert.equal(
        t("settings.endKeys.sameAsStart"),
        "Same combination as a start shortcut: tapping it again during a recording ends this way.",
      );
      assert.equal(
        tDe("settings.endKeys.sameAsStart"),
        "Dieselbe Tastenkombination wie eine Starttaste: Erneutes Antippen während einer Aufnahme beendet so.",
      );
      assert.equal(
        tDe("settings.endKeys.sameAsHold"),
        "Dieselbe Tastenkombination wie eine Haltetaste: Antippen während einer Aufnahme beendet so.",
      );
      const gone = [
        "settings.endKeys.builtin",
        "settings.endKeys.noShortcut",
        "settings.endKeys.clearShortcut",
        "settings.endKeys.newName",
        "settings.endKeys.inUse",
        "settings.endKeys.defaultEnd",
        "settings.endKeys.errors.needsName",
        "settings.profileView.fields.defaultEnds",
        // The second start shortcut moved to General (user, 2026-11-23).
        "settings.postProcessing.hotkey",
        // "When Held" and "When Tapped" gave way to the hold shortcuts.
        "settings.endKeys.holdEnds",
        "settings.endKeys.tapEnds",
        "settings.profileView.fields.holdEnds",
        // No key says whether a dictation is post-processed any more, and
        // the switch became a choice (qa 2026-11-26_1810, Phase 2).
        "settings.endKeys.postProcess",
        "settings.endKeys.postProcessOff",
        "settings.holdKeys.notTaken",
        "settings.debug.postProcessingToggle",
      ];
      for (const key of gone) {
        assert.equal(lookup(en as Tree, key), undefined, `en ${key}`);
        assert.equal(lookup(de as Tree, key), undefined, `de ${key}`);
      }
    },
  ],
];

// Whether a dictation with these values may post-process: the profile's
// mode isn't "never" (qa 2026-11-26_1810, "Schnittstelle Phase 2"); with
// "with_instruction_key" an instruction shortcut decides, with "always"
// every dictation is. Unknown counts as never, so nothing goes out.
cases.push([
  "a dictation may post-process unless its values' mode is never",
  () => {
    assert.equal(
      postProcessingOn({ post_process_mode: "with_instruction_key" }),
      true,
    );
    assert.equal(postProcessingOn({ post_process_mode: "always" }), true);
    assert.equal(postProcessingOn({ post_process_mode: "never" }), false);
    assert.equal(postProcessingOn({ post_process_mode: null }), false);
    assert.equal(postProcessingOn({}), false, "not known: never");
  },
]);
cases.push([
  "the end shortcuts' texts say nothing of post-processing but where context counts",
  () => {
    for (const tr of [t, tDe]) {
      const text = tr("settings.endKeys.description");
      assert.ok(!/post-processed\b|nachbearbeitet wird/.test(text), text);
    }
    assert.equal(
      t("settings.endKeys.sendContextHint"),
      "Earlier dictations from the same window go along as context. It counts only when the dictation is post-processed.",
    );
    assert.equal(
      tDe("settings.endKeys.sendContextHint"),
      "Frühere Diktate aus demselben Fenster gehen als Kontext mit. Das wirkt nur, wenn das Diktat nachbearbeitet wird.",
    );
  },
]);

// Holding and tapping are the two gestures (user, 2026-11-24; sidecar qa
// 2026-11-24_1716): a short press is "tap" / "antippen" wherever it stands
// against holding, never "press" / "drücken" / "Druck", which holding is
// too, and never a bare "Tippen", which is typing text in German.
const SHORT_PRESS_KEYS = [
  "settings.endKeys.description",
  "settings.endKeys.sameAsStart",
  "settings.endKeys.linuxNoShortcuts",
  "settings.endKeys.sameAsHold",
  "settings.general.dictationKeyInserts.description",
  "settings.startKeys.description",
  "settings.startKeys.shortcutHint",
  "settings.holdKeys.alsoStart",
  "settings.holdKeys.holdOnly",
];
const SHORT_PRESS_DE_ONLY = [
  "settings.general.dictationKeyInserts.label",
  "settings.debug.holdThreshold.description",
  "onboarding.demo.tap",
  "onboarding.try.tapNext",
  "onboarding.try.tapAgain",
  "onboarding.try.doneTap",
  "onboarding.done.subMac",
  "onboarding.done.subTray",
];
cases.push(
  [
    "the hold threshold speaks of hold shortcuts, not of a mode Auto",
    () => {
      const english = t("settings.debug.holdThreshold.description");
      const german = tDe("settings.debug.holdThreshold.description");
      assert.ok(!/Auto/.test(english), english);
      assert.ok(!/Automatisch/.test(german), german);
      assert.ok(/hold shortcut/.test(english), english);
      assert.ok(/Haltetaste/.test(german), german);
    },
  ],
  [
    "a short press is tapping, in English and German",
    () => {
      for (const key of SHORT_PRESS_KEYS) {
        const english = t(key, { start: "Start Shortcut" });
        assert.ok(!/\bpress/i.test(english), `en ${key}: ${english}`);
        assert.ok(/\btap/i.test(english), `en ${key}: ${english}`);
      }
      for (const key of [...SHORT_PRESS_KEYS, ...SHORT_PRESS_DE_ONLY]) {
        const german = tDe(key, { start: "Starttaste" });
        assert.ok(
          // "gedrückt" belongs to holding ("gedrückt halten").
          !/Druck|(?<!ge)drück|\bTippen\b|tippe darauf|Tippe (auf|erneut auf)|wo du tippen/.test(
            german,
          ),
          `de ${key}: ${german}`,
        );
        assert.ok(
          /[Aa]ntipp|[Tt]ipp\S* .*\ban\b/.test(german),
          `de ${key}: ${german}`,
        );
      }
    },
  ],
);

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
