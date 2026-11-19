import type { EndKey } from "../bindings";
import type { DropdownOption } from "../components/ui/Dropdown";

/**
 * DanyWhere: the end shortcuts (end_keys.rs). A recording ends with one of
 * them; each says what is appended, whether the text is post-processed and
 * whether earlier dictations go along as context. Every start shortcut has
 * a default end from the list. These helpers keep the editor a controlled
 * component: it gets a list and gives back a changed one.
 */

type Translate = (key: string, options?: Record<string, unknown>) => string;

/** The end shortcut without post-processing every profile starts with. */
export const PLAIN_ID = "plain";
/** The end shortcut with post-processing every profile starts with. */
export const POST_PROCESS_ID = "post_process";
/** The start shortcuts, each with a default end. */
export const START_BINDINGS = [
  "transcribe",
  "transcribe_with_post_process",
] as const;

const BUILTIN_DEFAULT: Record<string, string> = {
  transcribe: PLAIN_ID,
  transcribe_with_post_process: POST_PROCESS_ID,
};

const isBuiltin = (id: string) => id === PLAIN_ID || id === POST_PROCESS_ID;

/** The name to show: its own, or the built-in one's translated name. */
export const endKeyName = (end: EndKey, t: Translate): string => {
  if (end.name.trim() !== "" || !isBuiltin(end.id)) return end.name;
  return t(`settings.endKeys.builtin.${end.id}`);
};

/** A new end shortcut for `existing`, with an id and a name none has. */
export const newEndKey = (
  existing: EndKey[],
  baseName: string,
  now: number,
): EndKey => {
  const ids = new Set(existing.map((end) => end.id));
  let id = `end_${now}`;
  for (let n = 2; ids.has(id); n += 1) id = `end_${now}_${n}`;
  const names = new Set(existing.map((end) => end.name.trim()));
  let name = baseName;
  for (let n = 2; names.has(name); n += 1) name = `${baseName} ${n}`;
  return {
    id,
    name,
    shortcut: "",
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

/** The default end of `binding`: its entry, or the built-in one. */
export const defaultEndOf = (
  binding: string,
  defaultEnds: Partial<Record<string, string>>,
): string => defaultEnds[binding] ?? BUILTIN_DEFAULT[binding] ?? PLAIN_ID;

/** The start shortcuts whose default end is `id` (it can't go then). */
export const startsEndingWith = (
  id: string,
  defaultEnds: Partial<Record<string, string>>,
): string[] =>
  START_BINDINGS.filter((binding) => defaultEndOf(binding, defaultEnds) === id);

/** The entries of a default-end selection. */
export const endKeyOptions = (list: EndKey[], t: Translate): DropdownOption[] =>
  list.map((end) => ({ value: end.id, label: endKeyName(end, t) }));

/** The translation key for a refusal of the backend, if it has one. */
export const endKeyErrorKey = (error: string): string | null => {
  if (error.includes("end-key-needs-name:"))
    return "settings.endKeys.errors.needsName";
  if (error.includes("end-key-shortcut-twice:"))
    return "settings.endKeys.errors.shortcutTwice";
  if (error.includes("end-key-in-use:")) return "settings.endKeys.inUse";
  return null;
};
