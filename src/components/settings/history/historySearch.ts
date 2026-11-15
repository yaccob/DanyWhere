// DanyWhere: searching History (`history_search.rs`). The backend searches
// and pages; these helpers build its query, keep a shown search up to date,
// choose which of an entry's texts it shows, and find what to highlight in
// that text, in the window line and in a file's name.
import type {
  EntryWindowContext,
  HistoryEntry,
  HistorySearchQuery,
} from "@/bindings";
import {
  windowLabel,
  type WindowFilter,
} from "../dictation-context/dictationContext";
import { isCorrected, type CorrectionPart } from "./corrections";
import { postProcessedText, shownText } from "./postProcessEntry";

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

/** `needle` as a matcher: as typed with `regex`, else literally. */
const compileMatcher = (
  needle: string,
  regex: boolean,
  ignoreCase: boolean,
  wholeWord: boolean,
): TextMatcher | null => {
  if (needle.length === 0) return null;
  const source = regex ? needle : needle.replace(SYNTAX, "\\$&");
  try {
    return {
      regex: new RegExp(source, ignoreCase ? "giu" : "gu"),
      wholeWord,
    };
  } catch {
    return null;
  }
};

export const textMatcher = (fields: SearchFields): TextMatcher | null =>
  compileMatcher(
    fields.text,
    fields.regex,
    !fields.case_sensitive,
    fields.whole_word,
  );

/** The search for the window field, as `textMatcher` for the text: always
 * ignoring case and never by whole words, as the backend's window field. */
export const windowMatcher = (fields: SearchFields): TextMatcher | null =>
  compileMatcher(fields.window, fields.regex, true, false);

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

// What `windowLabel` puts between the app and the title.
const LINE_SEPARATOR = " – ";

/** `ranges` sorted, with overlapping ones joined. */
const joinRanges = (ranges: [number, number][]): [number, number][] => {
  const joined: [number, number][] = [];
  for (const [start, end] of [...ranges].sort((a, b) => a[0] - b[0])) {
    const last = joined[joined.length - 1];
    if (last && start <= last[1]) {
      last[1] = Math.max(last[1], end);
    } else {
      joined.push([start, end]);
    }
  }
  return joined;
};

/** Where `matcher` matches in the window line under an entry ("App –
 * Title", `windowLabel`): as the backend matches the window, in the app, in
 * the title and in the whole line, so `^Inbox` marks the title and `^Mail$`
 * the app. Empty when the entry shows no window line. */
export const windowLineRanges = (
  context: EntryWindowContext | null | undefined,
  matcher: TextMatcher | null,
): [number, number][] => {
  const label = windowLabel(context);
  const title = context?.window_title?.trim();
  if (!matcher || !label || !title) return [];
  const titleStart = label.length - title.length;
  const ranges: [number, number][] = matchRanges(title, matcher).map(
    ([start, end]) => [start + titleStart, end + titleStart],
  );
  if (titleStart > 0) {
    // "App – Title": the app ends before the separator.
    const app = label.slice(0, titleStart - LINE_SEPARATOR.length);
    ranges.push(...matchRanges(app, matcher), ...matchRanges(label, matcher));
  }
  return joinRanges(ranges);
};

type ShownTextFields = Parameters<typeof shownText>[0];

/** Whether an entry shows its original during a search: as the user chose
 * (`showOriginal`), unless that text has no match and the other one has;
 * then it shows the other, as if switched by hand. With matches in both it
 * keeps the user's choice. Without a text to search, with a pattern
 * JavaScript can't read (`matcher` null), or once the user switched it by
 * hand during this search (`pinned`), it shows what the user chose. */
export const showsOriginal = (
  entry: ShownTextFields,
  matcher: TextMatcher | null,
  showOriginal: boolean,
  pinned = false,
): boolean => {
  if (pinned || !matcher || postProcessedText(entry) === null) {
    return showOriginal;
  }
  const matches = (original: boolean) =>
    matchRanges(shownText(entry, original), matcher).length > 0;
  if (matches(showOriginal) || !matches(!showOriginal)) return showOriginal;
  return !showOriginal;
};

/** Whether the search matches the entry only in a text the user's
 * correction replaced (the backend searches those too), so neither view
 * shows the match and the entry says why it was found instead. `false`
 * without a text to search or with a pattern JavaScript can't read. */
export const matchesOnlyBeforeCorrection = (
  entry: ShownTextFields,
  matcher: TextMatcher | null,
): boolean => {
  if (!matcher) return false;
  const matches = (text: string | null) =>
    text !== null && matchRanges(text, matcher).length > 0;
  if (matches(shownText(entry, false)) || matches(shownText(entry, true))) {
    return false;
  }
  const replaced = (part: CorrectionPart) =>
    isCorrected(entry, part)
      ? part === "transcription"
        ? entry.transcription_text
        : postProcessedText(entry)
      : null;
  return (
    matches(replaced("transcription")) || matches(replaced("post_processing"))
  );
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
