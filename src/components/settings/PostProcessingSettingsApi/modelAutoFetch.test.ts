// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/PostProcessingSettingsApi/modelAutoFetch.test.ts
import assert from "node:assert";
import {
  isProviderReadyForModelList,
  modelFetchAttemptKey,
  shouldAutoFetchModels,
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

if (failures > 0) {
  console.error(`model auto-fetch: ${failures} of ${checks} checks failed`);
  process.exit(1);
}
console.log(`model auto-fetch: all ${checks} checks passed`);
