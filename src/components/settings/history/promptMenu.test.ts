// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/history/promptMenu.test.ts
//
// DanyWhere: the menu of History's ✨ (H6, qa 2026-11-18_2027): every
// prompt of the library by name, in the order of the profile in use, the
// active ones checked, long names cut with "…", a tooltip with the whole
// name and the first lines of the text.
import assert from "node:assert/strict";
import {
  MENU_NAME_LENGTH,
  promptMenuItems,
  promptTooltip,
  shortName,
} from "./promptMenu";

let total = 0;
const failures: string[] = [];
const check = (name: string, body: () => void) => {
  total += 1;
  try {
    body();
  } catch (error) {
    failures.push(`${name}: ${(error as Error).message.split("\n")[0]}`);
  }
};

const prompts = [
  { id: "a", name: "Improve Transcriptions", prompt: "Fix the text." },
  {
    id: "b",
    name: "Diktat (DE/EN)",
    prompt: "Line 1\nLine 2\n\nLine 3\nLine 4",
  },
  { id: "c", name: "Absätze", prompt: "Paragraphs." },
];

check("all prompts, in the profile's order, active ones checked", () => {
  const items = promptMenuItems(prompts, [
    { id: "b", active: true },
    { id: "a", active: false },
    { id: "c", active: true },
  ]);
  assert.deepEqual(
    items.map((item) => [item.id, item.active]),
    [
      ["b", true],
      ["a", false],
      ["c", true],
    ],
  );
});

check("prompts the order lacks come last, unchecked", () => {
  const items = promptMenuItems(prompts, [{ id: "c", active: true }]);
  assert.deepEqual(
    items.map((item) => [item.id, item.active]),
    [
      ["c", true],
      ["a", false],
      ["b", false],
    ],
  );
});

check("an order entry for a prompt that is gone is left out", () => {
  const items = promptMenuItems(prompts, [{ id: "x", active: true }]);
  assert.deepEqual(
    items.map((item) => item.id),
    ["a", "b", "c"],
  );
});

check("a short name stays as it is", () => {
  assert.equal(shortName("Absätze"), "Absätze");
});

check("a long name is cut with …", () => {
  const long = "A very long prompt name that goes on and on and on";
  const short = shortName(long);
  assert.ok(short.endsWith("…"), short);
  assert.equal([...short].length, MENU_NAME_LENGTH);
  assert.ok(long.startsWith(short.slice(0, -1).trimEnd()));
});

check("cutting never splits a character in two", () => {
  const name = "😀".repeat(MENU_NAME_LENGTH + 5);
  const short = shortName(name);
  assert.equal([...short].length, MENU_NAME_LENGTH);
  assert.ok(!short.includes("�"));
});

check("the tooltip: whole name, then the first lines of the text", () => {
  assert.equal(
    promptTooltip(prompts[1], 3),
    "Diktat (DE/EN)\n\nLine 1\nLine 2\nLine 3\n…",
  );
});

check("the tooltip of a short text has no …", () => {
  assert.equal(
    promptTooltip(prompts[0], 3),
    "Improve Transcriptions\n\nFix the text.",
  );
});

check("the tooltip of an empty text is the name", () => {
  assert.equal(
    promptTooltip({ id: "e", name: "Empty", prompt: "  " }, 3),
    "Empty",
  );
});

check("each item carries its short name and tooltip", () => {
  const [item] = promptMenuItems(prompts, [{ id: "a", active: true }]);
  assert.equal(item.label, shortName("Improve Transcriptions"));
  assert.equal(item.tooltip, promptTooltip(prompts[0]));
});

console.log(`${total - failures.length}/${total} checks passed`);
if (failures.length > 0) {
  for (const failure of failures) console.error(`FAIL ${failure}`);
  process.exit(1);
}
