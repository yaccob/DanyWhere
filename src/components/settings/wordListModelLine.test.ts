// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/wordListModelLine.test.ts
// DanyWhere (word lists, stage 1a, 2026-11-22): the word list pages name the
// speech model in effect there and say whether it takes the word lists while
// it recognizes speech, or leaves them to post-processing as known terms;
// the model table shows it as a capability. The user's wish: tell word lists
// the model can use apart from those it can't (sidecar
// qa/2026-11-22_2145_wortlisten-je-modell.md).
//
// The LLM check (2026-11-24, sidecar
// qa/2026-11-24_0130_wortlisten-llm-pruefung.md): for a model without
// vocabulary the line says whether the lists work through post-processing
// or, without a provider, not at all; the switch `word_list_llm_check`
// needs a provider with a model that isn't Apple Intelligence, as the
// backend's `word_list_check::wanted` does.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import type { AppSettings } from "../../bindings";
import { wordListCheckSetup, wordListModelLine } from "./wordListModelLine";

const HERE = join(fileURLToPath(import.meta.url), "..");
const source = (file: string) => readFileSync(join(HERE, file), "utf8");
const english = JSON.parse(
  readFileSync(join(HERE, "../../i18n/locales/en/translation.json"), "utf8"),
);

let checks = 0;
const failures: string[] = [];
const check = (name: string, run: () => void) => {
  checks += 1;
  try {
    run();
  } catch (error) {
    failures.push(`${name}: ${(error as Error).message.split("\n")[0]}`);
  }
};

const LISTS = "settings.advanced.wordLists";
const TURBO = { name: "Whisper Turbo", supports_vocabulary: true };
const CANARY = { name: "Canary 1B v2", supports_vocabulary: false };

// --- the line ------------------------------------------------------------
check("a model with vocabulary uses the word lists", () => {
  for (const ready of [true, false]) {
    assert.deepEqual(wordListModelLine(TURBO, ready), {
      key: `${LISTS}.modelUses`,
      model: "Whisper Turbo",
      state: "vocabulary",
    });
  }
});

check(
  "without vocabulary but with a provider, post-processing gets them",
  () => {
    assert.deepEqual(wordListModelLine(CANARY, true), {
      key: `${LISTS}.modelIgnores`,
      model: "Canary 1B v2",
      state: "postProcessing",
    });
  },
);

check("without vocabulary and without a provider, they have no effect", () => {
  assert.deepEqual(wordListModelLine(CANARY, false), {
    key: `${LISTS}.modelIgnoresNoPostProcess`,
    model: "Canary 1B v2",
    state: "noEffect",
  });
});

check("without a model there is no line", () => {
  assert.equal(wordListModelLine(undefined, true), null);
  assert.equal(wordListModelLine(undefined, false), null);
});

// --- when the LLM check can run --------------------------------------------
const OPENAI = {
  id: "openai",
  label: "OpenAI",
  base_url: "https://api.openai.com/v1",
};
const CUSTOM = {
  id: "custom",
  label: "Custom",
  base_url: "http://localhost:11434/v1",
};
const APPLE = {
  id: "apple_intelligence",
  label: "Apple Intelligence",
  base_url: "",
};
const PROVIDERS = [OPENAI, CUSTOM, APPLE];

const settings = (
  providerId: string,
  models: Record<string, string>,
  extra: Partial<AppSettings> = {},
): Partial<AppSettings> => ({
  post_process_provider_id: providerId,
  post_process_providers: PROVIDERS,
  post_process_models: models,
  ...extra,
});

check("a provider with a model is ready, and named", () => {
  assert.deepEqual(
    wordListCheckSetup(settings("openai", { openai: "gpt-5-mini" })),
    { kind: "ready", provider: OPENAI },
  );
  assert.deepEqual(
    wordListCheckSetup(settings("custom", { custom: "llama3.2" })),
    { kind: "ready", provider: CUSTOM },
  );
});

check("neither an instruction shortcut nor a prompt matters", () => {
  for (const mode of ["with_instruction_key", "always"] as const) {
    assert.equal(
      wordListCheckSetup(
        settings(
          "openai",
          { openai: "gpt-5-mini" },
          {
            post_process_mode: mode,
            post_process_prompts: [],
            post_process_prompt_order: [],
          },
        ),
      ).kind,
      "ready",
      mode,
    );
  }
});

// With post-processing set to "never" nothing goes to a provider, the check
// neither (qa 2026-11-26_1841, "Kleinere Folgen … ohne Frage").
check("with post-processing never the check can't run", () => {
  assert.deepEqual(
    wordListCheckSetup(
      settings(
        "openai",
        { openai: "gpt-5-mini" },
        { post_process_mode: "never" },
      ),
    ),
    { kind: "never" },
  );
  assert.ok(
    typeof english.settings.advanced.wordLists.llmCheckNever === "string",
    "its text",
  );
});

check("a provider without a model is missing", () => {
  const without: Record<string, string>[] = [
    {},
    { openai: "" },
    { openai: "  " },
  ];
  for (const models of without) {
    assert.deepEqual(wordListCheckSetup(settings("openai", models)), {
      kind: "missing",
    });
  }
  // Another provider's model doesn't count.
  assert.deepEqual(
    wordListCheckSetup(settings("openai", { custom: "llama3.2" })),
    { kind: "missing" },
  );
});

check("a provider that isn't in the list is missing", () => {
  assert.deepEqual(wordListCheckSetup(settings("gone", { gone: "model" })), {
    kind: "missing",
  });
});

check("Apple Intelligence can't take the request", () => {
  assert.deepEqual(
    wordListCheckSetup(
      settings("apple_intelligence", {
        apple_intelligence: "apple-intelligence",
      }),
    ),
    { kind: "unsupported", provider: APPLE },
  );
  assert.deepEqual(wordListCheckSetup(settings("apple_intelligence", {})), {
    kind: "unsupported",
    provider: APPLE,
  });
});

check("no settings, nothing ready", () => {
  assert.deepEqual(wordListCheckSetup(null), { kind: "missing" });
  assert.deepEqual(wordListCheckSetup(undefined), { kind: "missing" });
  assert.deepEqual(wordListCheckSetup({}), { kind: "missing" });
});

// --- the strings and where they show --------------------------------------
check("the three lines exist in English and name the model", () => {
  const lists = english.settings.advanced.wordLists;
  for (const key of [
    "modelUses",
    "modelIgnores",
    "modelIgnoresNoPostProcess",
  ]) {
    assert.equal(typeof lists[key], "string", key);
    assert.ok(lists[key].includes("{{model}}"), key);
  }
});

check("the switch, its notes and the buttons exist in English", () => {
  const lists = english.settings.advanced.wordLists;
  const placeholders: Record<string, string[]> = {
    llmCheckTitle: [],
    llmCheckDescription: ["{{provider}}"],
    llmCheckNeedsProvider: [],
    llmCheckNotWith: ["{{provider}}"],
    setUpPostProcessing: [],
    switchModel: ["{{model}}"],
    switchModelNote: [],
  };
  for (const [key, expected] of Object.entries(placeholders)) {
    assert.equal(typeof lists[key], "string", key);
    assert.deepEqual(lists[key].match(/\{\{[^}]*\}\}/g) ?? [], expected, key);
  }
});

check("the word list group shows the line", () => {
  const page = source("advanced/AdvancedSettings.tsx");
  assert.ok(page.includes("<WordListModelHint"), "AdvancedSettings");
});

check("the switch is a row of its own in the word list group", () => {
  const page = source("advanced/AdvancedSettings.tsx");
  assert.match(
    page,
    /<ScopedSetting field="word_list_llm_check">\s*<WordListLlmCheck\b/,
  );
});

check("the model table shows it as a capability", () => {
  const capabilities = source("../shared/modelTable/capabilities.ts");
  assert.match(capabilities, /wordLists: \{ Icon: \w+, flag: "vocabulary" \}/);
  const table = source("../shared/modelTable/tableModel.ts");
  assert.ok(table.includes("vocabulary: info.supports_vocabulary"));
  const tags = english.modelTable.tags;
  assert.equal(typeof tags.wordLists, "string");
  assert.equal(typeof tags.wordListsTitle, "string");
});

if (failures.length) {
  console.error(`${failures.length} of ${checks} checks failed:`);
  for (const failure of failures) console.error(`  ${failure}`);
  process.exit(1);
}
console.log(`${checks} of ${checks} checks passed`);
