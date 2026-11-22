// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/wordListModelLine.test.ts
// DanyWhere (word lists, stage 1a, 2026-11-22): the word list pages name the
// speech model in effect there and say whether it takes the word lists while
// it recognizes speech, or leaves them to post-processing as known terms;
// the model table shows it as a capability. The user's wish: tell word lists
// the model can use apart from those it can't (sidecar
// qa/2026-11-22_2145_wortlisten-je-modell.md).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { wordListModelLine } from "./wordListModelLine";

const HERE = join(fileURLToPath(import.meta.url), "..");
const source = (file: string) => readFileSync(join(HERE, file), "utf8");
const english = JSON.parse(
  readFileSync(join(HERE, "../../i18n/locales/en/translation.json"), "utf8"),
);

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

check("a model with vocabulary uses the word lists", () => {
  assert.deepEqual(
    wordListModelLine({ name: "Whisper Turbo", supports_vocabulary: true }),
    { key: "settings.advanced.wordLists.modelUses", model: "Whisper Turbo" },
  );
});

check("a model without vocabulary leaves them to post-processing", () => {
  assert.deepEqual(
    wordListModelLine({ name: "Canary 1B v2", supports_vocabulary: false }),
    { key: "settings.advanced.wordLists.modelIgnores", model: "Canary 1B v2" },
  );
});

check("without a model there is no line", () => {
  assert.equal(wordListModelLine(undefined), null);
});

check("both lines exist in English and name the model", () => {
  const lists = english.settings.advanced.wordLists;
  for (const key of ["modelUses", "modelIgnores"]) {
    assert.equal(typeof lists[key], "string", key);
    assert.ok(lists[key].includes("{{model}}"), key);
  }
});

check("the word list group shows the line", () => {
  const page = source("advanced/AdvancedSettings.tsx");
  assert.ok(page.includes("<WordListModelHint"), "AdvancedSettings");
});

check("the model table shows it as a capability", () => {
  const capabilities = source("../shared/modelTable/capabilities.ts");
  assert.match(capabilities, /wordLists: \{ Icon: \w+, flag: "vocabulary" \}/);
  const table = source("../shared/modelTable/tableModel.ts");
  assert.ok(table.includes("vocabulary: info.supports_vocabulary"));
  const tags = english.modelTable.tags;
  assert.equal(typeof tags.wordLists, "string");
  assert.equal(typeof tags.wordListsTitle, "string");
});

if (failures.length) {
  console.error(`${failures.length} of ${checks} checks failed:`);
  for (const failure of failures) console.error(`  ${failure}`);
  process.exit(1);
}
console.log(`${checks} of ${checks} checks passed`);
