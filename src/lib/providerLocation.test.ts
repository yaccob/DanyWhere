// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/providerLocation.test.ts
import assert from "node:assert";
import {
  APPLE_INTELLIGENCE_PROVIDER_ID,
  isLocalBaseUrl,
  providerLocation,
} from "./providerLocation";

const LOCAL = [
  "http://localhost:11434/v1",
  "http://LocalHost:1234/v1/",
  "http://127.0.0.1:1234/v1",
  "http://127.0.0.2:8080",
  "http://[::1]:8080/v1",
  "https://localhost/v1",
  "  http://localhost:11434/v1  ",
];

const ELSEWHERE = [
  "https://api.openai.com/v1",
  "https://api.anthropic.com/v1",
  "http://192.168.1.10:11434/v1",
  "http://10.0.0.1:1234/v1",
  "http://localhost.example.com/v1",
  "http://example.com/localhost",
  "http://127.0.0.1.example.com/v1",
  "apple-intelligence://local",
  "localhost:11434",
  "not a url",
  "",
];

const cases: Array<[string, () => void]> = [
  ...LOCAL.map((url): [string, () => void] => [
    `${url.trim()} is on this computer`,
    () => assert.strictEqual(isLocalBaseUrl(url), true),
  ]),
  ...ELSEWHERE.map((url): [string, () => void] => [
    `"${url}" leaves this computer`,
    () => assert.strictEqual(isLocalBaseUrl(url), false),
  ]),
  [
    "Ollama through the custom provider runs on this computer",
    () =>
      assert.strictEqual(
        providerLocation({
          id: "custom",
          base_url: "http://localhost:11434/v1",
        }),
        "local",
      ),
  ],
  [
    "the custom provider pointed at a server runs in the cloud",
    () =>
      assert.strictEqual(
        providerLocation({
          id: "custom",
          base_url: "https://llm.example.com/v1",
        }),
        "cloud",
      ),
  ],
  [
    "OpenAI runs in the cloud",
    () =>
      assert.strictEqual(
        providerLocation({
          id: "openai",
          base_url: "https://api.openai.com/v1",
        }),
        "cloud",
      ),
  ],
  [
    "Apple Intelligence runs on this computer",
    () =>
      assert.strictEqual(
        providerLocation({
          id: APPLE_INTELLIGENCE_PROVIDER_ID,
          base_url: "apple-intelligence://local",
        }),
        "local",
      ),
  ],
];

let failed = 0;
for (const [name, check] of cases) {
  try {
    check();
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${name}: ${(error as Error).message.split("\n")[0]}`);
  }
}
if (failed > 0) {
  console.error(`providerLocation: ${failed} of ${cases.length} checks failed`);
  process.exit(1);
}
console.log(`providerLocation: all ${cases.length} checks passed`);
