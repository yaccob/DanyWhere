// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/profileScope.test.ts
//
// DanyWhere: the defaults and the profiles that override them, as the
// settings window reads them (qa 2026-11-19_1711 P4, P5, P7, P8, P10): a
// value's state in a profile (coupled, own, own and equal), entry by entry
// for lists, the own values and their count, who deviates where, the
// settings a page reads in a scope and what a change there writes.
import assert from "node:assert/strict";
import {
  DEFAULTS_SCOPE,
  becomesOwn,
  combinedState,
  deviation,
  entriesOf,
  entryLabel,
  entryState,
  fieldState,
  ownCount,
  ownCountOnPage,
  ownValues,
  patchScope,
  profileWrites,
  sameScope,
  sameValue,
  scopeValues,
  scopedSettings,
  valueText,
  type FieldKind,
  type LayeredProfile,
} from "./profileScope";

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

const VALUE: FieldKind = { kind: "value" };
const LIST: FieldKind = { kind: "list", key: "id" };
const MAP: FieldKind = { kind: "map" };

const defaults: Record<string, unknown> = {
  paste_method: "ctrl_v",
  append_text: "",
  filler_word_removal_enabled: true,
  word_lists: [
    { id: "a", name: "Names", words: ["Ada"] },
    { id: "b", name: "Places", words: ["Graz"] },
  ],
  post_process_models: { anthropic: "haiku", openai: "gpt" },
};

const work: LayeredProfile = {
  id: "work",
  name: "Arbeit",
  values: {
    paste_method: "ctrl_v",
    append_text: "\n\n",
    filler_word_removal_enabled: true,
    word_lists: [
      { id: "a", name: "Names", words: ["Ada", "Grace"] },
      { id: "c", name: "Work", words: ["Kubernetes"] },
    ],
    post_process_models: { anthropic: "sonnet", openai: "gpt" },
  },
  own: ["append_text", "filler_word_removal_enabled"],
  own_entries: {
    word_lists: { own: ["a", "c"], off: ["b"] },
    post_process_models: { own: ["anthropic"], off: [] },
  },
};

const home: LayeredProfile = {
  id: "home",
  name: "Privat",
  values: { ...defaults },
};

const play: LayeredProfile = {
  id: "play",
  name: "Playground",
  values: { ...defaults, paste_method: "direct" },
  own: ["paste_method"],
};

// ---- comparing values ----------------------------------------------------

check("same value ignores the order of an object's keys", () => {
  assert.equal(sameValue({ a: 1, b: [1, 2] }, { b: [1, 2], a: 1 }), true);
});

check("same value tells arrays in another order apart", () => {
  assert.equal(sameValue([1, 2], [2, 1]), false);
});

check("same value: null and undefined are both nothing", () => {
  assert.equal(sameValue(null, undefined), true);
  assert.equal(sameValue("", null), false);
});

check("entries of a list by its key, in order", () => {
  assert.deepEqual(
    entriesOf(LIST, defaults.word_lists).map(([id]) => id),
    ["a", "b"],
  );
});

check("entries of a map by key", () => {
  assert.deepEqual(entriesOf(MAP, defaults.post_process_models), [
    ["anthropic", "haiku"],
    ["openai", "gpt"],
  ]);
});

check("entries of a plain value: none", () => {
  assert.deepEqual(entriesOf(VALUE, "x"), []);
});

// ---- the state of a value ------------------------------------------------

check("a field the profile doesn't hold is coupled", () => {
  assert.equal(fieldState(work, defaults, "paste_method", VALUE), "coupled");
});

check("an own field that differs is own", () => {
  assert.equal(fieldState(work, defaults, "append_text", VALUE), "own");
});

check("an own field that equals the default is own and equal", () => {
  assert.equal(
    fieldState(work, defaults, "filler_word_removal_enabled", VALUE),
    "own_equal",
  );
});

check("a list with own entries is own", () => {
  assert.equal(fieldState(work, defaults, "word_lists", LIST), "own");
});

check("a list without own entries is coupled", () => {
  assert.equal(fieldState(home, defaults, "word_lists", LIST), "coupled");
});

check("a profile without `own` fields (before A1) is coupled", () => {
  assert.equal(fieldState(home, defaults, "paste_method", VALUE), "coupled");
});

check("an entry the profile changed is own", () => {
  assert.equal(entryState(work, defaults, "word_lists", LIST, "a"), "own");
});

check("an entry the profile added is added", () => {
  assert.equal(entryState(work, defaults, "word_lists", LIST, "c"), "added");
});

check("an entry of the defaults the profile switched off is off", () => {
  assert.equal(entryState(work, defaults, "word_lists", LIST, "b"), "off");
});

check("a map entry the profile follows is coupled", () => {
  assert.equal(
    entryState(work, defaults, "post_process_models", MAP, "openai"),
    "coupled",
  );
});

check("an own map entry equal to the default is own and equal", () => {
  const held: LayeredProfile = {
    ...home,
    own_entries: { post_process_models: { own: ["openai"] } },
  };
  assert.equal(
    entryState(held, defaults, "post_process_models", MAP, "openai"),
    "own_equal",
  );
});

check("several fields: own wins over own and equal over coupled", () => {
  assert.equal(combinedState(["coupled", "own_equal", "own"]), "own");
  assert.equal(combinedState(["coupled", "own_equal"]), "own_equal");
  assert.equal(combinedState(["coupled", "coupled"]), "coupled");
  assert.equal(combinedState([]), "coupled");
});

check("changing a coupled value makes it own (P7.3 bar)", () => {
  assert.equal(becomesOwn(work, "paste_method", VALUE), true);
  assert.equal(becomesOwn(work, "append_text", VALUE), false);
});

check("changing a coupled entry makes it own, an own one not", () => {
  assert.equal(becomesOwn(work, "post_process_models", MAP, "openai"), true);
  assert.equal(
    becomesOwn(work, "post_process_models", MAP, "anthropic"),
    false,
  );
});

check("changing a whole list with no own entries makes it own", () => {
  assert.equal(becomesOwn(home, "word_lists", LIST), true);
  assert.equal(becomesOwn(work, "word_lists", LIST), false);
});

// ---- own values and counts (P5) ------------------------------------------

check("own values: own fields, then own and switched-off entries", () => {
  assert.deepEqual(ownValues(work), [
    { field: "append_text" },
    { field: "filler_word_removal_enabled" },
    { field: "post_process_models", entry: "anthropic" },
    { field: "word_lists", entry: "a" },
    { field: "word_lists", entry: "c" },
    { field: "word_lists", entry: "b", off: true },
  ]);
});

check("own count as the backend's own_count", () => {
  assert.equal(ownCount(work), 6);
  assert.equal(ownCount(home), 0);
  assert.equal(ownCount(play), 1);
});

check("own count per page", () => {
  const pageOf = (field: string) =>
    field === "post_process_models" ? "postprocessing" : "advanced";
  assert.equal(ownCountOnPage(work, "advanced", pageOf), 5);
  assert.equal(ownCountOnPage(work, "postprocessing", pageOf), 1);
  assert.equal(ownCountOnPage(work, "general", pageOf), 0);
});

check("own count per page can place entries of one field apart", () => {
  const pageOf = (field: string, entry?: string) =>
    field === "word_lists" && entry === "c" ? "general" : "advanced";
  assert.equal(ownCountOnPage(work, "general", pageOf), 1);
  assert.equal(ownCountOnPage(work, "advanced", pageOf), 5);
});

// ---- who deviates (P7.1) -------------------------------------------------

check("deviation: the profiles holding a value, and those following", () => {
  const { own, following } = deviation([work, home, play], "paste_method");
  assert.deepEqual(
    own.map((p) => p.id),
    ["play"],
  );
  assert.deepEqual(
    following.map((p) => p.id),
    ["work", "home"],
  );
});

check("deviation of a list: profiles with any own entry", () => {
  const { own } = deviation([work, home, play], "word_lists");
  assert.deepEqual(
    own.map((p) => p.id),
    ["work"],
  );
});

check("deviation of one entry", () => {
  const anthropic = deviation([work, home], "post_process_models", "anthropic");
  assert.deepEqual(
    anthropic.own.map((p) => p.id),
    ["work"],
  );
  const openai = deviation([work, home], "post_process_models", "openai");
  assert.deepEqual(openai.own, []);
});

check("deviation of a switched-off entry counts it", () => {
  const { own } = deviation([work, home], "word_lists", "b");
  assert.deepEqual(
    own.map((p) => p.id),
    ["work"],
  );
});

// ---- scopes ----------------------------------------------------------------

check("scopes compare by kind and id", () => {
  assert.equal(sameScope(DEFAULTS_SCOPE, { kind: "defaults" }), true);
  assert.equal(
    sameScope({ kind: "profile", id: "a" }, { kind: "profile", id: "a" }),
    true,
  );
  assert.equal(
    sameScope({ kind: "profile", id: "a" }, { kind: "profile", id: "b" }),
    false,
  );
  assert.equal(sameScope(DEFAULTS_SCOPE, { kind: "profile", id: "a" }), false);
});

check("the values of the defaults", () => {
  assert.equal(
    scopeValues({ default_values: defaults, profiles: [] }, DEFAULTS_SCOPE),
    defaults,
  );
});

check("the values of a profile", () => {
  assert.equal(
    scopeValues({ profiles: [work] }, { kind: "profile", id: "work" }),
    work.values,
  );
});

check("no values for an unknown profile or missing defaults", () => {
  assert.equal(
    scopeValues({ profiles: [work] }, { kind: "profile", id: "x" }),
    null,
  );
  assert.equal(scopeValues({ profiles: [] }, DEFAULTS_SCOPE), null);
});

check("scoped settings: the scope's values over the flat ones", () => {
  const flat = {
    paste_method: "direct",
    append_text: "x",
    theme: "dark",
    dictation_context: {
      age_limit_minutes: 1,
      max_entries: 1,
      char_budget: 1,
      excluded_apps: [],
    },
  };
  const scoped = scopedSettings(flat, {
    paste_method: "ctrl_v",
    context_max_entries: 7,
    context_excluded_apps: ["mail"],
  });
  assert.equal(scoped.paste_method, "ctrl_v");
  assert.equal(scoped.append_text, "x");
  assert.equal(scoped.theme, "dark");
  assert.deepEqual(scoped.dictation_context, {
    age_limit_minutes: 1,
    max_entries: 7,
    char_budget: 1,
    excluded_apps: ["mail"],
  });
  // The flat settings stay as they were.
  assert.equal(flat.paste_method, "direct");
  assert.equal(flat.dictation_context.max_entries, 1);
});

// ---- what a change writes ------------------------------------------------

const FIELDS = new Set([
  "paste_method",
  "context_age_limit_minutes",
  "context_max_entries",
  "context_char_budget",
  "context_excluded_apps",
]);

check("a profile value writes itself", () => {
  assert.deepEqual(profileWrites("paste_method", "direct", {}, FIELDS), [
    ["paste_method", "direct"],
  ]);
});

check("a setting that is no profile value writes nothing here", () => {
  assert.equal(profileWrites("theme", "dark", {}, FIELDS), null);
});

check("the context settings write the parts that changed", () => {
  const before = {
    dictation_context: {
      age_limit_minutes: 5,
      max_entries: 3,
      char_budget: 100,
      excluded_apps: ["a"],
    },
  };
  assert.deepEqual(
    profileWrites(
      "dictation_context",
      {
        age_limit_minutes: 5,
        max_entries: 4,
        char_budget: 100,
        excluded_apps: ["a", "b"],
      },
      before,
      FIELDS,
    ),
    [
      ["context_max_entries", 4],
      ["context_excluded_apps", ["a", "b"]],
    ],
  );
});

// ---- an optimistic change ------------------------------------------------

check("patching the defaults changes only their value", () => {
  const settings = { default_values: { ...defaults }, profiles: [work] };
  const patched = patchScope(settings, DEFAULTS_SCOPE, "paste_method", "none");
  assert.equal(patched.default_values?.paste_method, "none");
  assert.equal(settings.default_values.paste_method, "ctrl_v");
  assert.equal(patched.profiles, settings.profiles);
});

check("patching a profile changes only its value", () => {
  const settings = { default_values: defaults, profiles: [work, home] };
  const patched = patchScope(
    settings,
    { kind: "profile", id: "home" },
    "paste_method",
    "none",
  );
  assert.equal(patched.profiles?.[1].values.paste_method, "none");
  assert.equal(patched.profiles?.[0], work);
  assert.equal(home.values.paste_method, "ctrl_v");
});

check("patching an unknown profile changes nothing", () => {
  const settings = { default_values: defaults, profiles: [work] };
  assert.equal(
    patchScope(settings, { kind: "profile", id: "x" }, "paste_method", "none"),
    settings,
  );
});

// ---- showing a value -----------------------------------------------------

check("value text: on and off", () => {
  assert.deepEqual(valueText(true), { key: "on" });
  assert.deepEqual(valueText(false), { key: "off" });
});

check("value text: nothing for empty and null", () => {
  assert.deepEqual(valueText(""), { key: "empty" });
  assert.deepEqual(valueText(null), { key: "empty" });
  assert.deepEqual(valueText([]), { key: "empty" });
});

check("value text: control characters as escapes", () => {
  assert.deepEqual(valueText("\n\n"), { text: "\\n\\n" });
  assert.deepEqual(valueText("a\tb"), { text: "a\\tb" });
});

check("value text: numbers and short lists as they are", () => {
  assert.deepEqual(valueText(2500), { text: "2500" });
  assert.deepEqual(valueText(["de", "en"]), { text: "de, en" });
});

check("value text: long text is cut", () => {
  const long = "x".repeat(80);
  const shown = valueText(long);
  assert.ok("text" in shown && shown.text.length <= 41);
  assert.ok("text" in shown && shown.text.endsWith("…"));
});

check("value text: lists of objects as a count", () => {
  assert.deepEqual(valueText([{ id: "a" }, { id: "b" }]), {
    key: "items",
    count: 2,
  });
});

check("entry label: its name, else its label, else the id", () => {
  assert.equal(entryLabel({ id: "a", name: "Names" }, "a"), "Names");
  assert.equal(entryLabel({ id: "x", label: "OpenAI" }, "x"), "OpenAI");
  assert.equal(entryLabel({ id: "x", name: "" }, "x"), "x");
  assert.equal(entryLabel("haiku", "anthropic"), "anthropic");
});

console.log(`${total - failures.length}/${total} checks passed`);
if (failures.length > 0) {
  for (const failure of failures) console.error(`FAIL ${failure}`);
  process.exit(1);
}
