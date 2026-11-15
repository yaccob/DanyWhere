// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/PostProcessingSettingsApi/modelAutoFetch.test.ts
import assert from "node:assert";
import {
  forgetLoadedAttempt,
  isProviderReadyForModelList,
  modelFetchAttemptKey,
  modelsLoadedFor,
  shouldAutoFetchModels,
  withoutModelOptions,
} from "./modelAutoFetch";

let failures = 0;
let checks = 0;
const check = (name: string, actual: unknown, expected: unknown) => {
  checks += 1;
  try {
    assert.deepStrictEqual(actual, expected);
  } catch {
    failures += 1;
    console.error(
      `FAIL ${name}: got ${JSON.stringify(actual)}, want ${JSON.stringify(expected)}`,
    );
  }
};

const anthropic = {
  providerId: "anthropic",
  baseUrl: "https://api.anthropic.com/v1",
  apiKey: "sk-test",
  modelsLoaded: false,
  isFetching: false,
  alreadyTried: false,
};

// A provider can list its models once it has what it needs to be asked.
check(
  "provider with a key",
  isProviderReadyForModelList("anthropic", anthropic.baseUrl, "sk-test"),
  true,
);
check(
  "provider without a key",
  isProviderReadyForModelList("anthropic", anthropic.baseUrl, ""),
  false,
);
check(
  "blank key",
  isProviderReadyForModelList("anthropic", anthropic.baseUrl, "  "),
  false,
);
check(
  "custom provider with a base URL and no key",
  isProviderReadyForModelList("custom", "http://localhost:11434/v1", ""),
  true,
);
check(
  "custom provider without a base URL",
  isProviderReadyForModelList("custom", " ", "sk-test"),
  false,
);
check(
  "Apple Intelligence",
  isProviderReadyForModelList("apple_intelligence", "", "sk-test"),
  false,
);

// Opening the page loads the list once: the first time for a provider in this
// app session, and again only when its key or base URL changes.
check("first visit", shouldAutoFetchModels(anthropic), true);
check(
  "custom provider, first visit",
  shouldAutoFetchModels({
    ...anthropic,
    providerId: "custom",
    baseUrl: "http://localhost:11434/v1",
    apiKey: "",
  }),
  true,
);
check(
  "list already loaded",
  shouldAutoFetchModels({ ...anthropic, modelsLoaded: true }),
  false,
);
check(
  "request running",
  shouldAutoFetchModels({ ...anthropic, isFetching: true }),
  false,
);
check(
  "tried before, e.g. failed",
  shouldAutoFetchModels({ ...anthropic, alreadyTried: true }),
  false,
);
check("no key", shouldAutoFetchModels({ ...anthropic, apiKey: "" }), false);
check(
  "Apple Intelligence",
  shouldAutoFetchModels({ ...anthropic, providerId: "apple_intelligence" }),
  false,
);

// A failed attempt is not repeated for the same settings, but a new key or
// base URL is a new attempt.
check(
  "same settings, same attempt",
  modelFetchAttemptKey("anthropic", anthropic.baseUrl, "sk-test"),
  modelFetchAttemptKey("anthropic", anthropic.baseUrl, "sk-test"),
);
check(
  "new key, new attempt",
  modelFetchAttemptKey("anthropic", anthropic.baseUrl, "sk-test") ===
    modelFetchAttemptKey("anthropic", anthropic.baseUrl, "sk-other"),
  false,
);
check(
  "new base URL, new attempt",
  modelFetchAttemptKey("custom", "http://a/v1", "") ===
    modelFetchAttemptKey("custom", "http://b/v1", ""),
  false,
);
check(
  "other provider, other attempt",
  modelFetchAttemptKey("openai", "https://x/v1", "k") ===
    modelFetchAttemptKey("anthropic", "https://x/v1", "k"),
  false,
);

// OpenRouter answers GET /models without a key (checked 2026-11-15; OpenAI
// and Groq answer 401), so its list loads as soon as it is chosen.
check(
  "OpenRouter without a key",
  isProviderReadyForModelList("openrouter", "https://openrouter.ai/api/v1", ""),
  true,
);

// After a new key or a provider switch the list counts as not loaded, so the
// page loads it by itself (user, 2026-11-15: entering the key needed a click
// on refresh). Other providers' lists stay.
const loaded = { anthropic: ["claude-a"], openai: ["gpt-a"] };
const cleared = withoutModelOptions(loaded, "anthropic");
check("cleared list not loaded", modelsLoadedFor(cleared, "anthropic"), false);
check("other lists kept", cleared.openai, ["gpt-a"]);
check("nothing shown from before", cleared.anthropic ?? [], []);
check(
  "a new key loads the list again",
  shouldAutoFetchModels({
    ...anthropic,
    apiKey: "sk-new",
    modelsLoaded: modelsLoadedFor(cleared, "anthropic"),
  }),
  true,
);
check("a loaded list counts", modelsLoadedFor(loaded, "openai"), true);
check("a list never loaded", modelsLoadedFor(loaded, "groq"), false);

// Switching to another provider and back: the list loaded before was cleared
// by the switch and loads again; a failed attempt is still not repeated.
const attempt = modelFetchAttemptKey("anthropic", anthropic.baseUrl, "sk-test");
const succeeded = new Set([attempt]);
forgetLoadedAttempt(succeeded, attempt, true);
check("a loaded attempt is forgotten", succeeded.has(attempt), false);
check(
  "back to a provider whose list was loaded",
  shouldAutoFetchModels({
    ...anthropic,
    modelsLoaded: modelsLoadedFor(
      withoutModelOptions(loaded, "anthropic"),
      "anthropic",
    ),
    alreadyTried: succeeded.has(attempt),
  }),
  true,
);
const failed = new Set([attempt]);
forgetLoadedAttempt(failed, attempt, false);
check("a failed attempt stays tried", failed.has(attempt), true);

if (failures > 0) {
  console.error(`model auto-fetch: ${failures} of ${checks} checks failed`);
  process.exit(1);
}
console.log(`model auto-fetch: all ${checks} checks passed`);
