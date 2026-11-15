import type { AppendKey, AppendPart, AppendTextError } from "../bindings";
import type { OSType } from "./utils/keyboard";

/**
 * DanyWhere: the words for the "Append" field (append_text.rs). Rust parses
 * the field, the one source of truth for its syntax; this only names the
 * parts it returns, and its errors.
 */

type Translate = (key: string, options?: Record<string, unknown>) => string;

const PREFIX = "settings.advanced.appendText";

/** A key press as the field writes it; Cmd is Super off macOS. */
export const appendKeyLabel = (key: AppendKey, osType: OSType): string => {
  switch (key) {
    case "enter":
      return "Enter";
    case "tab":
      return "Tab";
    case "shift_enter":
      return "Shift+Enter";
    case "cmd_enter":
      return osType === "macos" ? "Cmd+Enter" : "Super+Enter";
    case "ctrl_enter":
      return "Ctrl+Enter";
  }
};

const partWords = (part: AppendPart, t: Translate, osType: OSType): string => {
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
        key: appendKeyLabel(part.key, osType),
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
  osType: OSType,
  locale?: string,
): string => {
  if (parts.length === 0) return t(`${PREFIX}.preview.nothing`);
  const sentence = parts
    .map((part) => partWords(part, t, osType))
    .reduce((first, next) => t(`${PREFIX}.preview.then`, { first, next }));
  return sentence.charAt(0).toLocaleUpperCase(locale) + sentence.slice(1);
};

/** Why a value is invalid, in words. */
export const appendErrorMessage = (
  error: AppendTextError,
  t: Translate,
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
  }
};
