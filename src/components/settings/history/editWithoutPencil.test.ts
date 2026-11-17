// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/history/editWithoutPencil.test.ts
// DanyWhere: a History entry has no pencil (user, 2026-11-16: "dieser
// normale Stift zum Bearbeiten ist ja mittlerweile völlig überflüssig, weil
// man ja nur in das Textfeld reinklicken muss"). A click into the text edits
// it, and so does Enter or Space on the box, which Tab reaches; the
// decisions are in `historyBox.ts` (tested in `historyBox.test.ts`), this
// checks that the box uses them and that the pencil is gone. The pencil
// that marks a version the user changed (`VersionTrack.tsx`) stays.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = join(fileURLToPath(import.meta.url), "..");
const source = (file: string) => readFileSync(join(HERE, file), "utf8");

let checks = 0;
const failures: string[] = [];
const check = (name: string, run: () => void) => {
  checks += 1;
  try {
    run();
  } catch (error) {
    failures.push(`${name}: ${(error as Error).message}`);
  }
};

const settings = source("HistorySettings.tsx");
const box = source("HistoryTextBox.tsx");

// Short messages instead of the whole source in a failure.
const has = (text: string, pattern: RegExp, file: string) =>
  assert.match(text, pattern, `${file} lacks ${pattern}`);
const hasNot = (text: string, pattern: RegExp, file: string) =>
  assert.doesNotMatch(text, pattern, `${file} still has ${pattern}`);

check("the entry's buttons have no pencil", () => {
  hasNot(settings, /<Pencil\b/u, "HistorySettings.tsx");
  hasNot(settings, /\bPencil,/u, "HistorySettings.tsx");
});

check("the edit field keeps its accessible name", () => {
  has(
    settings,
    /editLabel=\{t\("settings\.history\.edit"\)\}/u,
    "HistorySettings.tsx",
  );
});

check("the box is in the tab order while it can be edited", () => {
  has(box, /tabIndex=\{boxTabIndex\(/u, "HistoryTextBox.tsx");
});

check("Enter or Space on the box starts editing", () => {
  has(box, /activatesEditingByKey\(/u, "HistoryTextBox.tsx");
  has(box, /onKeyDown=\{handleViewKeyDown\}/u, "HistoryTextBox.tsx");
});

check("the box shows the keyboard focus", () => {
  has(
    box,
    /focus-visible:ring-1 focus-visible:ring-logo-primary/u,
    "HistoryTextBox.tsx",
  );
});

check("ending an edit gives the box the focus back", () => {
  has(box, /returnsFocusAfterEditing\(/u, "HistoryTextBox.tsx");
});

if (failures.length > 0) {
  for (const failure of failures) console.error(`FAILED: ${failure}`);
  console.error(
    `editWithoutPencil: ${failures.length} of ${checks} checks failed, ${checks - failures.length} passed`,
  );
  process.exit(1);
}
console.log(`editWithoutPencil: all ${checks} checks passed`);
