// DanyWhere: searching History (`history_search.rs`). The backend searches
// and pages; these helpers build its query, keep a shown search up to date
// and find what to highlight in an entry's text.
import type { HistoryEntry, HistorySearchQuery } from "@/bindings";
import type { WindowFilter } from "../dictation-context/dictationContext";

/** What the search fields and their toggles hold. */
export type SearchFields = {
  text: string;
  window: string;
  regex: boolean;
  case_sensitive: boolean;
  whole_word: boolean;
};

export const EMPTY_SEARCH: SearchFields = {
  text: "",
  window: "",
  regex: false,
  case_sensitive: false,
  whole_word: false,
};

/** How long typing has to pause before History searches. */
export const SEARCH_DELAY_MS = 250;

/** Whether the fields ask for anything: the toggles alone don't. */
export const isSearchActive = (fields: SearchFields): boolean =>
  fields.text.length > 0 || fields.window.length > 0;

/** The backend's query: the fields, within the "same window" filter if one
 * is set. */
export const searchQuery = (
  fields: SearchFields,
  windowFilter: WindowFilter | null,
): HistorySearchQuery => ({
  ...fields,
  same_window: windowFilter
    ? { app_id: windowFilter.app_id, title: windowFilter.title }
    : null,
});

/** The field an error of the search commands is about: its regular
 * expression doesn't compile. `null` for any other error. */
export const invalidField = (error: string): "text" | "window" | null =>
  error === "invalid_text_pattern"
    ? "text"
    : error === "invalid_window_pattern"
      ? "window"
      : null;

/** The shown results after `entry` was added or changed: one already shown
 * is replaced where it is, even if it no longer matches (it doesn't vanish
 * while the user works on it); another joins only if it `matches` and falls
 * among the loaded pages, so the next page still starts after the last one
 * shown. */
export const mergeSearchUpdate = (
  list: HistoryEntry[],
  entry: HistoryEntry,
  matches: boolean,
  hasMore: boolean,
): HistoryEntry[] => {
  if (list.some((e) => e.id === entry.id)) {
    return list.map((e) => (e.id === entry.id ? entry : e));
  }
  if (!matches) return list;
  const last = list[list.length - 1];
  if (hasMore && last !== undefined && entry.id < last.id) return list;
  // Newest first, by id, as the backend pages.
  return [...list, entry].sort((a, b) => b.id - a.id);
};

/** The search for the text, as a JavaScript regex with the backend's whole
 * word rule; `null` when there's no text or JavaScript can't read the
 * pattern (then nothing is highlighted, the search itself still works). */
export type TextMatcher = { regex: RegExp; wholeWord: boolean };

// Characters that mean something in a JavaScript regex with the `u` flag,
// which allows escaping no others.
const SYNTAX = /[\\^$.*+?()[\]{}|/]/g;

export const textMatcher = (fields: SearchFields): TextMatcher | null => {
  if (fields.text.length === 0) return null;
  const source = fields.regex
    ? fields.text
    : fields.text.replace(SYNTAX, "\\$&");
  try {
    return {
      regex: new RegExp(source, fields.case_sensitive ? "gu" : "giu"),
      wholeWord: fields.whole_word,
    };
  } catch {
    return null;
  }
};

// A letter, digit or underscore, as Rust's `char::is_alphanumeric` and `_`.
const WORD_CHAR = /^[\p{Alphabetic}\p{N}_]$/u;
const isWordChar = (c: string | undefined): boolean =>
  c !== undefined && WORD_CHAR.test(c);

/** The character (code point) that starts at `index`. */
const charAt = (text: string, index: number): string | undefined => {
  const code = text.codePointAt(index);
  return code === undefined ? undefined : String.fromCodePoint(code);
};

/** The character (code point) that ends at `index`. */
const charBefore = (text: string, index: number): string | undefined => {
  if (index <= 0) return undefined;
  const low = text.charCodeAt(index - 1);
  if (index >= 2 && low >= 0xdc00 && low <= 0xdfff) {
    const high = text.charCodeAt(index - 2);
    if (high >= 0xd800 && high <= 0xdbff) return text.slice(index - 2, index);
  }
  return text[index - 1];
};

/** The match `start..end` cuts no word in two (`cuts_no_word` in Rust). */
const cutsNoWord = (text: string, start: number, end: number): boolean => {
  const joined = (a: string | undefined, b: string | undefined) =>
    isWordChar(a) && isWordChar(b);
  return (
    !joined(charBefore(text, start), charAt(text, start)) &&
    !joined(charBefore(text, end), charAt(text, end))
  );
};

/** Where to look next after a match at `start` cut a word (`after_cut`). */
const afterCut = (text: string, start: number): number => {
  const first = charAt(text, start);
  if (first === undefined) return text.length + 1;
  if (!isWordChar(first)) return start + first.length;
  let at = start;
  for (let c = charAt(text, at); c !== undefined; c = charAt(text, at)) {
    if (!isWordChar(c)) return at;
    at += c.length;
  }
  return text.length;
};

/** Where `matcher` matches in `text` (UTF-16 offsets, start and end),
 * found as the backend finds them; empty matches are left out. */
export const matchRanges = (
  text: string,
  matcher: TextMatcher | null,
): [number, number][] => {
  if (!matcher) return [];
  const { regex, wholeWord } = matcher;
  const found: [number, number][] = [];
  let at = 0;
  while (at <= text.length) {
    regex.lastIndex = at;
    const match = regex.exec(text);
    if (!match) break;
    const start = match.index;
    const end = start + match[0].length;
    if (end > start && (!wholeWord || cutsNoWord(text, start, end))) {
      found.push([start, end]);
      at = end;
    } else if (wholeWord) {
      at = afterCut(text, start);
    } else {
      // An empty match: step over the character it sits before.
      at = start + (charAt(text, start)?.length ?? 1);
    }
  }
  return found;
};

/** `text` cut into the parts to show plain and highlighted. */
export const splitHighlights = (
  text: string,
  ranges: [number, number][],
): { text: string; mark: boolean }[] => {
  const parts: { text: string; mark: boolean }[] = [];
  let at = 0;
  for (const [start, end] of ranges) {
    if (start > at) parts.push({ text: text.slice(at, start), mark: false });
    parts.push({ text: text.slice(start, end), mark: true });
    at = end;
  }
  if (at < text.length || parts.length === 0) {
    parts.push({ text: text.slice(at), mark: false });
  }
  return parts;
};
