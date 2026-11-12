import assert from "node:assert/strict";
import {
  DEFAULT_CONTEXT_SETTINGS,
  LIMITS,
  filled,
  firstWords,
  inWindow,
  parseExcludedApps,
  parseLimit,
  windowFilterOf,
  windowLabel,
} from "./dictationContext";

let failures = 0;
const check = (name: string, run: () => void) => {
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

check("an older store gets the defaults", () => {
  assert.deepEqual(filled(undefined), DEFAULT_CONTEXT_SETTINGS);
  assert.deepEqual(filled({ age_limit_minutes: 30 }), {
    ...DEFAULT_CONTEXT_SETTINGS,
    age_limit_minutes: 30,
  });
});

check("a limit must be a whole number in range", () => {
  assert.equal(parseLimit("90", LIMITS.age_limit_minutes), 90);
  assert.equal(parseLimit(" 5 ", LIMITS.max_entries), 5);
  assert.equal(parseLimit("0", LIMITS.age_limit_minutes), null);
  assert.equal(parseLimit("21", LIMITS.max_entries), null);
  assert.equal(parseLimit("2.5", LIMITS.max_entries), null);
  assert.equal(parseLimit("", LIMITS.char_budget), null);
  assert.equal(parseLimit("abc", LIMITS.char_budget), null);
});

check("the exclusion list is one app per line", () => {
  assert.deepEqual(parseExcludedApps("1Password\n\n  Bitwarden  \n"), [
    "1Password",
    "Bitwarden",
  ]);
  assert.deepEqual(parseExcludedApps(""), []);
});

check("History shows app and window title", () => {
  const context = {
    window_app_id: "com.googlecode.iterm2",
    window_app_name: "iTerm2",
    window_title: "Release 0.1.4",
    context_requested: false,
    context_entry_ids: [],
    sent_as_context_at: null,
    word_replacements: [],
  };
  assert.equal(windowLabel(context), "iTerm2 – Release 0.1.4");
  assert.equal(
    windowLabel({ ...context, window_app_name: null }),
    "com.googlecode.iterm2 – Release 0.1.4",
  );
  assert.equal(windowLabel({ ...context, window_title: null }), null);
  assert.equal(windowLabel(undefined), null);
});

const iterm = {
  window_app_id: "com.googlecode.iterm2",
  window_app_name: "iTerm2",
  window_title: "Release 0.1.4",
  context_requested: false,
  context_entry_ids: [],
  sent_as_context_at: null,
  word_replacements: [],
};

check("an entry's window becomes a filter", () => {
  assert.deepEqual(windowFilterOf(iterm), {
    app_id: "com.googlecode.iterm2",
    title: "Release 0.1.4",
    label: "iTerm2 – Release 0.1.4",
  });
  assert.equal(windowFilterOf({ ...iterm, window_title: null }), null);
  assert.equal(windowFilterOf({ ...iterm, window_app_id: null }), null);
  assert.equal(windowFilterOf(undefined), null);
});

check("the filter matches app and title exactly", () => {
  const filter = windowFilterOf(iterm)!;
  assert.equal(inWindow(iterm, filter), true);
  assert.equal(inWindow({ ...iterm, window_title: "Release" }, filter), false);
  assert.equal(
    inWindow({ ...iterm, window_app_id: "com.apple.Terminal" }, filter),
    false,
  );
  assert.equal(inWindow(undefined, filter), false);
});

check("a preview shows the first words", () => {
  assert.equal(firstWords("eins zwei drei vier", 2), "eins zwei …");
  assert.equal(firstWords("  eins   zwei ", 5), "eins zwei");
  assert.equal(firstWords("", 3), "");
});

if (failures > 0) {
  console.error(`dictationContext: ${failures} check(s) failed`);
  process.exit(1);
}
console.log("dictationContext: all assertions passed");
