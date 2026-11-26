// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/profileFields.test.ts
//
// DanyWhere: every value of a profile has its page and a name to show
// (qa 2026-11-19_1711 P5, P8), checked against the generated bindings and
// the English strings.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { DEFAULTS_PAGES } from "./navigation";
import {
  PROFILE_FIELDS,
  PROFILE_FIELD_NAMES,
  fieldInfo,
  fieldsOnPage,
  pageOfValue,
  resetPlan,
} from "./profileFields";

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

const read = (path: string) =>
  readFileSync(new URL(path, import.meta.url), "utf8");

const english = JSON.parse(read("../i18n/locales/en/translation.json"));

const lookup = (key: string): unknown =>
  key
    .split(".")
    .reduce<unknown>(
      (node, part) =>
        node && typeof node === "object"
          ? (node as Record<string, unknown>)[part]
          : undefined,
      english,
    );

/** The field names of `ProfileValues` in the generated bindings. */
const bindingFields = (): string[] => {
  const source = read("../bindings.ts");
  const start = source.indexOf("export type ProfileValues = {");
  assert.ok(start >= 0, "ProfileValues not found in bindings.ts");
  let depth = 0;
  let end = start;
  for (let i = source.indexOf("{", start); i < source.length; i += 1) {
    if (source[i] === "{") depth += 1;
    if (source[i] === "}") depth -= 1;
    if (depth === 0) {
      end = i;
      break;
    }
  }
  const body = source
    .slice(source.indexOf("{", start) + 1, end)
    .replace(/\/\*\*[\s\S]*?\*\//g, "");
  // Top-level names only: drop nested object types first.
  let flat = "";
  depth = 0;
  for (const char of body) {
    if (char === "{") depth += 1;
    if (depth === 0) flat += char;
    if (char === "}") depth -= 1;
  }
  return [...flat.matchAll(/(?:^|;)\s*([a-z_]+)\??:/g)].map((m) => m[1]);
};

check("every field of ProfileValues has a row, and no other", () => {
  assert.deepEqual(
    [...PROFILE_FIELD_NAMES].sort(),
    [...new Set(bindingFields())].sort(),
  );
});

check("every row is on a page of the defaults", () => {
  for (const [field, info] of Object.entries(PROFILE_FIELDS)) {
    assert.ok(
      (DEFAULTS_PAGES as readonly string[]).includes(info.page),
      `${field}: ${info.page}`,
    );
  }
});

check("every name is an English string without placeholders", () => {
  for (const [field, info] of Object.entries(PROFILE_FIELDS)) {
    const text = lookup(info.label);
    assert.equal(typeof text, "string", `${field}: ${info.label}`);
    assert.ok(!String(text).includes("{{"), `${field}: ${text}`);
  }
});

check(
  "start, hold and end shortcuts are on General, the old model is gone",
  () => {
    assert.equal(pageOfValue("start_keys"), "general");
    assert.equal(pageOfValue("hold_keys"), "general");
    assert.equal(pageOfValue("end_keys"), "general");
    assert.equal(pageOfValue("bindings", "paste_last_transcript"), "general");
    assert.equal(pageOfValue("bindings"), "general");
    // The lists are large: under the defaults a deviating profile shows a
    // summary with "Open in …".
    assert.equal(fieldInfo("start_keys")?.large, true);
    assert.equal(fieldInfo("hold_keys")?.large, true);
    assert.equal(lookup(fieldInfo("start_keys")!.label), "Start Shortcuts");
    assert.equal(lookup(fieldInfo("hold_keys")!.label), "Hold Shortcuts");
    assert.equal(fieldInfo("shortcut_activation"), null);
    assert.equal(fieldInfo("hold_ends"), null);
  },
);

// The fourth list of the same shape, and the post-processing mode per
// profile that replaces the switch (qa 2026-11-26_1810, "Schnittstelle
// Phase 2").
check(
  "instruction shortcuts are on General, the post-processing mode on Advanced",
  () => {
    assert.equal(pageOfValue("instruction_keys"), "general");
    assert.equal(fieldInfo("instruction_keys")?.large, true);
    assert.equal(
      lookup(fieldInfo("instruction_keys")!.label),
      "Instruction Shortcuts",
    );
    assert.equal(pageOfValue("post_process_mode"), "advanced");
    // The name the switch had (docs/manual/STYLE.md, glossary).
    assert.equal(
      lookup(fieldInfo("post_process_mode")!.label),
      "Post Processing",
    );
    assert.equal(
      lookup("settings.postProcessing.mode.options.withInstructionKey"),
      "With Instruction Shortcut",
    );
    assert.equal(fieldInfo("post_process_enabled"), null);
  },
);

check("fields by page", () => {
  assert.equal(pageOfValue("selected_model"), "models");
  assert.ok(fieldsOnPage("advanced").includes("paste_method"));
  assert.ok(fieldsOnPage("postprocessing").includes("post_process_models"));
  assert.ok(!fieldsOnPage("general").includes("paste_method"));
});

check("the pages hold every field once", () => {
  const placed = DEFAULTS_PAGES.flatMap((page) => fieldsOnPage(page));
  assert.equal(placed.length, PROFILE_FIELD_NAMES.size);
  assert.deepEqual(new Set(placed), new Set(PROFILE_FIELD_NAMES));
});

check("a name that is no profile value has no row", () => {
  assert.equal(fieldInfo("theme"), null);
  assert.equal(fieldInfo("toString"), null);
});

check("page reset: whole fields, and entries placed apart one by one", () => {
  const own = [
    { field: "paste_method" },
    { field: "selected_model" },
    { field: "bindings", entry: "paste_last_transcript" },
    { field: "bindings", entry: "copy_last_transcript" },
    { field: "word_lists", entry: "a" },
    { field: "word_lists", entry: "b", off: true },
  ];
  // Every shortcut is on General now, so its page resets them as a whole.
  assert.deepEqual(resetPlan(own, "general"), {
    fields: ["bindings"],
    entries: [],
    count: 2,
  });
  assert.deepEqual(resetPlan(own, "postprocessing"), {
    fields: [],
    entries: [],
    count: 0,
  });
  assert.deepEqual(resetPlan(own, "advanced"), {
    fields: ["paste_method", "word_lists"],
    entries: [],
    count: 3,
  });
  assert.deepEqual(resetPlan(own, "models"), {
    fields: ["selected_model"],
    entries: [],
    count: 1,
  });
});

console.log(`${total - failures.length}/${total} checks passed`);
if (failures.length > 0) {
  for (const failure of failures) console.error(`FAIL ${failure}`);
  process.exit(1);
}
