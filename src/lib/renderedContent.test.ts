// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/renderedContent.test.ts
//
// DanyWhere: whether a setting drew anything, so its wrapper draws no
// state line under a setting this device doesn't show (a microphone's
// channel with one channel, the typing tool on macOS, the active list
// without lists; qa 2026-11-22_0107, error 3).
import assert from "node:assert/strict";
import { showsAnything, wrapperShows, type NodeLike } from "./renderedContent";

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

// The speech model's wrapper holds no setting: it stands for the model
// table next to it, so it shows its line although it draws nothing itself
// (qa 2026-11-22_0233, the speech model's state line was gone).
check("a wrapper without a setting always shows", () =>
  assert.equal(wrapperShows(false, false), true),
);
check("a wrapper around a setting that draws nothing hides", () =>
  assert.equal(wrapperShows(true, false), false),
);
check("a wrapper around a setting that draws something shows", () =>
  assert.equal(wrapperShows(true, true), true),
);

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`renderedContent: all ${total} checks passed`);
