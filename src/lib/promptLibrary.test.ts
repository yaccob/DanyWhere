import assert from "node:assert/strict";
import {
  dropIndex,
  hasActivePrompt,
  isNameTaken,
  moved,
  movedBy,
  normalizedOrder,
  promptRows,
  runsAlone,
  withActive,
  type LibraryPrompt,
  type PromptOrderEntry,
} from "./promptLibrary";

let failures = 0;
const check = (name: string, run: () => void) => {
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

const prompts: LibraryPrompt[] = [
  { id: "a", name: "Diktat", prompt: "Clean it up." },
  { id: "b", name: "Absätze", prompt: "Add paragraphs." },
  { id: "c", name: "Mail", prompt: "Make an email of it." },
];
const e = (id: string, active: boolean): PromptOrderEntry => ({ id, active });

check("the order covers the whole library once", () => {
  assert.deepEqual(
    normalizedOrder(prompts, [
      e("c", true),
      e("gone", true),
      e("a", false),
      e("c", false),
    ]),
    [e("c", true), e("a", false), e("b", false)],
  );
});

check("rows follow the profile's order, numbers only on active ones", () => {
  const rows = promptRows(prompts, [e("c", true), e("a", false), e("b", true)]);
  assert.deepEqual(
    rows.map((row) => [row.prompt.name, row.active, row.number]),
    [
      ["Mail", true, 1],
      ["Diktat", false, null],
      ["Absätze", true, 2],
    ],
  );
});

check(
  "rows show a prompt the order doesn't know yet, inactive, at the end",
  () => {
    const rows = promptRows(prompts, [e("b", true)]);
    assert.deepEqual(
      rows.map((row) => [row.prompt.id, row.active, row.number]),
      [
        ["b", true, 1],
        ["a", false, null],
        ["c", false, null],
      ],
    );
  },
);

check("ticking and unticking Active keeps the order", () => {
  const order = [e("a", true), e("b", false), e("c", false)];
  assert.deepEqual(withActive(order, "c", true), [
    e("a", true),
    e("b", false),
    e("c", true),
  ]);
  assert.deepEqual(withActive(order, "a", false), [
    e("a", false),
    e("b", false),
    e("c", false),
  ]);
  // The original stays as it was.
  assert.deepEqual(order, [e("a", true), e("b", false), e("c", false)]);
});

check("dragging moves an entry to the place it is dropped", () => {
  const order = [e("a", true), e("b", false), e("c", true)];
  assert.deepEqual(moved(order, 0, 2), [
    e("b", false),
    e("c", true),
    e("a", true),
  ]);
  assert.deepEqual(moved(order, 2, 0), [
    e("c", true),
    e("a", true),
    e("b", false),
  ]);
  assert.deepEqual(moved(order, 1, 1), order);
  assert.deepEqual(moved(order, 0, 9), [
    e("b", false),
    e("c", true),
    e("a", true),
  ]);
});

check("the arrow keys move an entry by one, not past the ends", () => {
  const order = [e("a", true), e("b", false), e("c", true)];
  assert.deepEqual(movedBy(order, "b", -1), [
    e("b", false),
    e("a", true),
    e("c", true),
  ]);
  assert.deepEqual(movedBy(order, "b", 1), [
    e("a", true),
    e("c", true),
    e("b", false),
  ]);
  assert.deepEqual(movedBy(order, "a", -1), order);
  assert.deepEqual(movedBy(order, "c", 1), order);
  assert.deepEqual(movedBy(order, "gone", 1), order);
});

check("the drop place counts the other entries above the pointer", () => {
  // Rows centred at 10, 30, 60 (the middle one taller), dragging the first.
  const midpoints = [10, 30, 60];
  assert.equal(dropIndex(5, midpoints, 0), 0);
  assert.equal(dropIndex(35, midpoints, 0), 1);
  assert.equal(dropIndex(70, midpoints, 0), 2);
  // Dragging the last one up.
  assert.equal(dropIndex(20, midpoints, 2), 1);
  assert.equal(dropIndex(0, midpoints, 2), 0);
  assert.equal(dropIndex(99, midpoints, 2), 2);
});

check("post-processing needs at least one active prompt with a text", () => {
  assert.equal(hasActivePrompt(prompts, [e("b", true)]), true);
  assert.equal(hasActivePrompt(prompts, [e("a", false), e("b", false)]), false);
  assert.equal(hasActivePrompt(prompts, [e("gone", true)]), false);
  assert.equal(
    hasActivePrompt([{ id: "a", name: "Blank", prompt: "  " }], [e("a", true)]),
    false,
  );
  assert.equal(hasActivePrompt(null, [e("a", true)]), false);
  assert.equal(hasActivePrompt(prompts, undefined), false);
});

check("a name is taken by another prompt only, trimmed", () => {
  assert.equal(isNameTaken(prompts, " Mail "), true);
  assert.equal(isNameTaken(prompts, "Mail", "c"), false);
  assert.equal(isNameTaken(prompts, "mail"), false);
  assert.equal(isNameTaken(prompts, "Englisch"), false);
});

check("a prompt with ${output} runs alone while others are active too", () => {
  const library = [
    ...prompts,
    { id: "old", name: "Old", prompt: "Fix: ${output}" },
  ];
  const order = [e("a", true), e("old", true), e("b", false)];
  assert.equal(runsAlone(library, order, "old"), true);
  assert.equal(runsAlone(library, order, "a"), false);
  assert.equal(
    runsAlone(library, [e("old", true), e("a", false)], "old"),
    false,
  );
  assert.equal(
    runsAlone(library, [e("old", false), e("a", true)], "old"),
    false,
  );
});

if (failures > 0) {
  console.error(`promptLibrary: ${failures} check(s) failed`);
  process.exit(1);
}
console.log("promptLibrary: all assertions passed");
