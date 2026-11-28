import type { EndKey, StartKey } from "../bindings";
import {
  endKeyErrorKey,
  endKeyName,
  sameKeys,
  type FormatShortcut,
} from "./endKeys";

/**
 * DanyWhere: start and hold shortcuts, two lists of their own beside the
 * end shortcuts (sidecar qa 2026-11-26_1713, decision; 2026-11-26_1810,
 * "Schnittstelle Phase 1"). A start shortcut is only a combination: tapping
 * it starts a recording that runs until an end shortcut ends it. A hold
 * shortcut has an end shortcut's shape: pressing it starts, releasing it
 * ends the recording with its own values. The same combination may stand in
 * several lists; what it does follows from the lists it stands in. An
 * instruction shortcut is a fourth list of that shape (qa 2026-11-26_1810,
 * "Schnittstelle Phase 2"): during a recording it switches to a spoken
 * instruction, and without a name of its own it is called "⇥ held", since
 * its name stands for the end when it is released.
 */

export type { StartKey };

/** "<keys> held" in the interface's language (`settings.endKeys.held`). */
export type Held = (shortcut: string) => string;

/** A new start shortcut with combination `shortcut`, with an id none of
 * `existing` has. */
export const newStartKey = (
  existing: readonly { id: string }[],
  shortcut: string,
  now: number,
): StartKey => {
  const ids = new Set(existing.map((start) => start.id));
  let id = `start_${now}`;
  for (let n = 2; ids.has(id); n += 1) id = `start_${now}_${n}`;
  return { id, shortcut };
};

/** The name a hold shortcut follows without one of its own: "⌃ Space
 * held", in the interface's language. */
export const holdKeyDefaultName = (
  hold: EndKey,
  format: FormatShortcut,
  held: Held,
): string => held(format(hold.shortcut));

/** The name to show for a hold shortcut: its own, or else the one it
 * follows ([`holdKeyDefaultName`]). */
export const holdKeyName = (
  hold: EndKey,
  format: FormatShortcut,
  held: Held,
): string =>
  hold.name.trim() !== "" ? hold.name : holdKeyDefaultName(hold, format, held);

/** Whether an entry of the start or hold shortcuts may go: every layer
 * keeps one of them, so that a recording can always be started (the
 * backend refuses with `start-key-last`). */
export const canRemoveStartOrHold = (
  starts: readonly unknown[],
  holds: readonly unknown[],
): boolean => starts.length + holds.length > 1;

/** Whether an entry of `list` has exactly the combination `shortcut`; a
 * side (`ctrl_left`) or a covering combination is another one. */
export const hasCombination = (
  list: readonly { shortcut: string }[],
  shortcut: string,
): boolean =>
  shortcut.trim() !== "" &&
  list.some(
    (entry) =>
      entry.shortcut.trim() !== "" && sameKeys(entry.shortcut, shortcut),
  );

/** Which list a reference of the backend points into. */
export type KeyKind = "start" | "hold" | "instruction" | "end";

/** A start, hold, instruction or end shortcut as the backend names it in a
 * refusal: `start_key:<id>`, `hold_key:<id>`, `instruction_key:<id>`,
 * `end_key:<id>`. */
export interface KeyRef {
  kind: KeyKind;
  id: string;
}

const KEY_REF = /^(start|hold|instruction|end)_key:([A-Za-z0-9_.-]+)$/;

/** The reference `ref` stands for, or null for any other binding id. */
export const parseKeyRef = (ref: string): KeyRef | null => {
  const match = KEY_REF.exec(ref);
  return match ? { kind: match[1] as KeyKind, id: match[2] } : null;
};

type Values = Readonly<Record<string, unknown>>;

const LIST_OF: Record<KeyKind, string> = {
  start: "start_keys",
  hold: "hold_keys",
  instruction: "instruction_keys",
  end: "end_keys",
};

/** What a reference is called: a start shortcut by its keys, a hold,
 * instruction or end shortcut by its name ("<keys> held" for the first two
 * without one of their own). It is looked up in `sources` in turn (the
 * values in use, the defaults', each profile's); null when none has it. */
export const keyRefLabel = (
  ref: KeyRef,
  sources: readonly (Values | null | undefined)[],
  format: FormatShortcut,
  held: Held,
): string | null => {
  for (const values of sources) {
    const list = values?.[LIST_OF[ref.kind]];
    if (!Array.isArray(list)) continue;
    const entry = list.find(
      (item): item is Record<string, unknown> =>
        typeof item === "object" &&
        item !== null &&
        (item as { id?: unknown }).id === ref.id,
    );
    if (!entry || typeof entry.shortcut !== "string") continue;
    if (ref.kind === "start") return format(entry.shortcut);
    const shaped = { name: "", ...entry } as unknown as EndKey;
    return ref.kind === "end"
      ? endKeyName(shaped, format)
      : holdKeyName(shaped, format, held);
  }
  return null;
};

/** A refusal of the backend for start, hold, instruction or end
 * shortcuts: the translation key of its sentence and, where the sentence
 * names a shortcut, which one. */
export interface KeyRefusal {
  key: string;
  ref?: KeyRef;
}

/** The refusal `error` is, if it is one of the codes of the start, hold,
 * instruction and end shortcuts (qa 2026-11-26_1810, "Prüfung und
 * Garantien" and "Schnittstelle Phase 2"). */
export const keyRefusal = (error: string): KeyRefusal | null => {
  const cannotEnd = /start-key-cannot-end:([A-Za-z0-9_.-]+)/.exec(error);
  if (cannotEnd) {
    return {
      key: "settings.startKeys.errors.cannotEnd",
      ref: { kind: "start", id: cannotEnd[1] },
    };
  }
  const codes: Array<[RegExp, string]> = [
    [/start-key-needs-shortcut:/, "settings.startKeys.errors.needsShortcut"],
    [/hold-key-needs-shortcut:/, "settings.holdKeys.errors.needsShortcut"],
    [
      /instruction-key-needs-shortcut:/,
      "settings.instructionKeys.errors.needsShortcut",
    ],
    [
      /instruction-key-needs-start-or-hold:/,
      "settings.instructionKeys.errors.needsStartOrHold",
    ],
    [/start-key-shortcut-twice:/, "settings.startKeys.errors.shortcutTwice"],
    [/hold-key-shortcut-twice:/, "settings.holdKeys.errors.shortcutTwice"],
    [
      /instruction-key-shortcut-twice:/,
      "settings.instructionKeys.errors.shortcutTwice",
    ],
    [/start-key-last/, "settings.startKeys.errors.last"],
  ];
  for (const [code, key] of codes) {
    if (code.test(error)) return { key };
  }
  const end = endKeyErrorKey(error);
  return end ? { key: end } : null;
};

/** The keys the onboarding practises with: those of the first start
 * shortcut (tapping), else of the first hold shortcut (holding); out of
 * the box both are ⌃ Space. */
export const practiceShortcut = (
  starts: readonly StartKey[] | null | undefined,
  holds: readonly EndKey[] | null | undefined,
): string => starts?.[0]?.shortcut ?? holds?.[0]?.shortcut ?? "";

/** The lists after the onboarding changed the practised keys to
 * `shortcut`: the first start shortcut takes them, and so does the first
 * hold shortcut if it had the same keys (it practises holding); without a
 * start shortcut, the first hold shortcut. `null` for a list that stays. */
export const practiceChange = (
  starts: readonly StartKey[] | null | undefined,
  holds: readonly EndKey[] | null | undefined,
  shortcut: string,
): { start_keys: StartKey[] | null; hold_keys: EndKey[] | null } => {
  const start = starts?.[0];
  const hold = holds?.[0];
  const withFirst = <T extends { shortcut: string }>(list: readonly T[]) =>
    list.map((entry, index) => (index === 0 ? { ...entry, shortcut } : entry));
  const holdFollows =
    hold !== undefined &&
    (start === undefined || sameKeys(hold.shortcut, start.shortcut));
  return {
    start_keys: start && starts ? withFirst(starts) : null,
    hold_keys: holdFollows && holds ? withFirst(holds) : null,
  };
};
