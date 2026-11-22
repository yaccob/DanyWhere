import type { AppendChoice, AppendKey, AppendPart } from "../bindings";
import type { DropdownOption } from "../components/ui/Dropdown";
import { keycapLabel, type OSType } from "./utils/keyboard";

/**
 * DanyWhere: the words and the key selection of the "Append" field
 * (append_text.rs). Rust splits the stored value into text and key presses,
 * composes it again and says which key presses this system offers; this
 * puts the parts into words and the key presses into the app's notation.
 */

type Translate = (key: string, options?: Record<string, unknown>) => string;

const PREFIX = "settings.advanced.appendText";

/** The selection's entry for no key press. */
export const NO_KEY = "none";
/** The selection's entry for stored key presses it doesn't offer. */
export const KEPT_KEYS = "kept";

/** Each key press as a shortcut binding, for the app's keycap labels. */
const BINDINGS: Record<AppendKey, string> = {
  enter: "enter",
  tab: "tab",
  shift_enter: "shift+enter",
  cmd_enter: "super+enter",
  ctrl_enter: "ctrl+enter",
};

/**
 * A key press as the app shows shortcuts on `os`: "⌘↩" on the Mac,
 * "Strg+Eingabe" elsewhere in German.
 */
export const appendKeyLabel = (
  key: AppendKey,
  os: OSType,
  t: Translate,
): string => keycapLabel(BINDINGS[key], os, t);

/** Several key presses in their order: "Tab, then Enter". */
const keysLabel = (keys: AppendKey[], t: Translate, os: OSType): string =>
  keys
    .map((key) => appendKeyLabel(key, os, t))
    .reduce((first, next) => t(`${PREFIX}.preview.then`, { first, next }));

const partWords = (part: AppendPart, t: Translate, os: OSType): string => {
  switch (part.kind) {
    case "text":
      return t(`${PREFIX}.preview.text`, { text: part.text });
    case "space":
      return t(`${PREFIX}.preview.space`);
    case "new_line":
      return t(`${PREFIX}.preview.newLine`);
    case "tab":
      return t(`${PREFIX}.preview.tab`);
    case "key":
      return t(`${PREFIX}.preview.key`, {
        key: appendKeyLabel(part.key, os, t),
      });
  }
};

/**
 * The parts in words, in their order, e.g. "Space, then the Enter key";
 * "Nothing is appended" for none. `locale` capitalizes the first letter.
 */
export const describeAppend = (
  parts: AppendPart[],
  t: Translate,
  os: OSType,
  locale?: string,
): string => {
  if (parts.length === 0) return t(`${PREFIX}.preview.nothing`);
  const sentence = parts
    .map((part) => partWords(part, t, os))
    .reduce((first, next) => t(`${PREFIX}.preview.then`, { first, next }));
  return sentence.charAt(0).toLocaleUpperCase(locale) + sentence.slice(1);
};

/**
 * The selection's entries: no key press, then the ones `offered` here, and
 * for stored key presses it can't show (`choice.kept`) an extra entry at
 * the end, which keeps them until another is chosen.
 */
export const keyOptions = (
  offered: AppendKey[],
  choice: AppendChoice,
  t: Translate,
  os: OSType,
): DropdownOption[] => {
  const options: DropdownOption[] = [
    { value: NO_KEY, label: t(`${PREFIX}.keyNone`) },
    ...offered.map((key) => ({
      value: key,
      label: appendKeyLabel(key, os, t),
    })),
  ];
  if (choice.kept) {
    options.push({
      value: KEPT_KEYS,
      label: keysLabel(choice.keys, t, os),
      description: t(`${PREFIX}.keyKept`),
    });
  }
  return options;
};

/** The entry that shows the stored key presses. */
export const selectedKeyOption = (choice: AppendChoice): string => {
  if (choice.keys.length === 0) return NO_KEY;
  return choice.kept ? KEPT_KEYS : choice.keys[0];
};

/** The key presses an entry stands for. */
export const keysOfOption = (
  value: string,
  choice: AppendChoice,
): AppendKey[] => {
  if (value === NO_KEY) return [];
  if (value === KEPT_KEYS) return choice.keys;
  return [value as AppendKey];
};
