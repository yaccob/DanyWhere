import assert from "node:assert/strict";
import type { EntryWindowContext, HistoryEntry } from "@/bindings";
import {
  EMPTY_SEARCH,
  invalidField,
  isSearchActive,
  matchesOnlyBeforeCorrection,
  matchRanges,
  mergeSearchUpdate,
  searchQuery,
  showsOriginal,
  splitHighlights,
  textMatcher,
  windowLineRanges,
  windowMatcher,
  type SearchFields,
} from "./historySearch";

let failures = 0;
const check = (name: string, run: () => void) => {
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

const fields = (changes: Partial<SearchFields>): SearchFields => ({
  ...EMPTY_SEARCH,
  ...changes,
});

const entry = (id: number, text = `entry ${id}`): HistoryEntry =>
  ({ id, transcription_text: text }) as HistoryEntry;

const ids = (list: HistoryEntry[]) => list.map((e) => e.id);

const ranges = (text: string, search: Partial<SearchFields>) =>
  matchRanges(text, textMatcher(fields(search)));

// ---- The query ----------------------------------------------------------

check("empty fields search nothing, whatever the toggles", () => {
  assert.equal(isSearchActive(EMPTY_SEARCH), false);
  assert.equal(
    isSearchActive(
      fields({ regex: true, case_sensitive: true, whole_word: true }),
    ),
    false,
  );
});

check("either field searches", () => {
  assert.equal(isSearchActive(fields({ text: "a" })), true);
  assert.equal(isSearchActive(fields({ window: "Safari" })), true);
});

check("the query carries the same-window filter when one is set", () => {
  const search = fields({ text: "build", whole_word: true });
  assert.deepEqual(searchQuery(search, null), {
    ...search,
    same_window: null,
  });
  assert.deepEqual(
    searchQuery(search, { app_id: "com.x", title: "CI", label: "X – CI" }),
    { ...search, same_window: { app_id: "com.x", title: "CI" } },
  );
});

check("an invalid pattern names its field", () => {
  assert.equal(invalidField("invalid_text_pattern"), "text");
  assert.equal(invalidField("invalid_window_pattern"), "window");
  assert.equal(invalidField("database is locked"), null);
});

// ---- Live updates -------------------------------------------------------

check("a shown entry is replaced where it is, matching or not", () => {
  const list = [entry(9), entry(5), entry(3)];
  const changed = entry(5, "corrected away");
  const merged = mergeSearchUpdate(list, changed, false, true);
  assert.deepEqual(ids(merged), [9, 5, 3]);
  assert.equal(merged[1].transcription_text, "corrected away");
});

check("a new matching entry joins at the top", () => {
  const list = [entry(9), entry(5)];
  assert.deepEqual(
    ids(mergeSearchUpdate(list, entry(12), true, true)),
    [12, 9, 5],
  );
});

check("an entry that doesn't match stays out", () => {
  const list = [entry(9), entry(5)];
  assert.deepEqual(
    ids(mergeSearchUpdate(list, entry(12), false, false)),
    [9, 5],
  );
});

check("a matching entry among the loaded pages joins in order", () => {
  const list = [entry(9), entry(5), entry(3)];
  assert.deepEqual(
    ids(mergeSearchUpdate(list, entry(7), true, true)),
    [9, 7, 5, 3],
  );
});

check("a matching entry past the loaded pages waits for its page", () => {
  const list = [entry(9), entry(5)];
  assert.deepEqual(ids(mergeSearchUpdate(list, entry(2), true, true)), [9, 5]);
  // Without more pages it belongs at the end.
  assert.deepEqual(
    ids(mergeSearchUpdate(list, entry(2), true, false)),
    [9, 5, 2],
  );
});

// ---- Highlighting -------------------------------------------------------

check("nothing to highlight without text", () => {
  assert.equal(textMatcher(EMPTY_SEARCH), null);
  assert.deepEqual(ranges("anything", {}), []);
});

check("a substring is highlighted wherever it is, ignoring case", () => {
  assert.deepEqual(ranges("Build, build, BUILD", { text: "build" }), [
    [0, 5],
    [7, 12],
    [14, 19],
  ]);
  assert.deepEqual(ranges("ÜBER und über", { text: "über" }), [
    [0, 4],
    [9, 13],
  ]);
});

check("matching case highlights only the same case", () => {
  assert.deepEqual(
    ranges("Build, build", { text: "Build", case_sensitive: true }),
    [[0, 5]],
  );
});

check("without regex the text is literal", () => {
  assert.deepEqual(ranges("abc a.c (1)", { text: "a.c" }), [[4, 7]]);
  assert.deepEqual(ranges("abc a.c (1)", { text: "(1)" }), [[8, 11]]);
});

check("whole words are highlighted as the backend finds them", () => {
  assert.deepEqual(ranges("Start art.", { text: "art", whole_word: true }), [
    [6, 9],
  ]);
  assert.deepEqual(ranges("für fürs", { text: "für", whole_word: true }), [
    [0, 3],
  ]);
  assert.deepEqual(ranges("see foo.bar", { text: "foo.", whole_word: true }), [
    [4, 8],
  ]);
  assert.deepEqual(ranges("foo.", { text: "fo", whole_word: true }), []);
});

check("a regex is highlighted where it matches", () => {
  assert.deepEqual(ranges("Zimmer 123 und 45", { text: "\\d+", regex: true }), [
    [7, 10],
    [15, 17],
  ]);
  assert.deepEqual(
    ranges("abc123 abc 123", { text: "\\d+", regex: true, whole_word: true }),
    [[11, 14]],
  );
});

check("empty matches are not highlighted", () => {
  assert.deepEqual(ranges("a xx b", { text: "x*", regex: true }), [[2, 4]]);
});

check("a pattern JavaScript can't read highlights nothing", () => {
  assert.equal(textMatcher(fields({ text: "(unclosed", regex: true })), null);
  // Rust's inline flags aren't JavaScript's.
  assert.equal(textMatcher(fields({ text: "(?i)abc", regex: true })), null);
});

check("the text is cut into plain and highlighted parts", () => {
  assert.deepEqual(
    splitHighlights("one two three", [
      [4, 7],
      [8, 13],
    ]),
    [
      { text: "one ", mark: false },
      { text: "two", mark: true },
      { text: " ", mark: false },
      { text: "three", mark: true },
    ],
  );
  assert.deepEqual(splitHighlights("plain", []), [
    { text: "plain", mark: false },
  ]);
});

// ---- Which text an entry shows during a search --------------------------

const processed = (changes: Partial<HistoryEntry> = {}): HistoryEntry =>
  ({
    id: 1,
    transcription_text: "the raw words",
    post_processed_text: "The polished text.",
    transcription_correction: null,
    post_processed_correction: null,
    ...changes,
  }) as HistoryEntry;

const shows = (
  shown: HistoryEntry,
  search: Partial<SearchFields>,
  chosenOriginal = false,
  pinned = false,
) => showsOriginal(shown, textMatcher(fields(search)), chosenOriginal, pinned);

check("a match only in the original shows the original", () => {
  assert.equal(shows(processed(), { text: "raw" }), true);
});

check("a match in the post-processed text keeps it shown", () => {
  assert.equal(shows(processed(), { text: "polished" }), false);
});

check("a match in both texts keeps the normal display", () => {
  const both = processed({
    transcription_text: "the words",
    post_processed_text: "The words.",
  });
  assert.equal(shows(both, { text: "words" }), false);
});

check("without a search the entry shows what the user chose", () => {
  assert.equal(showsOriginal(processed(), null, false), false);
  assert.equal(showsOriginal(processed(), null, true), true);
  // A search of the window alone has no text to match.
  assert.equal(shows(processed(), { window: "safari" }), false);
});

check("a match only in the other text shows it, whichever was chosen", () => {
  assert.equal(shows(processed(), { text: "polished" }, true), false);
  assert.equal(shows(processed(), { text: "raw" }, true), true);
});

check(
  "a match only in the transcription's correction shows the original",
  () => {
    const corrected = processed({
      transcription_correction: "the fixed words",
    });
    assert.equal(shows(corrected, { text: "fixed" }), true);
  },
);

check("a match only in a text a correction replaced changes nothing", () => {
  const ppCorrected = processed({ post_processed_correction: "Corrected." });
  assert.equal(shows(ppCorrected, { text: "polished" }), false);
  const corrected = processed({ transcription_correction: "fixed words" });
  assert.equal(shows(corrected, { text: "raw" }), false);
});

check("switching by hand during the search keeps the user's choice", () => {
  assert.equal(shows(processed(), { text: "raw" }, false, true), false);
  assert.equal(shows(processed(), { text: "polished" }, true, true), true);
});

check("the whole word rule decides which text matches", () => {
  const entry = processed({
    transcription_text: "start art",
    post_processed_text: "Start.",
  });
  assert.equal(shows(entry, { text: "art", whole_word: true }), true);
  assert.equal(shows(entry, { text: "art" }), false);
});

check("an entry without post-processing has nothing to switch to", () => {
  const plain = processed({ post_processed_text: null });
  assert.equal(shows(plain, { text: "raw" }), false);
});

// ---- A match only in a text a correction replaced -----------------------

const beforeCorrection = (shown: HistoryEntry, search: Partial<SearchFields>) =>
  matchesOnlyBeforeCorrection(shown, textMatcher(fields(search)));

check("a match only in the post-processing a correction replaced", () => {
  const entry = processed({ post_processed_correction: "Corrected." });
  assert.equal(beforeCorrection(entry, { text: "polished" }), true);
});

check("a match only in the transcription a correction replaced", () => {
  const entry = processed({ transcription_correction: "fixed words" });
  assert.equal(beforeCorrection(entry, { text: "raw" }), true);
  // Without post-processing too: its one view shows the correction.
  const plain = processed({
    post_processed_text: null,
    transcription_correction: "fixed words",
  });
  assert.equal(beforeCorrection(plain, { text: "raw" }), true);
});

check("no hint when a view shows the match", () => {
  // The original view still has it.
  const inOriginal = processed({
    post_processed_text: "The raw text.",
    post_processed_correction: "Corrected.",
  });
  assert.equal(beforeCorrection(inOriginal, { text: "raw" }), false);
  // The correction itself has it.
  const inCorrection = processed({ post_processed_correction: "Polished!" });
  assert.equal(beforeCorrection(inCorrection, { text: "polished" }), false);
  // Without a correction every text is in a view.
  assert.equal(beforeCorrection(processed(), { text: "raw" }), false);
  assert.equal(beforeCorrection(processed(), { text: "polished" }), false);
});

check("no hint when nothing matches", () => {
  const entry = processed({ post_processed_correction: "Corrected." });
  assert.equal(beforeCorrection(entry, { text: "elsewhere" }), false);
});

check("no hint without a text to search", () => {
  const entry = processed({ post_processed_correction: "Corrected." });
  assert.equal(matchesOnlyBeforeCorrection(entry, null), false);
  assert.equal(beforeCorrection(entry, { window: "polished" }), false);
  assert.equal(beforeCorrection(entry, { text: "(", regex: true }), false);
});

check("the hint follows case and whole words as the search does", () => {
  const entry = processed({
    post_processed_text: "Start polished.",
    post_processed_correction: "Corrected.",
  });
  assert.equal(
    beforeCorrection(entry, { text: "art", whole_word: true }),
    false,
  );
  assert.equal(beforeCorrection(entry, { text: "art" }), true);
  assert.equal(
    beforeCorrection(entry, { text: "start", case_sensitive: true }),
    false,
  );
  assert.equal(
    beforeCorrection(entry, { text: "Start", case_sensitive: true }),
    true,
  );
});

check("a blank correction replaces nothing", () => {
  const entry = processed({ post_processed_correction: "  " });
  assert.equal(beforeCorrection(entry, { text: "polished" }), false);
});

// ---- Highlighting the window line and the file name ---------------------

const win = (
  app_name: string | null,
  title: string | null,
  app_id: string | null = "com.example.app",
): EntryWindowContext => ({
  window_app_id: app_id,
  window_app_name: app_name,
  window_title: title,
  context_requested: false,
  context_entry_ids: [],
  sent_as_context_at: null,
  word_replacements: [],
});

const lineRanges = (
  context: EntryWindowContext,
  search: Partial<SearchFields>,
) => windowLineRanges(context, windowMatcher(fields(search)));

// "Safari – GitHub – DanyWhere": the title starts at 9.
const safari = win("Safari", "GitHub – DanyWhere");

check("the window line highlights the app and the title", () => {
  assert.deepEqual(lineRanges(safari, { window: "saf" }), [[0, 3]]);
  assert.deepEqual(lineRanges(safari, { window: "github" }), [[9, 15]]);
  assert.deepEqual(lineRanges(safari, { window: "fari – Git" }), [[2, 12]]);
});

check("the window line ignores case and whole words", () => {
  assert.deepEqual(
    lineRanges(safari, {
      window: "SAF",
      case_sensitive: true,
      whole_word: true,
    }),
    [[0, 3]],
  );
});

check("a window regex is anchored at the app, the title or the line", () => {
  assert.deepEqual(lineRanges(safari, { window: "^safari$", regex: true }), [
    [0, 6],
  ]);
  assert.deepEqual(lineRanges(safari, { window: "^github", regex: true }), [
    [9, 15],
  ]);
  assert.deepEqual(
    lineRanges(safari, { window: "^Safari – Git", regex: true }),
    [[0, 12]],
  );
});

check("the window line's matches are joined without repeats", () => {
  // "Safari – Mail": in the app, the title and the line alike.
  assert.deepEqual(lineRanges(win("Safari", "Mail"), { window: "i" }), [
    [5, 6],
    [11, 12],
  ]);
  // "Mail – Inbox": the title's "^Inbox" and the line's "l – In" overlap.
  assert.deepEqual(
    lineRanges(win("Mail", "Inbox"), { window: "^Inbox|l – In", regex: true }),
    [[3, 12]],
  );
});

check("the window line falls back to the app id as History does", () => {
  assert.deepEqual(
    lineRanges(win("", "Inbox", "com.apple.mail"), { window: "apple" }),
    [[4, 9]],
  );
});

check("no window line or no window field highlights nothing", () => {
  assert.deepEqual(lineRanges(win("Mail", null), { window: "mail" }), []);
  assert.deepEqual(lineRanges(safari, { text: "safari" }), []);
  assert.deepEqual(
    windowLineRanges(null, windowMatcher(fields({ window: "a" }))),
    [],
  );
});

check("a window pattern JavaScript can't read highlights nothing", () => {
  assert.equal(windowMatcher(fields({ window: "(?i)abc", regex: true })), null);
  assert.equal(windowMatcher(fields({ window: "[a-", regex: true })), null);
  assert.equal(windowMatcher(EMPTY_SEARCH), null);
});

check("a transcribed file's name is highlighted like the window", () => {
  const name = "Interview Müller.m4a";
  const fileRanges = (search: Partial<SearchFields>) =>
    matchRanges(name, windowMatcher(fields(search)));
  assert.deepEqual(fileRanges({ window: "MÜLLER" }), [[10, 16]]);
  assert.deepEqual(fileRanges({ window: "\\.m4a$", regex: true }), [[16, 20]]);
  // Literal without the regex toggle.
  assert.deepEqual(fileRanges({ window: "w.m" }), []);
  assert.deepEqual(fileRanges({ window: "w.m", regex: true }), [[8, 11]]);
});

if (failures > 0) {
  console.error(`historySearch: ${failures} check(s) failed`);
  process.exit(1);
}
console.log("historySearch: all assertions passed");
