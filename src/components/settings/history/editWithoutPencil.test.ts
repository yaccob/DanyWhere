// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/history/editWithoutPencil.test.ts
// DanyWhere: a History entry has no pencil (user, 2026-11-16: "dieser
// normale Stift zum Bearbeiten ist ja mittlerweile völlig überflüssig, weil
// man ja nur in das Textfeld reinklicken muss"). A click into the text edits
// it. For the keyboard, screen readers and scripts there is a hidden button
// named like the edit field next to the text, as in Atlassian's inline edit
// (user, 2026-11-17: "bau den unsichtbaren Knopf wie bei Atlassian"): Tab
// reaches it, Enter or Space press it, and the box shows its focus ring.
// The text itself stays plain text, outside the tab order. The decisions
// are in `historyBox.ts` (tested in `historyBox.test.ts`), this checks that
// the box uses them and that the pencil is gone. The pencil that marks a
// version the user changed (`VersionTrack.tsx`) stays.
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

check("a hidden button named like the edit field opens editing", () => {
  // Named by `editLabel`, like the textarea: two uses.
  assert.equal(
    (box.match(/aria-label=\{editLabel\}/gu) ?? []).length,
    2,
    "HistoryTextBox.tsx: editLabel names both the button and the textarea",
  );
  has(box, /<button\b/u, "HistoryTextBox.tsx");
  has(box, /\bsr-only\b/u, "HistoryTextBox.tsx");
  has(box, /ref=\{editButtonRef\}/u, "HistoryTextBox.tsx");
});

check("the text itself is not in the tab order", () => {
  hasNot(box, /tabIndex=/u, "HistoryTextBox.tsx");
  hasNot(box, /handleViewKeyDown/u, "HistoryTextBox.tsx");
});

check("the box shows the button's keyboard focus", () => {
  has(
    box,
    /peer-focus-visible:ring-1 peer-focus-visible:ring-logo-primary/u,
    "HistoryTextBox.tsx",
  );
});

check("a key that ends an edit gives the button the focus back", () => {
  has(box, /returnsFocusAfterEditing\(/u, "HistoryTextBox.tsx");
  has(box, /editButtonRef\.current\?\.focus\(/u, "HistoryTextBox.tsx");
});

if (failures.length > 0) {
  for (const failure of failures) console.error(`FAILED: ${failure}`);
  console.error(
    `editWithoutPencil: ${failures.length} of ${checks} checks failed, ${checks - failures.length} passed`,
  );
  process.exit(1);
}
console.log(`editWithoutPencil: all ${checks} checks passed`);
