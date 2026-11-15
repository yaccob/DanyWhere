import type {
  AppendKeyNames,
  AppendPart,
  AppendTextError,
  KeyPlatform,
} from "../bindings";

/**
 * DanyWhere: the words for the "Append" field (append_text.rs). Rust parses
 * the field and names its keys as this system writes them, the one source
 * of truth for its syntax; this only puts the parts it returns, and its
 * errors, into words.
 */

type Translate = (key: string, options?: Record<string, unknown>) => string;

const PREFIX = "settings.advanced.appendText";

const partWords = (part: AppendPart, t: Translate): string => {
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
      return t(`${PREFIX}.preview.key`, { key: part.name });
  }
};

/**
 * The parts in words, in their order, e.g. "Space, then the Enter key";
 * "Nothing is appended" for none. `locale` capitalizes the first letter.
 */
export const describeAppend = (
  parts: AppendPart[],
  t: Translate,
  locale?: string,
): string => {
  if (parts.length === 0) return t(`${PREFIX}.preview.nothing`);
  const sentence = parts
    .map((part) => partWords(part, t))
    .reduce((first, next) => t(`${PREFIX}.preview.then`, { first, next }));
  return sentence.charAt(0).toLocaleUpperCase(locale) + sentence.slice(1);
};

/** The field's description, with this system's name of the Cmd key press. */
export const appendDescription = (t: Translate, names: AppendKeyNames) =>
  t(`${PREFIX}.description`, { cmdEnter: `{${names.cmd_enter}}` });

/** Why a value is invalid, in words; `platform` is the one Rust parsed for. */
export const appendErrorMessage = (
  error: AppendTextError,
  t: Translate,
  platform: KeyPlatform,
): string => {
  switch (error.code) {
    case "unknown_escape":
      return t(`${PREFIX}.errors.unknownEscape`, { escape: error.detail });
    case "trailing_backslash":
      return t(`${PREFIX}.errors.trailingBackslash`);
    case "unknown_key":
      return t(`${PREFIX}.errors.unknownKey`, { key: `{${error.detail}}` });
    case "unclosed_key":
      return t(`${PREFIX}.errors.unclosedKey`);
    case "unmatched_brace":
      return t(`${PREFIX}.errors.unmatchedBrace`);
    case "text_after_key":
      return t(`${PREFIX}.errors.textAfterKey`);
    case "foreign_key_name":
      return t(`${PREFIX}.errors.foreignKeyName.${platform}`, {
        name: error.detail,
        key: `{${error.suggestion}}`,
      });
  }
};
