import type {
  EndKey,
  HoldEnd,
  ShortcutActivation,
  ShortcutBinding,
} from "../bindings";

/**
 * DanyWhere: the end shortcuts (end_keys.rs, qa 2026-11-22_2134). A
 * recording ends with one of them; each has a key combination of its own
 * and says what is appended, whether the text is post-processed and whether
 * earlier dictations go along as context. Start shortcuts only start: one
 * ends a recording only through an end with its exact combination, and
 * when held, through its end for holding (`hold_ends`). These helpers keep
 * the editor a controlled component: it gets a list and gives back a
 * changed one.
 */

/** Writes a stored combination as the shortcut controls do
 * (`formatKeyCombination` with the system and the interface's language). */
export type FormatShortcut = (keys: string) => string;

/** The start shortcuts, each with an end for holding. */
export const START_BINDINGS = [
  "transcribe",
  "transcribe_with_post_process",
] as const;

/** The end for holding of a start shortcut out of the box. */
const factoryHoldEnd = (binding: string): HoldEnd => ({
  append_text: "",
  post_process: binding === "transcribe_with_post_process",
  send_context: false,
});

/** The name to show: its own, or else its combination, which it follows
 * (also the interface's language). */
export const endKeyName = (end: EndKey, formatShortcut: FormatShortcut) =>
  end.name.trim() !== "" ? end.name : formatShortcut(end.shortcut);

/** What to store for a typed name: nothing ("no own name") when it is
 * blank or just the combination's name, else the name. */
export const nameToStore = (
  typed: string,
  end: EndKey,
  formatShortcut: FormatShortcut,
): string => {
  const name = typed.trim();
  return name === "" || name === formatShortcut(end.shortcut).trim()
    ? ""
    : name;
};

/** A new end with combination `shortcut` for `existing`, with an id none
 * has and no own name. */
export const newEndKey = (
  existing: EndKey[],
  shortcut: string,
  now: number,
): EndKey => {
  const ids = new Set(existing.map((end) => end.id));
  let id = `end_${now}`;
  for (let n = 2; ids.has(id); n += 1) id = `end_${now}_${n}`;
  return {
    id,
    name: "",
    shortcut,
    append_text: "",
    post_process: false,
    send_context: false,
  };
};

/** `list` with entry `id` changed by `change`. */
export const updateEndKey = (
  list: EndKey[],
  id: string,
  change: Partial<EndKey>,
): EndKey[] => list.map((end) => (end.id === id ? { ...end, ...change } : end));

/** `list` without entry `id`. */
export const removeEndKey = (list: EndKey[], id: string): EndKey[] =>
  list.filter((end) => end.id !== id);

/** Whether an entry of `list` may go: one must stay, so that a recording
 * can always be ended. */
export const canRemove = (list: readonly EndKey[]): boolean => list.length > 1;

/** A combination as stored, compared without spacing and case. */
const sameKeys = (a: string, b: string): boolean => {
  const normal = (keys: string) =>
    keys
      .split("+")
      .map((part) => part.trim().toLowerCase())
      .join("+");
  return normal(a) === normal(b);
};

/** The start shortcuts (binding ids) whose combination is exactly the
 * end's: pressing one again during a recording ends this way. */
export const startsSharing = (
  end: EndKey,
  bindings: Partial<Record<string, ShortcutBinding>>,
): string[] => {
  if (end.shortcut.trim() === "") return [];
  return START_BINDINGS.filter((binding) => {
    const keys = bindings[binding]?.current_binding ?? "";
    return keys.trim() !== "" && sameKeys(keys, end.shortcut);
  });
};

/** The end for holding of `binding`: its entry, or the one out of the box
 * (`transcribe` without, `transcribe_with_post_process` with
 * post-processing). */
export const holdEndOf = (
  binding: string,
  holdEnds: Partial<Record<string, HoldEnd>>,
): HoldEnd => ({ ...factoryHoldEnd(binding), ...holdEnds[binding] });

/** Whether a start shortcut can be held with `activation`: unless it is
 * "Toggle" (nothing stored is Auto). */
export const holdPossible = (
  activation: ShortcutActivation | null | undefined,
): boolean => activation !== "toggle";

/** The translation key for a refusal of the backend, if it has one. */
export const endKeyErrorKey = (error: string): string | null => {
  if (error.includes("end-key-needs-shortcut:"))
    return "settings.endKeys.errors.needsShortcut";
  if (error.includes("end-key-shortcut-twice:"))
    return "settings.endKeys.errors.shortcutTwice";
  if (error.includes("end-key-last")) return "settings.endKeys.errors.last";
  if (error.includes("end-key-cannot-end"))
    return "settings.endKeys.errors.cannotEnd";
  return null;
};
