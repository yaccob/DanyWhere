// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/endKeys.test.ts
//
// DanyWhere: start and end shortcuts apart (qa 2026-11-22_2134). Every end
// has a key combination; without an own name it is called by it. A start
// shortcut ends a recording only through an end with its combination, and
// when held, through its end for holding.
import assert from "node:assert/strict";
import type { EndKey, HoldEnd, ShortcutBinding } from "../bindings";
import {
  canRemove,
  endKeyErrorKey,
  endKeyName,
  holdEndOf,
  holdPossible,
  nameToStore,
  newEndKey,
  postProcessingOn,
  removeEndKey,
  secondStartTaken,
  startsSharing,
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

const end = (
  id: string,
  shortcut: string,
  name = "",
  post_process = false,
): EndKey => ({
  id,
  name,
  shortcut,
  append_text: "",
  post_process,
  send_context: false,
});

/** Out of the box: both ends nameless, with the start shortcuts' keys. */
const factory = [
  end("plain", "ctrl+space"),
  end("post_process", "ctrl+shift+space", "", true),
];

const binding = (id: string, keys: string): ShortcutBinding => ({
  id,
  name: id,
  description: "",
  default_binding: keys,
  current_binding: keys,
});

const bindings: Partial<Record<string, ShortcutBinding>> = {
  transcribe: binding("transcribe", "ctrl+space"),
  transcribe_with_post_process: binding(
    "transcribe_with_post_process",
    "ctrl+shift+space",
  ),
  paste_last_transcript: binding("paste_last_transcript", "ctrl+shift+v"),
  cancel: binding("cancel", "escape"),
};

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
      const plain = factory[0];
      assert.equal(nameToStore("", plain, macEn), "");
      assert.equal(nameToStore("   ", plain, macEn), "");
      assert.equal(nameToStore(macEn("ctrl+space"), plain, macEn), "");
      assert.equal(nameToStore(` ${macDe("ctrl+space")} `, plain, macDe), "");
      assert.equal(nameToStore(" Senden ", plain, macEn), "Senden");
      // Emptying an own name makes it follow the combination again.
      const named = end("send", "ctrl+enter", "Senden");
      assert.equal(nameToStore("", named, macEn), "");
    },
  ],
  [
    "a new end has the captured combination, an id none has and no own name",
    () => {
      const first = newEndKey(factory, "ctrl+enter", 1000);
      assert.deepEqual(first, {
        id: "end_1000",
        name: "",
        shortcut: "ctrl+enter",
        append_text: "",
        post_process: false,
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
    "an end with a start shortcut's exact combination names that start shortcut",
    () => {
      assert.deepEqual(startsSharing(factory[0], bindings), ["transcribe"]);
      assert.deepEqual(startsSharing(factory[1], bindings), [
        "transcribe_with_post_process",
      ]);
      // Stored with other spacing or case, it is still the same.
      assert.deepEqual(startsSharing(end("x", " Ctrl+Space "), bindings), [
        "transcribe",
      ]);
      // A side is another combination; a covering one is no exact match.
      assert.deepEqual(
        startsSharing(end("x", "ctrl_left+space"), bindings),
        [],
      );
      // Only start shortcuts count, not the others.
      assert.deepEqual(startsSharing(end("x", "ctrl+shift+v"), bindings), []);
      assert.deepEqual(startsSharing(end("x", "ctrl+enter"), bindings), []);
      // Both start shortcuts on the same keys: both are named.
      const same = {
        ...bindings,
        transcribe_with_post_process: binding(
          "transcribe_with_post_process",
          "ctrl+space",
        ),
      };
      assert.deepEqual(startsSharing(factory[0], same), [
        "transcribe",
        "transcribe_with_post_process",
      ]);
      // A start shortcut without keys shares nothing with an end without.
      const none = { transcribe: binding("transcribe", "") };
      assert.deepEqual(startsSharing(end("x", ""), none), []);
    },
  ],
  [
    "a start shortcut without its own end for holding ends as out of the box",
    () => {
      const plain: HoldEnd = {
        append_text: "",
        post_process: false,
        send_context: false,
      };
      assert.deepEqual(holdEndOf("transcribe", {}), plain);
      assert.deepEqual(holdEndOf("transcribe_with_post_process", {}), {
        ...plain,
        post_process: true,
      });
      const own: HoldEnd = {
        append_text: "\\n",
        post_process: true,
        send_context: true,
      };
      assert.deepEqual(holdEndOf("transcribe", { transcribe: own }), own);
      // An entry for one start shortcut leaves the other's as it was.
      assert.deepEqual(
        holdEndOf("transcribe_with_post_process", { transcribe: own }),
        { ...plain, post_process: true },
      );
    },
  ],
  [
    "holding is possible unless the behaviour is Toggle",
    () => {
      assert.equal(holdPossible("toggle"), false);
      assert.equal(holdPossible("push_to_talk"), true);
      assert.equal(holdPossible("hold_or_toggle"), true);
      // Nothing stored means the default, Auto.
      assert.equal(holdPossible(undefined), true);
      assert.equal(holdPossible(null), true);
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
        "settings.endKeys.postProcess",
        "settings.endKeys.addPressKeys",
        "settings.endKeys.lastOne",
        "settings.endKeys.linuxNoShortcuts",
        "settings.endKeys.holdEnds.title",
        "settings.endKeys.holdEnds.description",
        "settings.profileView.fields.holdEnds",
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
      assert.ok(
        tDe("settings.endKeys.sameAsStart", { start: "Starttaste" }).includes(
          "„Starttaste“",
        ),
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
      ];
      for (const key of gone) {
        assert.equal(lookup(en as Tree, key), undefined, `en ${key}`);
        assert.equal(lookup(de as Tree, key), undefined, `de ${key}`);
      }
    },
  ],
];

// The second start shortcut is taken as `app_profiles::post_processing_reachable`
// says (user, 2026-11-23, option a): "When Held" shows it only then.
const profile = (id: string, apps: string[], postProcess: boolean) => ({
  id,
  apps,
  values: { post_process_enabled: postProcess },
});
cases.push(
  [
    "the second start shortcut is taken while the values in use post-process",
    () => {
      assert.equal(
        secondStartTaken({ post_process_enabled: true, profiles: [] }),
        true,
      );
      assert.equal(
        secondStartTaken({ post_process_enabled: false, profiles: [] }),
        false,
      );
    },
  ],
  [
    "or while another profile with applications post-processes, chosen by application",
    () => {
      const settings = {
        post_process_enabled: false,
        profile_auto_by_app: true,
        active_profile_id: "default",
        profiles: [profile("work", ["com.microsoft.VSCode"], true)],
      };
      assert.equal(secondStartTaken(settings), true);
      assert.equal(
        secondStartTaken({ ...settings, profile_auto_by_app: false }),
        false,
        "not chosen by application",
      );
      assert.equal(
        secondStartTaken({ ...settings, active_profile_id: "work" }),
        false,
        "the profile in use counts through the flat values",
      );
      assert.equal(
        secondStartTaken({
          ...settings,
          profiles: [profile("work", [], true)],
        }),
        false,
        "without applications",
      );
    },
  ],
);

// With post-processing off no end post-processes (user, 2026-11-23, option
// a; `app_profiles::effective_post_process`): the pages say so.
cases.push([
  "a dictation can post-process only with its values' switch on",
  () => {
    assert.equal(postProcessingOn({ post_process_enabled: true }), true);
    assert.equal(postProcessingOn({ post_process_enabled: false }), false);
    assert.equal(postProcessingOn({ post_process_enabled: null }), false);
    assert.equal(postProcessingOn({}), false, "not known: off");
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
  "settings.general.shortcut.bindings.transcribe_with_post_process.descriptionMacos",
  "settings.general.dictationKeyInserts.description",
  "settings.general.shortcutActivation.descriptions.toggle",
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
    "the two ways to end are When Held and When Tapped, the behavior that only taps is Tap",
    () => {
      assert.equal(t("settings.endKeys.holdEnds.title"), "When Held");
      assert.equal(t("settings.endKeys.tapEnds.title"), "When Tapped");
      assert.equal(tDe("settings.endKeys.holdEnds.title"), "Beim Halten");
      assert.equal(tDe("settings.endKeys.tapEnds.title"), "Beim Antippen");
      assert.equal(
        tDe("settings.endKeys.tapEnds.description"),
        "Wie das Antippen einer Endtaste die Aufnahme beendet.",
      );
      assert.equal(
        t("settings.general.shortcutActivation.options.toggle"),
        "Tap",
      );
      assert.equal(
        tDe("settings.general.shortcutActivation.options.toggle"),
        "Antippen",
      );
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
