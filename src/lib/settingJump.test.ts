// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/settingJump.test.ts
//
// DanyWhere: the jump from a profile's page "Profile" to one of its own
// values (qa 2026-11-22_1603, B5): which setting is the target, when a
// wrapper takes the jump, how it scrolls and how long it lights up.
import assert from "node:assert/strict";
import {
  HIGHLIGHT_MS,
  JUMP_WAIT_MS,
  isJumpTarget,
  jumpTarget,
  jumpWaits,
  scrollBehavior,
  scrollBlock,
  takesJump,
  type PendingJump,
} from "./settingJump";

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

// --- which setting is the target ------------------------------------------
check("an own field targets that field", () =>
  assert.deepEqual(jumpTarget("work", { field: "paste_method" }), {
    profileId: "work",
    field: "paste_method",
  }),
);
check("an own entry targets the entry of its field", () =>
  assert.deepEqual(jumpTarget("work", { field: "end_keys", entry: "end_1" }), {
    profileId: "work",
    field: "end_keys",
    entry: "end_1",
  }),
);
check("a switched-off entry targets it like an own one", () =>
  assert.deepEqual(
    jumpTarget("work", { field: "word_lists", entry: "list_1", off: true }),
    { profileId: "work", field: "word_lists", entry: "list_1" },
  ),
);

const pasteMethod = jumpTarget("work", { field: "paste_method" });
check("the wrapper of the field is the target", () =>
  assert.equal(isJumpTarget(pasteMethod, "work", ["paste_method"]), true),
);
check("a wrapper of several fields is the target of each", () =>
  assert.equal(
    isJumpTarget(
      jumpTarget("work", { field: "external_script_path" }),
      "work",
      ["paste_method", "external_script_path"],
    ),
    true,
  ),
);
check("another field's wrapper is not the target", () =>
  assert.equal(isJumpTarget(pasteMethod, "work", ["typing_tool"]), false),
);
check("the same setting of another profile is not the target", () =>
  assert.equal(isJumpTarget(pasteMethod, "home", ["paste_method"]), false),
);
check("an entry's target is the wrapper of its whole list", () => {
  assert.equal(
    isJumpTarget(
      jumpTarget("work", { field: "end_keys", entry: "end_1" }),
      "work",
      ["end_keys"],
    ),
    true,
  );
  assert.equal(
    isJumpTarget(
      jumpTarget("work", { field: "word_lists", entry: "list_1" }),
      "work",
      ["word_lists"],
    ),
    true,
  );
});
check("an entry's target is the wrapper of that entry", () =>
  assert.equal(
    isJumpTarget(
      jumpTarget("work", { field: "bindings", entry: "transcribe" }),
      "work",
      ["bindings"],
      "transcribe",
    ),
    true,
  ),
);
check("the wrapper of another entry is not the target", () =>
  assert.equal(
    isJumpTarget(
      jumpTarget("work", { field: "bindings", entry: "transcribe" }),
      "work",
      ["bindings"],
      "cancel",
    ),
    false,
  ),
);
// The provider's settings show the selected provider's entry only: another
// provider's own model has no wrapper on the page, so the page just opens.
check("another provider's entry is not the shown provider's", () =>
  assert.equal(
    isJumpTarget(
      jumpTarget("work", { field: "post_process_models", entry: "openai" }),
      "work",
      ["post_process_models"],
      "anthropic",
    ),
    false,
  ),
);

// --- when a wrapper takes the jump ----------------------------------------
const pending = (requestedAt: number): PendingJump => ({
  ...pasteMethod,
  requestedAt,
});
check("a jump waits for its setting right after the click", () =>
  assert.equal(jumpWaits(pending(1000), 1000), true),
);
check("a jump still waits at the end of its time", () =>
  assert.equal(jumpWaits(pending(1000), 1000 + JUMP_WAIT_MS), true),
);
check("a jump whose setting never came is over", () =>
  assert.equal(jumpWaits(pending(1000), 1001 + JUMP_WAIT_MS), false),
);
check("a jump from a clock that went back is over", () =>
  assert.equal(jumpWaits(pending(1000), 999), false),
);
check("the waiting time covers a page that loads, not a later visit", () =>
  assert.ok(JUMP_WAIT_MS >= 500 && JUMP_WAIT_MS <= 5000, `${JUMP_WAIT_MS}`),
);
check("the target's wrapper takes a waiting jump", () =>
  assert.equal(takesJump(pending(1000), 1100, "work", ["paste_method"]), true),
);
check("no wrapper takes a jump that isn't there", () =>
  assert.equal(takesJump(null, 1100, "work", ["paste_method"]), false),
);
check("another wrapper doesn't take it", () =>
  assert.equal(takesJump(pending(1000), 1100, "work", ["typing_tool"]), false),
);
check("an expired jump isn't taken on a later visit", () =>
  assert.equal(
    takesJump(pending(1000), 1001 + JUMP_WAIT_MS, "work", ["paste_method"]),
    false,
  ),
);

// --- how it scrolls and lights up -------------------------------------------
check("a setting that fits lands in the middle", () =>
  assert.equal(scrollBlock(120, 600), "center"),
);
check("a setting as tall as the view lands in the middle", () =>
  assert.equal(scrollBlock(600, 600), "center"),
);
check("a taller setting lands with its top at the top", () =>
  assert.equal(scrollBlock(900, 600), "start"),
);
check("it scrolls smoothly", () =>
  assert.equal(scrollBehavior(false), "smooth"),
);
check("with less motion asked for, it jumps without scrolling smoothly", () =>
  assert.equal(scrollBehavior(true), "auto"),
);
check("it lights up for about 1.5 seconds", () =>
  assert.ok(HIGHLIGHT_MS >= 1200 && HIGHLIGHT_MS <= 2000, `${HIGHLIGHT_MS}`),
);

if (failures.length > 0) {
  console.error(
    `settingJump.test.ts: ${failures.length} of ${total} checks failed`,
  );
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`settingJump.test.ts: all ${total} checks passed`);
