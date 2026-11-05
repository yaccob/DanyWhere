import assert from "node:assert/strict";
import {
  NO_WORD_LIST,
  activeListOption,
  addWord,
  normalizeWord,
  removeWord,
} from "./wordLists";

// Words are cleaned the way the global list always cleaned them.
assert.equal(normalizeWord('  Claude   "Code" '), "Claude Code");
assert.equal(normalizeWord("<b>"), "b");

// Adding keeps the order and refuses blanks, duplicates and long words.
assert.deepEqual(addWord(["Jira"], " Confluence "), {
  kind: "added",
  words: ["Jira", "Confluence"],
});
assert.deepEqual(addWord(["Jira"], "Jira"), {
  kind: "duplicate",
  word: "Jira",
});
assert.deepEqual(addWord(["Jira"], "   "), { kind: "invalid" });
assert.deepEqual(addWord([], "x".repeat(51)), { kind: "invalid" });
assert.deepEqual(addWord([], "x".repeat(50)), {
  kind: "added",
  words: ["x".repeat(50)],
});

assert.deepEqual(removeWord(["a", "b", "c"], "b"), ["a", "c"]);

// The active-list dropdown: "none" is a value of its own, and an id that
// names no list shows as none.
const lists = [
  { id: "w1", name: "Work", words: [] },
  { id: "w2", name: "Code", words: [] },
];
assert.equal(activeListOption(lists, null), NO_WORD_LIST);
assert.equal(activeListOption(lists, "w2"), "w2");
assert.equal(activeListOption(lists, "gone"), NO_WORD_LIST);
assert.notEqual(NO_WORD_LIST, "");
