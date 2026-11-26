import type { EndKey, PostProcessMode } from "../bindings";

/**
 * DanyWhere: the end shortcuts (end_keys.rs, qa 2026-11-22_2134). A
 * recording ends with one of them; each has a key combination of its own
 * and says what is appended and whether earlier dictations go along as
 * context, which counts only when the dictation is post-processed. Hold and
 * instruction shortcuts have the same shape (qa 2026-11-26_1810; the start,
 * hold and instruction shortcuts' own helpers: `startKeys.ts`). Whether a
 * dictation is post-processed no key says: an instruction shortcut or the
 * profile's mode does ("Schnittstelle Phase 2"). These helpers keep the
 * editor a controlled component: it gets a list and gives back a changed
 * one.
 */

/** Writes a stored combination as the shortcut controls do
 * (`formatKeyCombination` with the system and the interface's language). */
export type FormatShortcut = (keys: string) => string;

/** The name to show: its own, or else its combination, which it follows
 * (also the interface's language). */
export const endKeyName = (end: EndKey, formatShortcut: FormatShortcut) =>
  end.name.trim() !== "" ? end.name : formatShortcut(end.shortcut);

/** What to store for a typed name: nothing ("no own name") when it is
 * blank or just the name it follows without one (`followed`: the
 * combination, or for a hold shortcut "<keys> held"), else the name. */
export const nameToStore = (typed: string, followed: string): string => {
  const name = typed.trim();
  return name === "" || name === followed.trim() ? "" : name;
};

/** A new entry with combination `shortcut` for `existing`, with an id none
 * has and no own name: an end shortcut (`end_<now>`) or, with `prefix`
 * "hold" or "instruction", a hold (`hold_<now>`) or instruction shortcut
 * (`instruction_<now>`). */
export const newEndKey = (
  existing: readonly { id: string }[],
  shortcut: string,
  now: number,
  prefix = "end",
): EndKey => {
  const ids = new Set(existing.map((end) => end.id));
  let id = `${prefix}_${now}`;
  for (let n = 2; ids.has(id); n += 1) id = `${prefix}_${now}_${n}`;
  return {
    id,
    name: "",
    shortcut,
    append_text: "",
    send_context: false,
  };
};

/** `list` with entry `id` changed by `change`. */
export const updateEndKey = <T extends { id: string }>(
  list: readonly T[],
  id: string,
  change: Partial<T>,
): T[] => list.map((end) => (end.id === id ? { ...end, ...change } : end));

/** `list` without entry `id`. */
export const removeEndKey = <T extends { id: string }>(
  list: readonly T[],
  id: string,
): T[] => list.filter((end) => end.id !== id);

/** Whether an entry of `list` may go: one must stay, so that a recording
 * can always be ended. */
export const canRemove = (list: readonly EndKey[]): boolean => list.length > 1;

/** Whether two stored combinations are the same, compared without spacing
 * and case; a side (`ctrl_left`) is another combination. */
export const sameKeys = (a: string, b: string): boolean => {
  const normal = (keys: string) =>
    keys
      .split("+")
      .map((part) => part.trim().toLowerCase())
      .join("+");
  return normal(a) === normal(b);
};

/** The translation key for a refusal of the end shortcuts' own codes, if
 * it is one. */
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

/** What [`postProcessingOn`] reads: a profile's values, the defaults' or
 * the flat values in use. */
export interface PostProcessSwitch {
  post_process_mode?: PostProcessMode | null;
}

/** Whether a dictation with `values` may post-process: their mode isn't
 * "never" (qa 2026-11-26_1810, "Schnittstelle Phase 2"). With
 * "with_instruction_key" a dictation is post-processed once an instruction
 * shortcut was pressed in it, with "always" every one. A mode not known
 * counts as "never", so nothing goes out. */
export const postProcessingOn = (values: PostProcessSwitch): boolean =>
  values.post_process_mode === "with_instruction_key" ||
  values.post_process_mode === "always";
