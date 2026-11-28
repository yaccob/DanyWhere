// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/postProcessSetup/guides.test.ts
//
// DanyWhere: every page a guided key's step or an account button opens, as
// checked at the providers' pages and docs on 2026-11-26 (sidecar qa
// 2026-11-26_2250, "Prüfung der Adressen"). A changed address fails here
// until it is checked again.
import assert from "node:assert/strict";
import { GUIDES, PROVIDER_PAGES, guideFor, providerPages } from "./guides";

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

/** Each guide's step links, in order. */
const STEP_URLS: Record<string, string[]> = {
  anthropic: [
    // The article's current slug; the old one answers with a redirect.
    "https://support.claude.com/en/articles/17154008-monthly-api-credits-for-max-ultra-and-team-plans",
    "https://platform.claude.com/settings/keys",
  ],
  gemini: [
    "https://aistudio.google.com/",
    "https://aistudio.google.com/apikey",
  ],
  huggingface: [
    // Creates a fine-grained token with "Make calls to Inference Providers".
    "https://huggingface.co/settings/tokens/new?ownUserPermissions=inference.serverless.write&tokenType=fineGrained",
  ],
  ollama_cloud: ["https://ollama.com/settings/keys"],
  groq: ["https://console.groq.com/keys"],
};

/** Each provider's account pages. */
const PAGES: Record<
  string,
  { usage?: string; credit?: string; keys?: string }
> = {
  chatgpt: { usage: "https://chatgpt.com/settings/usage" },
  openrouter: {
    credit: "https://openrouter.ai/settings/credits",
    keys: "https://openrouter.ai/settings/keys",
  },
  anthropic: {
    usage: "https://platform.claude.com/settings/spend-limits",
    credit: "https://platform.claude.com/settings/billing",
    keys: "https://platform.claude.com/settings/keys",
  },
  gemini: {
    usage: "https://aistudio.google.com/usage",
    credit: "https://aistudio.google.com/billing",
    keys: "https://aistudio.google.com/apikey",
  },
  huggingface: {
    usage: "https://huggingface.co/settings/billing",
    credit: "https://huggingface.co/settings/billing",
    keys: "https://huggingface.co/settings/tokens",
  },
  ollama_cloud: {
    usage: "https://ollama.com/settings",
    keys: "https://ollama.com/settings/keys",
  },
  groq: {
    usage: "https://console.groq.com/dashboard/usage",
    keys: "https://console.groq.com/keys",
  },
};

check("the guides are exactly the five guided keys", () =>
  assert.deepEqual(Object.keys(GUIDES).sort(), Object.keys(STEP_URLS).sort()),
);

for (const [provider, urls] of Object.entries(STEP_URLS)) {
  check(`${provider}: the steps' links`, () =>
    assert.deepEqual(
      guideFor(provider)
        ?.steps.map((step) => step.link?.url)
        .filter((url) => url !== undefined),
      urls,
    ),
  );
}

check("the account pages are exactly these providers'", () =>
  assert.deepEqual(
    Object.keys(PROVIDER_PAGES).sort(),
    Object.keys(PAGES).sort(),
  ),
);

for (const [provider, pages] of Object.entries(PAGES)) {
  check(`${provider}: the account pages`, () =>
    assert.deepEqual(providerPages(provider), pages),
  );
}

check("a provider without pages has none", () =>
  assert.deepEqual(providerPages("custom"), {}),
);

check("every address is https without spaces", () => {
  const all = [
    ...Object.values(GUIDES).flatMap((guide) =>
      guide.steps.flatMap((step) => (step.link ? [step.link.url] : [])),
    ),
    ...Object.values(PROVIDER_PAGES).flatMap((pages) => Object.values(pages)),
  ];
  for (const url of all) {
    assert.match(url, /^https:\/\/[^\s]+$/, url);
    assert.doesNotThrow(() => new URL(url), url);
  }
});

if (failures.length > 0) {
  for (const failure of failures) console.error(`FAIL ${failure}`);
  console.error(
    `postProcessSetup/guides: ${failures.length} of ${total} checks failed`,
  );
  process.exit(1);
}
console.log(`postProcessSetup/guides: all ${total} checks passed`);
