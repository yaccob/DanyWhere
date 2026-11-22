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
  openInTarget,
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

// --- "Open in …" under a default (qa 2026-11-22_1638) -----------------------
// The row of a deviating profile under a default jumps to the same setting
// in that profile: the wrapper of the same fields (and entry) there.
check("Open in … of one field targets that field", () =>
  assert.deepEqual(openInTarget("work", ["selected_model"], undefined, []), {
    profileId: "work",
    field: "selected_model",
  }),
);
check("Open in … of an entry targets that entry", () =>
  assert.deepEqual(
    openInTarget("work", ["post_process_models"], "anthropic", [
      "post_process_models",
    ]),
    { profileId: "work", field: "post_process_models", entry: "anthropic" },
  ),
);
check("Open in … of several fields targets the profile's own one", () =>
  assert.deepEqual(
    openInTarget("work", ["paste_method", "external_script_path"], undefined, [
      "external_script_path",
    ]),
    { profileId: "work", field: "external_script_path" },
  ),
);
check("Open in … of several fields without an own one takes the first", () =>
  assert.deepEqual(
    openInTarget(
      "work",
      ["paste_method", "external_script_path"],
      undefined,
      [],
    ),
    { profileId: "work", field: "paste_method" },
  ),
);
const takenBySameWrapper = (
  fields: readonly string[],
  entry: string | undefined,
  own: readonly string[],
) => {
  const target = openInTarget("work", fields, entry, own);
  return target !== null && isJumpTarget(target, "work", fields, entry);
};
check("the profile's wrapper of the same field takes it", () =>
  assert.equal(takenBySameWrapper(["selected_model"], undefined, []), true),
);
check("the profile's wrapper of the active prompts takes it", () =>
  assert.equal(
    takenBySameWrapper(["post_process_prompt_order"], undefined, [
      "post_process_prompt_order",
    ]),
    true,
  ),
);
check("the profile's wrapper of the whole list takes it", () =>
  assert.equal(takenBySameWrapper(["end_keys"], undefined, ["end_keys"]), true),
);
check("the profile's wrapper of the same entry takes it", () =>
  assert.equal(
    takenBySameWrapper(["post_process_api_keys"], "openai", [
      "post_process_api_keys",
    ]),
    true,
  ),
);
check("the profile's wrapper of several fields takes it", () =>
  assert.equal(
    takenBySameWrapper(
      ["filler_word_removal_enabled", "custom_filler_words"],
      undefined,
      ["custom_filler_words"],
    ),
    true,
  ),
);
// The provider's settings show the profile's selected provider only: one
// that selects another provider has no wrapper of this entry, so the page
// just opens, as before.
check("Open in … of an entry isn't taken by another entry's wrapper", () => {
  const target = openInTarget("work", ["post_process_models"], "anthropic", [
    "post_process_models",
  ]);
  assert.equal(
    target !== null &&
      isJumpTarget(target, "work", ["post_process_models"], "openai"),
    false,
  );
});
check("Open in … isn't taken by another profile's wrapper", () => {
  const target = openInTarget("work", ["selected_model"], undefined, []);
  assert.equal(
    target !== null && isJumpTarget(target, "home", ["selected_model"]),
    false,
  );
});

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
