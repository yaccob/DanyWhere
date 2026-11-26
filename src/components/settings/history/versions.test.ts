import assert from "node:assert/strict";
import type {
  HistoryEntry,
  HistoryVersion,
  VersionAuthor,
  VersionStep,
} from "@/bindings";
import { EMPTY_SEARCH, textMatcher, type SearchFields } from "./historySearch";
import {
  defaultSaveMode,
  describeStep,
  distinctLines,
  dropIndex,
  dropSlot,
  entryText,
  hasText,
  indexAfterDelete,
  indexAfterMove,
  isSaveShortcut,
  matchingVersions,
  modelName,
  moveTarget,
  ranWithSpokenInstruction,
  saveModes,
  shownVersion,
  stepSeconds,
  topIndex,
  versionActions,
  versionMark,
  versionText,
} from "./versions";

let checks = 0;
let failures = 0;
const check = (name: string, run: () => void) => {
  checks += 1;
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

// ---- Building stacks -----------------------------------------------------

const step = (
  by: VersionAuthor,
  at: number | null = null,
  prompt_name: string | null = null,
  model: string | null = null,
): VersionStep => ({ by, at, prompt_name, model });

const version = (text: string, ...by: VersionAuthor[]): HistoryVersion => ({
  text,
  steps: by.map((author) => step(author)),
});

const transcription = (text = "the raw words") =>
  version(text, "transcription");

type Entry = Pick<HistoryEntry, "versions" | "post_process_requested">;

const entry = (
  versions: HistoryVersion[],
  post_process_requested = false,
): Entry => ({ versions, post_process_requested });

// Transcription → ✨ → ✎, the shape of most edited entries.
const threeLayers = entry(
  [
    transcription("the raw words"),
    version("The polished text.", "ai"),
    version("The polished text, fixed.", "user"),
  ],
  true,
);

const fields = (changes: Partial<SearchFields>): SearchFields => ({
  ...EMPTY_SEARCH,
  ...changes,
});
const search = (changes: Partial<SearchFields>) => textMatcher(fields(changes));

// ---- The top and the texts -----------------------------------------------

check("the top is the last version", () => {
  assert.equal(topIndex(entry([transcription()])), 0);
  assert.equal(topIndex(threeLayers), 2);
});

check("an empty stack has the first index as its top", () => {
  assert.equal(topIndex(entry([])), 0);
});

check("a version's text, and the entry's text is the top's", () => {
  assert.equal(versionText(threeLayers, 0), "the raw words");
  assert.equal(versionText(threeLayers, 1), "The polished text.");
  assert.equal(entryText(threeLayers), "The polished text, fixed.");
  assert.equal(entryText(entry([transcription("only")])), "only");
});

check("a version that doesn't exist has no text", () => {
  assert.equal(versionText(threeLayers, 3), "");
  assert.equal(versionText(threeLayers, -1), "");
  assert.equal(entryText(entry([])), "");
});

// ---- Which versions the search matches -----------------------------------

check("the versions with a match, bottom to top", () => {
  assert.deepEqual(matchingVersions(threeLayers, search({ text: "raw" })), [0]);
  assert.deepEqual(
    matchingVersions(threeLayers, search({ text: "polished" })),
    [1, 2],
  );
  assert.deepEqual(matchingVersions(threeLayers, search({ text: "fixed" })), [
    2,
  ]);
  assert.deepEqual(
    matchingVersions(threeLayers, search({ text: "nowhere" })),
    [],
  );
});

check("no search, or a pattern JavaScript can't read, matches nothing", () => {
  assert.deepEqual(matchingVersions(threeLayers, null), []);
  assert.deepEqual(
    matchingVersions(threeLayers, search({ text: "(", regex: true })),
    [],
  );
});

check("the matches follow case and whole words as the search does", () => {
  assert.deepEqual(
    matchingVersions(
      threeLayers,
      search({ text: "the", case_sensitive: true }),
    ),
    [0],
  );
  assert.deepEqual(
    matchingVersions(
      threeLayers,
      search({ text: "The", case_sensitive: true }),
    ),
    [1, 2],
  );
  assert.deepEqual(
    matchingVersions(threeLayers, search({ text: "fix", whole_word: true })),
    [],
  );
});

// ---- Which version an entry shows ----------------------------------------

check("without a search the entry shows its top", () => {
  assert.equal(shownVersion(threeLayers, null, null), 2);
  // A search of the window alone has no text to match.
  assert.equal(shownVersion(threeLayers, search({ window: "mail" }), null), 2);
});

check("without a search the entry shows the version the user chose", () => {
  assert.equal(shownVersion(threeLayers, null, 0), 0);
  assert.equal(shownVersion(threeLayers, null, 1), 1);
});

check("a choice the stack no longer has shows the top", () => {
  assert.equal(shownVersion(threeLayers, null, 5), 2);
  assert.equal(shownVersion(threeLayers, null, -1), 2);
});

check("during a search the top shows when it matches", () => {
  assert.equal(
    shownVersion(threeLayers, search({ text: "polished" }), null),
    2,
  );
});

check("else the topmost version that matches", () => {
  assert.equal(shownVersion(threeLayers, search({ text: "raw" }), null), 0);
  const fourLayers = entry([
    transcription("alpha"),
    version("beta", "ai"),
    version("beta two", "user"),
    version("gamma", "ai"),
  ]);
  assert.equal(shownVersion(fourLayers, search({ text: "beta" }), null), 2);
});

check("with no version matching the top shows", () => {
  assert.equal(shownVersion(threeLayers, search({ text: "nowhere" }), null), 2);
});

check("a choice from before the search stays while it matches", () => {
  assert.equal(shownVersion(threeLayers, search({ text: "polished" }), 1), 1);
  // It doesn't match: the topmost version that does.
  assert.equal(shownVersion(threeLayers, search({ text: "fixed" }), 0), 2);
  // Nothing matches: the choice.
  assert.equal(shownVersion(threeLayers, search({ text: "nowhere" }), 0), 0);
});

check("the user's choice during this search wins", () => {
  assert.equal(shownVersion(threeLayers, search({ text: "raw" }), 1, true), 1);
  assert.equal(shownVersion(threeLayers, search({ text: "raw" }), 2, true), 2);
});

check("a pattern JavaScript can't read shows as without a search", () => {
  assert.equal(
    shownVersion(threeLayers, search({ text: "(", regex: true }), null),
    2,
  );
  assert.equal(
    shownVersion(threeLayers, search({ text: "(", regex: true }), 1),
    1,
  );
});

check("an entry of one version always shows it", () => {
  const plain = entry([transcription()]);
  assert.equal(shownVersion(plain, null, null), 0);
  assert.equal(shownVersion(plain, search({ text: "nowhere" }), null), 0);
});

// ---- Whether an entry has text ------------------------------------------

check("an entry has text when its top version has some", () => {
  assert.equal(hasText(threeLayers), true);
  assert.equal(hasText(entry([transcription("")])), false);
  assert.equal(hasText(entry([transcription(" \n")])), false);
  assert.equal(hasText(entry([])), false);
  // Without its transcription, too.
  assert.equal(hasText(entry([version("Polished.", "ai")])), true);
});

// ---- Saving an edit ------------------------------------------------------

check("only the top version is changed in place", () => {
  assert.deepEqual(saveModes(threeLayers, 2), ["replace", "new"]);
  assert.deepEqual(saveModes(threeLayers, 1), ["new"]);
  assert.deepEqual(saveModes(threeLayers, 0), ["new"]);
});

check("a transcription on top is changed like any version", () => {
  assert.deepEqual(saveModes(entry([transcription()]), 0), ["replace", "new"]);
  // The default keeps it and adds a new version.
  assert.equal(defaultSaveMode(entry([transcription()]), 0), "new");
});

check("a version the stack doesn't have is saved as new", () => {
  assert.deepEqual(saveModes(threeLayers, 3), ["new"]);
  assert.equal(defaultSaveMode(threeLayers, 3), "new");
});

check("the default changes the top when its last step is the user's", () => {
  assert.equal(defaultSaveMode(threeLayers, 2), "replace");
  const aiThenUser = entry([
    transcription(),
    version("Polished, fixed.", "ai", "user"),
  ]);
  assert.equal(defaultSaveMode(aiThenUser, 1), "replace");
});

check("the default keeps the AI's text and adds a new version", () => {
  const userThenAi = entry([
    transcription(),
    version("Polished again.", "user", "ai"),
  ]);
  assert.equal(defaultSaveMode(userThenAi, 1), "new");
  // Below the top, the user's own version too: only as new.
  const userBelow = entry([
    transcription(),
    version("fixed", "user"),
    version("Polished.", "ai"),
  ]);
  assert.equal(defaultSaveMode(userBelow, 1), "new");
});

// ---- A version's menu: moving and deleting -------------------------------

check(
  "a version below the top goes to the top or up, above the bottom down",
  () => {
    assert.deepEqual(versionActions(threeLayers, 0), [
      "to_top",
      "up",
      "delete",
    ]);
    assert.deepEqual(versionActions(threeLayers, 1), [
      "to_top",
      "up",
      "down",
      "delete",
    ]);
    assert.deepEqual(versionActions(threeLayers, 2), ["down", "delete"]);
  },
);

check("the only version can be neither moved nor deleted", () => {
  assert.deepEqual(versionActions(entry([transcription()]), 0), []);
  assert.deepEqual(versionActions(threeLayers, 5), []);
});

check("where a version moves to", () => {
  assert.equal(moveTarget(threeLayers, 0, "to_top"), 2);
  assert.equal(moveTarget(threeLayers, 0, "up"), 1);
  assert.equal(moveTarget(threeLayers, 1, "up"), 2);
  assert.equal(moveTarget(threeLayers, 2, "down"), 1);
  assert.equal(moveTarget(threeLayers, 1, "down"), 0);
});

check("a version keeps being shown when another one moves", () => {
  // The one moved.
  assert.equal(indexAfterMove(0, 0, 2), 2);
  assert.equal(indexAfterMove(2, 2, 0), 0);
  // One it passed.
  assert.equal(indexAfterMove(1, 0, 2), 0);
  assert.equal(indexAfterMove(2, 0, 2), 1);
  assert.equal(indexAfterMove(0, 2, 0), 1);
  assert.equal(indexAfterMove(1, 2, 0), 2);
  // One it didn't pass.
  assert.equal(indexAfterMove(0, 1, 2), 0);
  assert.equal(indexAfterMove(3, 0, 2), 3);
});

check("a version keeps being shown when another one goes", () => {
  assert.equal(indexAfterDelete(1, 1), null);
  assert.equal(indexAfterDelete(2, 0), 1);
  assert.equal(indexAfterDelete(0, 2), 0);
});

// ---- Dragging a chip -----------------------------------------------------

// Three chips in a row, 40 px wide with 10 px between them, and one on a
// second row.
const row = (left: number, top = 0) => ({
  left,
  right: left + 40,
  top,
  bottom: top + 20,
});
const chips = [row(0), row(50), row(100), row(0, 30)];

check("a chip dropped on another goes before or after it by its middle", () => {
  assert.equal(dropSlot(chips, 55, 10), 1);
  assert.equal(dropSlot(chips, 85, 10), 2);
  assert.equal(dropSlot(chips, 5, 10), 0);
  assert.equal(dropSlot(chips, 139, 10), 3);
});

check("a chip dropped between or past the chips takes the nearest one", () => {
  assert.equal(dropSlot(chips, 46, 10), 1);
  assert.equal(dropSlot(chips, 300, 10), 3);
  assert.equal(dropSlot(chips, -30, 10), 0);
});

check("the row it is dropped in comes first", () => {
  assert.equal(dropSlot(chips, 35, 40), 4);
  assert.equal(dropSlot(chips, 5, 45), 3);
  // Between the rows, nearer the second.
  assert.equal(dropSlot(chips, 5, 27), 3);
});

check("right to left, after means left of the middle", () => {
  const rtl = [row(100), row(50), row(0)];
  assert.equal(dropSlot(rtl, 105, 10, true), 1);
  assert.equal(dropSlot(rtl, 135, 10, true), 0);
  assert.equal(dropSlot(rtl, 5, 10, true), 3);
});

check("no chips, no place but the first", () => {
  assert.equal(dropSlot([], 5, 5), 0);
});

check("the place a dragged version ends up in", () => {
  assert.equal(dropIndex(0, 0), 0);
  assert.equal(dropIndex(0, 1), 0);
  assert.equal(dropIndex(0, 3), 2);
  assert.equal(dropIndex(2, 0), 0);
  assert.equal(dropIndex(2, 3), 2);
  assert.equal(dropIndex(1, 3), 2);
});

// ---- The marks on the track ----------------------------------------------

check("a version's mark says who made it", () => {
  assert.equal(versionMark(transcription()), "transcription");
  assert.equal(versionMark(version("x", "ai")), "ai");
  assert.equal(versionMark(version("x", "user")), "user");
  assert.equal(versionMark(version("x", "user", "user")), "user");
});

check("a version both made carries both marks, in either order", () => {
  assert.equal(versionMark(version("x", "ai", "user")), "ai_user");
  assert.equal(versionMark(version("x", "user", "ai")), "ai_user");
  assert.equal(versionMark(version("x", "ai", "user", "ai")), "ai_user");
});

check("a step's time and prompt don't change the mark", () => {
  const timed: HistoryVersion = {
    text: "x",
    steps: [step("ai", 1_763_240_000_000, "E-Mail")],
  };
  assert.equal(versionMark(timed), "ai");
});

// ---- A step in the chip's tooltip ----------------------------------------

check("a step names who made it", () => {
  assert.deepEqual(describeStep(step("transcription")), {
    key: "transcription",
    prompt: null,
    model: null,
    atSeconds: null,
  });
  assert.deepEqual(describeStep(step("user")), {
    key: "user",
    prompt: null,
    model: null,
    atSeconds: null,
  });
  assert.deepEqual(describeStep(step("ai")), {
    key: "ai",
    prompt: null,
    model: null,
    atSeconds: null,
  });
});

check("an AI step names its prompt when it's known", () => {
  assert.deepEqual(describeStep(step("ai", null, "E-Mail")), {
    key: "aiPrompt",
    prompt: "E-Mail",
    model: null,
    atSeconds: null,
  });
  // A blank name is none.
  assert.equal(describeStep(step("ai", null, "  ")).key, "ai");
  // Only AI steps have a prompt.
  assert.equal(describeStep(step("user", null, "E-Mail")).key, "user");
});

check("a transcription names its speech model when it's known", () => {
  assert.deepEqual(
    describeStep(step("transcription", null, null, "canary-1b-v2")),
    {
      key: "transcriptionModel",
      prompt: null,
      model: "canary-1b-v2",
      atSeconds: null,
    },
  );
  // Without one (from before it was recorded), as before.
  assert.equal(describeStep(step("transcription")).key, "transcription");
  assert.equal(
    describeStep(step("transcription", null, null, " ")).key,
    "transcription",
  );
  // Only a transcription has a model.
  assert.equal(describeStep(step("ai", null, null, "canary-1b-v2")).key, "ai");
});

// DanyWhere: a post-processing with the instruction spoken for the
// dictation says so (qa 2026-11-26_1720, F6).
check("a post-processing with the spoken instruction says so", () => {
  const withInstruction = {
    ...step("ai", 5, "E-Mail"),
    spoken_instruction: true,
  };
  assert.equal(ranWithSpokenInstruction(withInstruction), true);
  // Without the flag, from before it existed, or off.
  assert.equal(ranWithSpokenInstruction(step("ai", 5, "E-Mail")), false);
  assert.equal(
    ranWithSpokenInstruction({ ...step("ai"), spoken_instruction: false }),
    false,
  );
  // Only a post-processing runs with it.
  assert.equal(
    ranWithSpokenInstruction({ ...step("user"), spoken_instruction: true }),
    false,
  );
  // Its prompt is still named as before.
  assert.equal(describeStep(withInstruction).key, "aiPrompt");
});

check("a model shows by its name, an unknown one by its id", () => {
  const models = [
    { id: "canary-1b-v2", name: "Canary 1B v2" },
    { id: "turbo", name: "Whisper Turbo" },
  ];
  assert.equal(modelName("canary-1b-v2", models), "Canary 1B v2");
  // A deleted or custom model the list no longer has.
  assert.equal(modelName("gone-model", models), "gone-model");
  // The name can come translated.
  assert.equal(
    modelName("turbo", models, (model) => `${model.name} (de)`),
    "Whisper Turbo (de)",
  );
});

check("a step's time is in seconds, as History formats dates", () => {
  assert.equal(
    describeStep(step("user", 1_763_240_000_999)).atSeconds,
    1_763_240_000,
  );
  assert.equal(describeStep(step("user", null)).atSeconds, null);
});

// ---- The time a chip's tooltip names for a step ---------------------------

type Timed = Pick<HistoryEntry, "versions" | "timestamp">;
const DICTATED_AT = 1_763_200_000;
const timed = (versions: HistoryVersion[]): Timed => ({
  versions,
  timestamp: DICTATED_AT,
});

check("a step with a time keeps it, in seconds", () => {
  const entry = timed([
    transcription(),
    { text: "x", steps: [step("user", 1_763_240_000_999)] },
  ]);
  assert.equal(stepSeconds(entry, 1, 0), 1_763_240_000);
});

check("a transcription without a time is the entry's, wherever it is", () => {
  assert.equal(stepSeconds(timed([transcription()]), 0, 0), DICTATED_AT);
  const moved = timed([version("Polished.", "ai"), transcription()]);
  assert.equal(stepSeconds(moved, 1, 0), DICTATED_AT);
});

check("other steps without a time have none, wherever they are", () => {
  // The backend gives a dictation's own post-processing the entry's time;
  // one without a time came later.
  const later = timed([transcription(), version("Polished.", "ai")]);
  assert.equal(stepSeconds(later, 1, 0), null);
  const edited = timed([transcription(), version("fixed", "user")]);
  assert.equal(stepSeconds(edited, 1, 0), null);
  const second = timed([transcription(), version("x", "user", "ai")]);
  assert.equal(stepSeconds(second, 1, 1), null);
});

check("a step the stack doesn't have has no time", () => {
  assert.equal(stepSeconds(timed([transcription()]), 1, 0), null);
  assert.equal(stepSeconds(timed([transcription()]), 0, 1), null);
});

// ---- The save shortcut ---------------------------------------------------

check("Cmd+Enter and Ctrl+Enter save, Enter alone doesn't", () => {
  assert.equal(
    isSaveShortcut({ key: "Enter", metaKey: true, ctrlKey: false }),
    true,
  );
  assert.equal(
    isSaveShortcut({ key: "Enter", metaKey: false, ctrlKey: true }),
    true,
  );
  assert.equal(
    isSaveShortcut({ key: "Enter", metaKey: false, ctrlKey: false }),
    false,
  );
  assert.equal(
    isSaveShortcut({ key: "s", metaKey: true, ctrlKey: false }),
    false,
  );
});

// A chip's tooltip names a repeated step once (qa 2026-11-22_0107: two
// saves in place in the same minute read the same).
check("a step line repeated right after itself shows once", () => {
  assert.deepEqual(
    distinctLines(["Edited by you, 01:45", "Edited by you, 01:45", "Shown"]),
    ["Edited by you, 01:45", "Shown"],
  );
});
check("the same line apart from itself stays", () => {
  assert.deepEqual(distinctLines(["A", "B", "A"]), ["A", "B", "A"]);
});

if (failures > 0) {
  console.error(`versions: ${failures} of ${checks} check(s) failed`);
  process.exit(1);
}
console.log(`versions: all ${checks} checks passed`);
