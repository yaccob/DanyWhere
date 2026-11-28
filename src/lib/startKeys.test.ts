// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/startKeys.test.ts
//
// DanyWhere: start and hold shortcuts as lists of their own (sidecar qa
// 2026-11-26_1713, decision; 2026-11-26_1810, "Schnittstelle Phase 1"). A
// start shortcut is only a combination; a hold shortcut has the shape of an
// end shortcut and, without a name of its own, is called "<keys> held". The
// same combination may stand in several lists. Every layer keeps one start
// or hold shortcut. The backend's refusals for them read as sentences that
// name the shortcut by its name or its combination. Instruction shortcuts
// are a fourth list of the hold shortcuts' shape, named the same way ("⇥
// held"), and may be none (qa 2026-11-26_1810, "Schnittstelle Phase 2").
import assert from "node:assert/strict";
import type { EndKey } from "../bindings";
import {
  canRemoveStartOrHold,
  hasCombination,
  holdKeyName,
  keyRefLabel,
  keyRefusal,
  newStartKey,
  parseKeyRef,
  practiceChange,
  practiceShortcut,
  type StartKey,
} from "./startKeys";
import { nameToStore } from "./endKeys";
import { shortcutRefusalText } from "./shortcutRefusal";
import { formatKeyCombination } from "./utils/keyboard";
import en from "../i18n/locales/en/translation.json";
import de from "../i18n/locales/de/translation.json";

type Tree = { [key: string]: unknown };

const lookup = (tree: Tree, key: string): unknown =>
  key
    .split(".")
    .reduce<unknown>(
      (node, part) =>
        node && typeof node === "object" ? (node as Tree)[part] : undefined,
      tree,
    );

/** i18next's lookup and {{name}} interpolation, enough for these strings;
 * `defaultValue` as i18next takes it. */
const translator =
  (tree: Tree) =>
  (key: string, options: Record<string, unknown> = {}): string => {
    const found = lookup(tree, key);
    if (typeof found !== "string") {
      return typeof options.defaultValue === "string"
        ? options.defaultValue
        : `MISSING ${key}`;
    }
    return found.replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
      String(options[name]),
    );
  };

const t = translator(en as Tree);
const tDe = translator(de as Tree);
const macEn = (keys: string) => formatKeyCombination(keys, "macos", t);
const macDe = (keys: string) => formatKeyCombination(keys, "macos", tDe);
const heldEn = (shortcut: string) => t("settings.endKeys.held", { shortcut });
const heldDe = (shortcut: string) => tDe("settings.endKeys.held", { shortcut });

const start = (id: string, shortcut: string): StartKey => ({ id, shortcut });
const hold = (
  id: string,
  shortcut: string,
  extra: Partial<EndKey> = {},
): EndKey => ({
  id,
  name: "",
  shortcut,
  append_text: "",
  send_context: false,
  ...extra,
});

/** Out of the box on a Mac (qa 2026-11-26_1810, "Ab Werk"). */
const STARTS = [start("start", "ctrl+space")];
const HOLDS = [
  hold("hold_direct", "ctrl+space", { append_text: "\\s" }),
  hold("hold_post", "ctrl+shift+space", { send_context: true }),
];
const INSTRUCTIONS = [
  hold("instruction_tab", "tab", { append_text: "\\s", send_context: true }),
];
const ENDS = [
  hold("plain", "ctrl+space", { name: "Direkt · Leerzeichen" }),
  hold("end_1795307754299", "ctrl+return"),
];

const cases: Array<[string, () => void]> = [];
const check = (name: string, body: () => void) => cases.push([name, body]);

check(
  "a new start shortcut has the captured combination and an id none has",
  () => {
    assert.deepEqual(newStartKey(STARTS, "ctrl+e", 1000), {
      id: "start_1000",
      shortcut: "ctrl+e",
    });
    const first = newStartKey(STARTS, "ctrl+e", 1000);
    assert.equal(
      newStartKey([...STARTS, first], "f5", 1000).id,
      "start_1000_2",
    );
  },
);

check(
  "a hold shortcut without a name of its own is called after its keys, held",
  () => {
    assert.equal(holdKeyName(HOLDS[0], macEn, heldEn), "⌃ Space held");
    assert.equal(holdKeyName(HOLDS[1], macDe, heldDe), "⌃⇧ Leertaste gehalten");
    // It follows the combination when that changes.
    assert.equal(
      holdKeyName({ ...HOLDS[0], shortcut: "ctrl+alt+h" }, macEn, heldEn),
      "⌥⌃H held",
    );
    // A name of its own is shown as typed.
    assert.equal(
      holdKeyName({ ...HOLDS[1], name: "Polieren" }, macEn, heldEn),
      "Polieren",
    );
    // Blank or the name it follows: stored as no name of its own.
    assert.equal(nameToStore("  ", "⌃ Space held"), "");
    assert.equal(nameToStore(" ⌃ Space held ", "⌃ Space held"), "");
    assert.equal(nameToStore(" Polieren ", "⌃ Space held"), "Polieren");
  },
);

check("the last start or hold shortcut of a layer can't go", () => {
  assert.equal(canRemoveStartOrHold(STARTS, HOLDS), true);
  assert.equal(canRemoveStartOrHold(STARTS, []), false);
  assert.equal(canRemoveStartOrHold([], [HOLDS[0]]), false);
  assert.equal(canRemoveStartOrHold([], HOLDS), true);
  assert.equal(canRemoveStartOrHold([], []), false);
});

check(
  "only the exact same combination counts as shared between the lists",
  () => {
    assert.equal(hasCombination(HOLDS, "ctrl+space"), true);
    assert.equal(hasCombination(HOLDS, " Ctrl+Space "), true);
    assert.equal(hasCombination(STARTS, "ctrl+shift+space"), false);
    // A side is another combination; a covering one is no exact match.
    assert.equal(hasCombination(HOLDS, "ctrl_left+space"), false);
    assert.equal(hasCombination([start("x", "")], ""), false);
  },
);

check("a reference names a start, hold or end shortcut by its id", () => {
  assert.deepEqual(parseKeyRef("start_key:start"), {
    kind: "start",
    id: "start",
  });
  assert.deepEqual(parseKeyRef("hold_key:hold_1795300000001"), {
    kind: "hold",
    id: "hold_1795300000001",
  });
  assert.deepEqual(parseKeyRef("end_key:end_1795307754299"), {
    kind: "end",
    id: "end_1795307754299",
  });
  assert.deepEqual(parseKeyRef("instruction_key:instruction_tab"), {
    kind: "instruction",
    id: "instruction_tab",
  });
  assert.deepEqual(parseKeyRef("instruction_key:instruction_1795300000002"), {
    kind: "instruction",
    id: "instruction_1795300000002",
  });
  assert.equal(parseKeyRef("paste_last_transcript"), null);
  assert.equal(parseKeyRef("start_key:"), null);
});

const VALUES = {
  start_keys: STARTS,
  hold_keys: HOLDS,
  instruction_keys: INSTRUCTIONS,
  end_keys: ENDS,
};
const OTHER = {
  start_keys: [start("start_1795300000005", "ctrl+alt+s")],
  hold_keys: [],
  end_keys: [],
};

check(
  "a reference's label: a start shortcut's keys, a hold or end shortcut's name",
  () => {
    const label = (ref: string, lng: "en" | "de" = "en") =>
      keyRefLabel(
        parseKeyRef(ref)!,
        [VALUES, null, OTHER],
        lng === "en" ? macEn : macDe,
        lng === "en" ? heldEn : heldDe,
      );
    assert.equal(label("start_key:start"), "⌃ Space");
    assert.equal(label("start_key:start", "de"), "⌃ Leertaste");
    assert.equal(label("hold_key:hold_post"), "⌃⇧ Space held");
    assert.equal(label("instruction_key:instruction_tab"), "⇥ held");
    assert.equal(label("instruction_key:instruction_tab", "de"), "⇥ gehalten");
    assert.equal(label("end_key:plain"), "Direkt · Leerzeichen");
    assert.equal(label("end_key:end_1795307754299"), "⌃↩");
    // Found in any of the layers given.
    assert.equal(label("start_key:start_1795300000005"), "⌥⌃S");
    assert.equal(label("hold_key:hold_1795300000009"), null);
  },
);

/** The refusals of the backend for start and hold shortcuts, with what the
 * notice must say (qa 2026-11-26_1810, "Prüfung und Garantien"). */
const REFUSALS: Array<[string, string, string]> = [
  [
    "start-key-needs-shortcut:start_1795300000005",
    "Every start shortcut needs a key combination.",
    "Jede Starttaste braucht eine Tastenkombination.",
  ],
  [
    "hold-key-needs-shortcut:hold_post",
    "Every hold shortcut needs a key combination.",
    "Jede Haltetaste braucht eine Tastenkombination.",
  ],
  [
    "start-key-shortcut-twice:ctrl+space",
    "Two start shortcuts can't share a combination.",
    "Zwei Starttasten können nicht dieselbe Tastenkombination haben.",
  ],
  [
    "hold-key-shortcut-twice:ctrl+space",
    "Two hold shortcuts can't share a combination.",
    "Zwei Haltetasten können nicht dieselbe Tastenkombination haben.",
  ],
  [
    "start-key-last",
    "The last start or hold shortcut can't go: a recording must always be possible to start.",
    "Die letzte Start- oder Haltetaste kann nicht weg: Eine Aufnahme muss sich immer starten lassen.",
  ],
  [
    "start-key-cannot-end:start",
    "On Linux a recording that ⌃ Space starts couldn't be ended: keep an end shortcut with the same combination.",
    "Unter Linux ließe sich eine Aufnahme, die ⌃ Leertaste startet, nicht beenden: Behalte eine Endtaste mit derselben Tastenkombination.",
  ],
  [
    "conflict:start_key:start",
    "Start shortcut ⌃ Space already uses this shortcut",
    "„Starttaste ⌃ Leertaste“ verwendet dieses Kürzel schon",
  ],
  [
    "conflict:hold_key:hold_post",
    "Hold shortcut ⌃⇧ Space held already uses this shortcut",
    "„Haltetaste ⌃⇧ Leertaste gehalten“ verwendet dieses Kürzel schon",
  ],
  [
    "instruction-key-needs-shortcut:instruction_tab",
    "Every instruction shortcut needs a key combination.",
    "Jede Anweisungstaste braucht eine Tastenkombination.",
  ],
  [
    "instruction-key-shortcut-twice:tab",
    "Two instruction shortcuts can't share a combination.",
    "Zwei Anweisungstasten können nicht dieselbe Tastenkombination haben.",
  ],
  [
    "conflict:instruction_key:instruction_tab",
    "Instruction shortcut ⇥ held already uses this shortcut",
    "„Anweisungstaste ⇥ gehalten“ verwendet dieses Kürzel schon",
  ],
  [
    "conflict:end_key:plain",
    "End shortcut Direkt · Leerzeichen already uses this shortcut",
    "„Endtaste Direkt · Leerzeichen“ verwendet dieses Kürzel schon",
  ],
  [
    "conflict:paste_last_transcript",
    "Paste Last Transcript already uses this shortcut",
    "„Letzte Transkription einfügen“ verwendet dieses Kürzel schon",
  ],
  // The end shortcuts' own codes stay.
  [
    "end-key-shortcut-twice:ctrl+return",
    "Two end shortcuts can't share a shortcut.",
    "Zwei Endtasten können nicht dieselbe Tastenkombination haben.",
  ],
  [
    "end-key-cannot-end",
    "On Linux a recording could no longer be ended: keep an end shortcut with a start shortcut's combination.",
    "Unter Linux ließe sich eine Aufnahme nicht mehr beenden: Behalte eine Endtaste mit der Tastenkombination einer Starttaste.",
  ],
];

check(
  "each refusal of the backend reads as its sentence, in English and German",
  () => {
    for (const [error, english, german] of REFUSALS) {
      for (const [tr, format, held, expected] of [
        [t, macEn, heldEn, english],
        [tDe, macDe, heldDe, german],
      ] as const) {
        const label = (ref: Parameters<typeof keyRefLabel>[0]) =>
          keyRefLabel(ref, [VALUES], format, held);
        // As the command gives it, sometimes inside a longer message.
        assert.equal(shortcutRefusalText(error, tr, label), expected, error);
        assert.equal(
          shortcutRefusalText(`Invalid: ${error}`, tr, label),
          expected,
          error,
        );
      }
    }
  },
);

check("a refusal names its text and, where it has one, the shortcut", () => {
  assert.deepEqual(keyRefusal("start-key-last"), {
    key: "settings.startKeys.errors.last",
  });
  assert.deepEqual(keyRefusal("start-key-cannot-end:start"), {
    key: "settings.startKeys.errors.cannotEnd",
    ref: { kind: "start", id: "start" },
  });
  // Linux (2026-11-28): an instruction shortcut on neither list's keys.
  assert.deepEqual(
    keyRefusal("instruction-key-needs-start-or-hold:instruction_tab"),
    { key: "settings.instructionKeys.errors.needsStartOrHold" },
  );
  // An end shortcut's code is not taken for a start shortcut's, nor back.
  assert.equal(keyRefusal("end-key-last")?.key, "settings.endKeys.errors.last");
  assert.equal(keyRefusal("something else"), null);
});

check(
  "a conflict with a shortcut no layer has names its kind, never its id",
  () => {
    const none = () => null;
    assert.equal(
      shortcutRefusalText("conflict:hold_key:hold_1795300000009", t, none),
      "Hold shortcut already uses this shortcut",
    );
    assert.equal(
      shortcutRefusalText("conflict:start_key:start_1795300000009", tDe, none),
      "„Starttaste“ verwendet dieses Kürzel schon",
    );
  },
);

// The onboarding practises holding with the first hold shortcut and tapping
// with the first start shortcut; out of the box both are ⌃ Space.
check("the onboarding practises with the first start shortcut's keys", () => {
  assert.equal(practiceShortcut(STARTS, HOLDS), "ctrl+space");
  assert.equal(practiceShortcut([], HOLDS), "ctrl+space");
  assert.equal(practiceShortcut([], []), "");
  assert.equal(practiceShortcut(undefined, null), "");
});

check(
  "changing the practised keys changes the first start shortcut and its hold shortcut",
  () => {
    assert.deepEqual(practiceChange(STARTS, HOLDS, "ctrl+alt+d"), {
      start_keys: [start("start", "ctrl+alt+d")],
      hold_keys: [{ ...HOLDS[0], shortcut: "ctrl+alt+d" }, HOLDS[1]],
    });
    // A first hold shortcut on other keys stays as it is.
    const apart = [hold("hold_direct", "ctrl+alt+h"), HOLDS[1]];
    assert.deepEqual(practiceChange(STARTS, apart, "ctrl+alt+d"), {
      start_keys: [start("start", "ctrl+alt+d")],
      hold_keys: null,
    });
    // Without a start shortcut, the first hold shortcut.
    assert.deepEqual(practiceChange([], HOLDS, "ctrl+alt+d"), {
      start_keys: null,
      hold_keys: [{ ...HOLDS[0], shortcut: "ctrl+alt+d" }, HOLDS[1]],
    });
    assert.deepEqual(practiceChange([], [], "ctrl+alt+d"), {
      start_keys: null,
      hold_keys: null,
    });
  },
);

/** The texts of the cards Start Shortcuts and Hold Shortcuts (qa
 * 2026-11-26_1713, table "Texte"). */
const TEXTS: Array<[string, string, string]> = [
  ["settings.general.startShortcuts", "Start Shortcuts", "Starttasten"],
  [
    "settings.startKeys.description",
    "Tap to start a recording. It runs until an end shortcut ends it.",
    "Antippen startet eine Aufnahme. Sie läuft, bis eine Endtaste sie beendet.",
  ],
  ["settings.startKeys.add", "Add Start Shortcut", "Starttaste hinzufügen"],
  [
    "settings.startKeys.addPressKeys",
    "Press the combination for the new start shortcut",
    "Drück die Tastenkombination für die neue Starttaste",
  ],
  ["settings.startKeys.alsoHold", "Also a hold shortcut", "Auch Haltetaste"],
  [
    "settings.startKeys.lastOne",
    "One start or hold shortcut must stay, so that a recording can always be started.",
    "Eine Start- oder Haltetaste muss bleiben, damit sich eine Aufnahme immer starten lässt.",
  ],
  ["settings.holdKeys.title", "Hold Shortcuts", "Haltetasten"],
  [
    "settings.holdKeys.description",
    "Hold to record. Releasing ends the recording this way: what is appended and whether earlier dictations go along as context.",
    "Gedrückt halten nimmt auf. Das Loslassen beendet die Aufnahme so: was angehängt wird und ob frühere Diktate als Kontext mitgehen.",
  ],
  ["settings.holdKeys.add", "Add Hold Shortcut", "Haltetaste hinzufügen"],
  [
    "settings.holdKeys.addPressKeys",
    "Press the combination for the new hold shortcut",
    "Drück die Tastenkombination für die neue Haltetaste",
  ],
  [
    "settings.holdKeys.alsoStart",
    "Also a start shortcut: a short tap starts a recording that keeps running.",
    "Auch Starttaste: kurzes Antippen startet eine Aufnahme, die weiterläuft.",
  ],
  [
    "settings.holdKeys.holdOnly",
    "Hold shortcut only: a short tap does nothing.",
    "Nur Haltetaste: kurzes Antippen tut nichts.",
  ],
  [
    "settings.holdKeys.alsoInstruction",
    "Also an instruction shortcut: during a recording it switches to the instruction.",
    "Auch Anweisungstaste: Während einer Aufnahme schaltet sie zur Anweisung.",
  ],
  [
    "settings.startKeys.alsoInstruction",
    "Also an instruction shortcut",
    "Auch Anweisungstaste",
  ],
  // The card of the instruction shortcuts (qa 2026-11-26_1841, decision;
  // 2026-11-26_1720, "Einstellungen auf ‚Allgemein‘").
  [
    "settings.instructionKeys.title",
    "Instruction Shortcuts",
    "Anweisungstasten",
  ],
  [
    "settings.instructionKeys.description",
    "During a recording, an instruction shortcut switches from the text to an instruction: what you say next isn't inserted but tells post-processing what to do with the text, such as “translate this into Polish”. Tap it, and an end shortcut sends the dictation off; hold it, and releasing it sends the dictation off as set here. Such a dictation is always post-processed.",
    "Während einer Aufnahme schaltet eine Anweisungstaste vom Text zur Anweisung: Was du danach sprichst, wird nicht eingefügt, sondern sagt der Nachbearbeitung, was sie mit dem Text tun soll, etwa „übersetze das ins Polnische“. Antippen: Eine Endtaste schickt das Diktat ab. Halten: Das Loslassen schickt es ab, so wie hier eingestellt. Nachbearbeitet wird dann immer.",
  ],
  [
    "settings.instructionKeys.onlyWhileRecording",
    "Taken only during a recording.",
    "Nur während einer Aufnahme belegt.",
  ],
  [
    "settings.instructionKeys.add",
    "Add Instruction Shortcut",
    "Anweisungstaste hinzufügen",
  ],
  [
    "settings.instructionKeys.addPressKeys",
    "Press the combination for the new instruction shortcut",
    "Drück die Tastenkombination für die neue Anweisungstaste",
  ],
];

check(
  "the texts of start, hold and instruction shortcuts, in English and German",
  () => {
    for (const [key, english, german] of TEXTS) {
      assert.equal(t(key), english, `en ${key}`);
      assert.equal(tDe(key), german, `de ${key}`);
    }
  },
);

check("the texts of the old model are gone from English and German", () => {
  const gone = [
    "settings.general.shortcutActivation",
    "settings.general.shortcut.bindings.transcribe",
    "settings.general.shortcut.bindings.transcribe_with_post_process",
    "settings.endKeys.holdEnds",
    "settings.endKeys.tapEnds",
    "settings.profileView.fields.holdEnds",
    "settings.holdKeys.notTaken",
  ];
  for (const key of gone) {
    assert.equal(lookup(en as Tree, key), undefined, `en ${key}`);
    assert.equal(lookup(de as Tree, key), undefined, `de ${key}`);
  }
  // What build.rs carries into the tray stays.
  assert.notEqual(
    lookup(en as Tree, "settings.endKeys.factoryNames"),
    undefined,
  );
});

let failed = 0;
for (const [name, body] of cases) {
  try {
    body();
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${name}: ${(error as Error).message.split("\n")[0]}`);
  }
}
if (failed > 0) {
  console.error(`startKeys: ${failed} of ${cases.length} checks failed`);
  process.exit(1);
}
console.log(`startKeys: all ${cases.length} checks passed`);
