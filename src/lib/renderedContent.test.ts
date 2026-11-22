// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/renderedContent.test.ts
//
// DanyWhere: whether a setting drew anything, so its wrapper draws no
// state line under a setting this device doesn't show (a microphone's
// channel with one channel, the typing tool on macOS, the active list
// without lists; qa 2026-11-22_0107, error 3).
import assert from "node:assert/strict";
import { showsAnything, type NodeLike } from "./renderedContent";

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

const element = (text = ""): NodeLike => ({ nodeType: 1, textContent: text });
const text = (content: string): NodeLike => ({
  nodeType: 3,
  textContent: content,
});
const comment = (content: string): NodeLike => ({
  nodeType: 8,
  textContent: content,
});

check("nothing rendered shows nothing", () =>
  assert.equal(showsAnything([]), false),
);
check("white space alone shows nothing", () =>
  assert.equal(showsAnything([text("  \n ")]), false),
);
check("a comment shows nothing", () =>
  assert.equal(showsAnything([comment("hidden")]), false),
);
check("an element shows something, even an empty one", () =>
  assert.equal(showsAnything([element()]), true),
);
check("text shows something", () =>
  assert.equal(showsAnything([text("Kanal")]), true),
);
check("one element among white space shows something", () =>
  assert.equal(showsAnything([text(" "), element("Kanal"), text(" ")]), true),
);

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`renderedContent: all ${total} checks passed`);
