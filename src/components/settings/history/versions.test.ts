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
  entryText,
  isSaveShortcut,
  matchingVersions,
  postProcessActions,
  postProcessQuestion,
  redoNeedsConfirm,
  retranscribeNeedsConfirm,
  saveModes,
  shownVersion,
  stepSeconds,
  topIndex,
  versionMark,
  versionText,
  versionsLostByEditing,
  versionsLostByGoingBack,
  versionsLostByPostProcessing,
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
): VersionStep => ({ by, at, prompt_name });

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

check("an empty stack has the transcription's index as its top", () => {
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

// ---- Saving an edit ------------------------------------------------------

check("the transcription is never replaced: only a new version", () => {
  assert.deepEqual(saveModes(threeLayers, 0), ["new"]);
  assert.equal(defaultSaveMode(threeLayers, 0), "new");
  assert.deepEqual(saveModes(entry([transcription()]), 0), ["new"]);
});

check("a version above it can be changed or kept", () => {
  assert.deepEqual(saveModes(threeLayers, 1), ["replace", "new"]);
  assert.deepEqual(saveModes(threeLayers, 2), ["replace", "new"]);
});

check("the default changes a version whose last step is the user's", () => {
  assert.equal(defaultSaveMode(threeLayers, 2), "replace");
  const aiThenUser = entry([
    transcription(),
    version("Polished, fixed.", "ai", "user"),
  ]);
  assert.equal(defaultSaveMode(aiThenUser, 1), "replace");
});

check("the default keeps the AI's text and adds a new version", () => {
  assert.equal(defaultSaveMode(threeLayers, 1), "new");
  const userThenAi = entry([
    transcription(),
    version("Polished again.", "user", "ai"),
  ]);
  assert.equal(defaultSaveMode(userThenAi, 1), "new");
});

// ---- Post-processing the current version -----------------------------------
// ✨ works on the current version, the one the entry shows (`shown`); on the
// top as before, below it after deleting the versions above it.

check("on the transcription only post-processing further", () => {
  assert.deepEqual(postProcessActions(entry([transcription()]), 0), [
    "post_process",
  ]);
});

check("on a version of the user's: further and replacing it", () => {
  const edited = entry([transcription(), version("fixed", "user")]);
  assert.deepEqual(postProcessActions(edited, 1), [
    "post_process",
    "post_process_replace",
  ]);
  assert.deepEqual(postProcessActions(threeLayers, 2), [
    "post_process",
    "post_process_replace",
  ]);
});

check("on a version the AI made: redoing it too", () => {
  const processed = entry([transcription(), version("Polished.", "ai")]);
  assert.deepEqual(postProcessActions(processed, 1), [
    "post_process",
    "post_process_replace",
    "post_process_redo",
  ]);
  const aiThenUser = entry([
    transcription(),
    version("Polished, fixed.", "ai", "user"),
  ]);
  assert.deepEqual(postProcessActions(aiThenUser, 1), [
    "post_process",
    "post_process_replace",
    "post_process_redo",
  ]);
});

check("a version the user started isn't redone, even with an AI step", () => {
  const userThenAi = entry([
    transcription(),
    version("Polished again.", "user", "ai"),
  ]);
  assert.deepEqual(postProcessActions(userThenAi, 1), [
    "post_process",
    "post_process_replace",
  ]);
});

check("the transcription below the top: only post-processing further", () => {
  assert.deepEqual(postProcessActions(threeLayers, 0), ["post_process"]);
});

check("the AI's version below the top: redoing it too", () => {
  assert.deepEqual(postProcessActions(threeLayers, 1), [
    "post_process",
    "post_process_replace",
    "post_process_redo",
  ]);
});

check("the user's version below an AI one isn't redone", () => {
  const userBelowAi = entry([
    transcription(),
    version("Polished again.", "user", "ai"),
    version("Polished twice.", "ai"),
  ]);
  assert.deepEqual(postProcessActions(userBelowAi, 1), [
    "post_process",
    "post_process_replace",
  ]);
});

check("a current version the stack doesn't have counts as the top", () => {
  assert.deepEqual(postProcessActions(threeLayers, 9), [
    "post_process",
    "post_process_replace",
  ]);
  assert.equal(versionsLostByPostProcessing(threeLayers, 9), 0);
});

check("redoing asks first only over the user's own steps", () => {
  const processed = entry([transcription(), version("Polished.", "ai")]);
  assert.equal(redoNeedsConfirm(processed, 1), false);
  const aiThenUser = entry([
    transcription(),
    version("Polished, fixed.", "ai", "user"),
  ]);
  assert.equal(redoNeedsConfirm(aiThenUser, 1), true);
});

check(
  "redoing asks over the current version's own steps, not the top's",
  () => {
    const editedBelow = entry([
      transcription(),
      version("Polished, fixed.", "ai", "user"),
      version("Polished twice.", "ai"),
    ]);
    assert.equal(redoNeedsConfirm(editedBelow, 1), true);
    assert.equal(redoNeedsConfirm(editedBelow, 2), false);
    // threeLayers' top is the user's, the AI's version below it is not.
    assert.equal(redoNeedsConfirm(threeLayers, 1), false);
  },
);

check("✨ on the top deletes nothing", () => {
  assert.equal(versionsLostByPostProcessing(threeLayers, 2), 0);
  assert.equal(versionsLostByPostProcessing(entry([transcription()]), 0), 0);
});

check("✨ below the top deletes the versions above the current one", () => {
  assert.equal(versionsLostByPostProcessing(threeLayers, 1), 1);
  assert.equal(versionsLostByPostProcessing(threeLayers, 0), 2);
});

// ---- What ✨ asks first ----------------------------------------------------

check(
  "✨ on the top asks nothing, unless a redo loses the user's steps",
  () => {
    assert.equal(postProcessQuestion(threeLayers, 2, "post_process"), null);
    assert.equal(
      postProcessQuestion(threeLayers, 2, "post_process_replace"),
      null,
    );
    const processed = entry([transcription(), version("Polished.", "ai")]);
    assert.equal(postProcessQuestion(processed, 1, "post_process_redo"), null);
    const aiThenUser = entry([
      transcription(),
      version("Polished, fixed.", "ai", "user"),
    ]);
    assert.deepEqual(postProcessQuestion(aiThenUser, 1, "post_process_redo"), {
      kind: "redo",
    });
  },
);

check("✨ further below the top asks about the versions above it", () => {
  assert.deepEqual(postProcessQuestion(threeLayers, 1, "post_process"), {
    kind: "above",
    count: 1,
  });
  assert.deepEqual(postProcessQuestion(threeLayers, 0, "post_process"), {
    kind: "above",
    count: 2,
  });
});

check("replacing below the top asks about the versions above it", () => {
  // Replacing post-processes the current version itself, so the user's
  // steps in it live on in the result: only the versions above go.
  const editedBelow = entry([
    transcription(),
    version("Polished, fixed.", "ai", "user"),
    version("Polished twice.", "ai"),
  ]);
  assert.deepEqual(
    postProcessQuestion(editedBelow, 1, "post_process_replace"),
    { kind: "above", count: 1 },
  );
});

check("redoing below the top asks about the versions above it", () => {
  assert.deepEqual(postProcessQuestion(threeLayers, 1, "post_process_redo"), {
    kind: "above",
    count: 1,
  });
});

check("redoing over the user's steps below the top asks once for both", () => {
  const editedBelow = entry([
    transcription(),
    version("Polished, fixed.", "ai", "user"),
    version("Polished twice.", "ai"),
    version("Polished thrice.", "ai"),
  ]);
  assert.deepEqual(postProcessQuestion(editedBelow, 1, "post_process_redo"), {
    kind: "redo_above",
    count: 2,
  });
});

// ---- What goes, and when History asks first ------------------------------

check("editing a version below the top removes the ones above it", () => {
  assert.equal(versionsLostByEditing(threeLayers, 2), 0);
  assert.equal(versionsLostByEditing(threeLayers, 1), 1);
  assert.equal(versionsLostByEditing(threeLayers, 0), 2);
});

check("going back to a version removes the ones above it", () => {
  assert.equal(versionsLostByGoingBack(threeLayers, 0), 2);
  assert.equal(versionsLostByGoingBack(threeLayers, 1), 1);
  assert.equal(versionsLostByGoingBack(threeLayers, 2), 0);
  assert.equal(versionsLostByGoingBack(threeLayers, 7), 0);
});

check("re-transcribing a plain entry asks nothing", () => {
  const plain = entry([transcription()]);
  assert.equal(retranscribeNeedsConfirm(plain, "transcribe"), false);
  assert.equal(
    retranscribeNeedsConfirm(plain, "transcribe_and_post_process"),
    false,
  );
});

check("re-transcribing a dictation with post-processing asks nothing", () => {
  // It's post-processed again, so [transcription, ✨] comes back.
  const processed = entry([transcription(), version("Polished.", "ai")], true);
  assert.equal(retranscribeNeedsConfirm(processed, "transcribe"), false);
  assert.equal(
    retranscribeNeedsConfirm(processed, "transcribe_and_post_process"),
    false,
  );
});

check("re-transcribing asks when the second version won't come back", () => {
  const processed = entry([transcription(), version("Polished.", "ai")], false);
  assert.equal(retranscribeNeedsConfirm(processed, "transcribe"), true);
  assert.equal(
    retranscribeNeedsConfirm(processed, "transcribe_and_post_process"),
    false,
  );
});

check("re-transcribing asks over any step of the user's", () => {
  const edited = entry([transcription(), version("fixed", "user")], false);
  assert.equal(retranscribeNeedsConfirm(edited, "transcribe"), true);
  assert.equal(
    retranscribeNeedsConfirm(edited, "transcribe_and_post_process"),
    true,
  );
  const aiThenUser = entry(
    [transcription(), version("Polished, fixed.", "ai", "user")],
    true,
  );
  assert.equal(retranscribeNeedsConfirm(aiThenUser, "transcribe"), true);
});

check("re-transcribing asks over more than two versions", () => {
  const twice = entry(
    [
      transcription(),
      version("Polished.", "ai"),
      version("Polished twice.", "ai"),
    ],
    true,
  );
  assert.equal(retranscribeNeedsConfirm(twice, "transcribe"), true);
  assert.equal(
    retranscribeNeedsConfirm(twice, "transcribe_and_post_process"),
    true,
  );
  assert.equal(retranscribeNeedsConfirm(threeLayers, "transcribe"), true);
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
    atSeconds: null,
  });
  assert.deepEqual(describeStep(step("user")), {
    key: "user",
    prompt: null,
    atSeconds: null,
  });
  assert.deepEqual(describeStep(step("ai")), {
    key: "ai",
    prompt: null,
    atSeconds: null,
  });
});

check("an AI step names its prompt when it's known", () => {
  assert.deepEqual(describeStep(step("ai", null, "E-Mail")), {
    key: "aiPrompt",
    prompt: "E-Mail",
    atSeconds: null,
  });
  // A blank name is none.
  assert.equal(describeStep(step("ai", null, "  ")).key, "ai");
  // Only AI steps have a prompt.
  assert.equal(describeStep(step("user", null, "E-Mail")).key, "user");
});

check("a step's time is in seconds, as History formats dates", () => {
  assert.equal(
    describeStep(step("user", 1_763_240_000_999)).atSeconds,
    1_763_240_000,
  );
  assert.equal(describeStep(step("user", null)).atSeconds, null);
});

// ---- The time a chip's tooltip names for a step ---------------------------

type Timed = Pick<HistoryEntry, "versions" | "timestamp" | "source_name">;
const DICTATED_AT = 1_763_200_000;
const timed = (
  versions: HistoryVersion[],
  source_name: string | null = null,
): Timed => ({ versions, timestamp: DICTATED_AT, source_name });

check("a step with a time keeps it, in seconds", () => {
  const entry = timed([
    transcription(),
    { text: "x", steps: [step("user", 1_763_240_000_999)] },
  ]);
  assert.equal(stepSeconds(entry, 1, 0), 1_763_240_000);
});

check("the transcription without a time is the entry's", () => {
  assert.equal(stepSeconds(timed([transcription()]), 0, 0), DICTATED_AT);
  assert.equal(
    stepSeconds(timed([transcription()], "talk.m4a"), 0, 0),
    DICTATED_AT,
  );
});

check("a dictation's own post-processing without a time is the entry's", () => {
  const dictated = timed([transcription(), version("Polished.", "ai")]);
  assert.equal(stepSeconds(dictated, 1, 0), DICTATED_AT);
});

check("other steps without a time have none", () => {
  // A file's post-processing may have come later.
  const file = timed([transcription(), version("Polished.", "ai")], "a.m4a");
  assert.equal(stepSeconds(file, 1, 0), null);
  // Migrated edits.
  const edited = timed([transcription(), version("fixed", "user")]);
  assert.equal(stepSeconds(edited, 1, 0), null);
  // Post-processing of an edit, and a later step of the first version.
  const later = timed([
    transcription(),
    version("fixed", "user"),
    version("Polished.", "ai"),
  ]);
  assert.equal(stepSeconds(later, 2, 0), null);
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

if (failures > 0) {
  console.error(`versions: ${failures} of ${checks} check(s) failed`);
  process.exit(1);
}
console.log(`versions: all ${checks} checks passed`);
