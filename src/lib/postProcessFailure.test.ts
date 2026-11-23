// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/postProcessFailure.test.ts
//
// DanyWhere: a failed post-processing's notice names its reason (user,
// 2026-11-23). The backend's detail (src-tauri/src/post_process_failure.rs)
// becomes a sentence after the notice's explanation; a detail this app can't
// read leaves the explanation as it was. With i18next itself and the real
// locale files.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import i18next from "i18next";
import type { Translate } from "./displayNames";
import {
  isPostProcessFailureDetail,
  withPostProcessFailureReason,
} from "./postProcessFailure";

let total = 0;
const failures: string[] = [];
const check = (name: string, body: () => void) => {
  total += 1;
  try {
    body();
  } catch (error) {
    failures.push(
      `${name}: ${(error as Error).message.split("\n").slice(0, 6).join(" ")}`,
    );
  }
};

const localesDir = new URL("../i18n/locales/", import.meta.url);
const locales = readdirSync(localesDir, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort();

const i18n = i18next.createInstance();
await i18n.init({
  lng: "en",
  fallbackLng: false,
  resources: Object.fromEntries(
    locales.map((code) => [
      code,
      {
        translation: JSON.parse(
          readFileSync(new URL(`${code}/translation.json`, localesDir), "utf8"),
        ) as Record<string, unknown>,
      },
    ]),
  ),
  interpolation: { escapeValue: false },
});
const tFor = (lng: string): Translate => {
  const fixed = i18n.getFixedT(lng);
  return (key, options) => String(fixed(key, options ?? {}));
};
const en = tFor("en");
const de = tFor("de");

/** The detail the backend sends for `fields`. */
const detail = (fields: Record<string, unknown>) => JSON.stringify(fields);
const OPENAI = { providerId: "openai", provider: "OpenAI", model: "gpt-5" };
const CUSTOM = {
  providerId: "custom",
  provider: "Custom",
  model: "gemma4-12b-qat",
};
const APPLE = {
  providerId: "apple_intelligence",
  provider: "Apple Intelligence",
  model: "Apple Intelligence",
};

/** Each reason the backend sends, with its English sentence. */
const REASONS: [string, Record<string, unknown>, string][] = [
  [
    "no provider",
    { reason: "no-provider" },
    "No provider is selected for post-processing.",
  ],
  [
    "no model",
    { reason: "no-model", providerId: "openai", provider: "OpenAI" },
    "No model is selected for OpenAI.",
  ],
  ["no prompt", { reason: "no-prompt", ...OPENAI }, "No prompt is active."],
  [
    "no API key",
    { reason: "no-api-key", ...OPENAI },
    "OpenAI needs an API key, and none is set.",
  ],
  [
    "a refused key",
    { reason: "key-refused", ...OPENAI },
    "OpenAI refused the API key.",
  ],
  [
    "a model the provider doesn't have",
    { reason: "model-not-found", ...CUSTOM },
    "Custom doesn't have the model “gemma4-12b-qat”.",
  ],
  [
    "a provider that can't be reached",
    { reason: "unreachable", ...CUSTOM },
    "Custom couldn't be reached.",
  ],
  [
    "no answer in time",
    { reason: "timeout", ...OPENAI, seconds: 60 },
    "OpenAI didn't answer within the time limit (60 s).",
  ],
  [
    "another status",
    { reason: "status", ...OPENAI, status: 503 },
    "OpenAI answered with HTTP status 503.",
  ],
  [
    "an empty answer",
    { reason: "empty-reply", ...OPENAI },
    "OpenAI sent back no text.",
  ],
  [
    "a cut-off answer",
    { reason: "truncated", ...OPENAI },
    "The answer was cut off at the model's output limit.",
  ],
  [
    "a provider not available here",
    { reason: "unavailable", ...APPLE },
    "Apple Intelligence isn't available on this computer.",
  ],
  [
    "anything else",
    {
      reason: "other",
      ...OPENAI,
      error: "Invalid authorization header value",
    },
    "Error: Invalid authorization header value",
  ],
];

// ---- the sentence per reason -----------------------------------------------
for (const [name, fields, sentence] of REASONS) {
  check(`English: ${name}`, () =>
    assert.equal(
      withPostProcessFailureReason(en, "BASE", detail(fields)),
      `BASE ${sentence}`,
    ),
  );
}

check("the user's case, in full, under the dictation's explanation", () =>
  assert.equal(
    withPostProcessFailureReason(
      en,
      en("errors.postProcessFailed"),
      detail({ reason: "model-not-found", ...CUSTOM }),
    ),
    "Post Process didn't finish, so the transcript was pasted as is. Custom doesn't have the model “gemma4-12b-qat”.",
  ),
);

// ---- German ----------------------------------------------------------------
check("German: a model the custom provider doesn't have", () =>
  assert.equal(
    withPostProcessFailureReason(
      de,
      de("errors.postProcessFailed"),
      detail({ reason: "model-not-found", ...CUSTOM }),
    ),
    "Die Nachbearbeitung wurde nicht abgeschlossen, daher wurde das Transkript unverändert eingefügt. Eigener Anbieter kennt das Modell „gemma4-12b-qat“ nicht.",
  ),
);
check("German: no API key", () =>
  assert.equal(
    withPostProcessFailureReason(
      de,
      "BASE",
      detail({ reason: "no-api-key", ...OPENAI }),
    ),
    "BASE OpenAI braucht einen API-Schlüssel, es ist aber keiner eingetragen.",
  ),
);
check("German: a refused key", () =>
  assert.equal(
    withPostProcessFailureReason(
      de,
      "BASE",
      detail({ reason: "key-refused", ...OPENAI }),
    ),
    "BASE OpenAI hat den API-Schlüssel abgelehnt.",
  ),
);
check("German: no answer in time", () =>
  assert.equal(
    withPostProcessFailureReason(
      de,
      "BASE",
      detail({ reason: "timeout", ...OPENAI, seconds: 60 }),
    ),
    "BASE OpenAI hat nicht innerhalb der Zeitgrenze geantwortet (60 s).",
  ),
);
check("German: a provider that can't be reached", () =>
  assert.equal(
    withPostProcessFailureReason(
      de,
      "BASE",
      detail({ reason: "unreachable", ...CUSTOM }),
    ),
    "BASE Eigener Anbieter ist nicht erreichbar.",
  ),
);

// ---- what can't be read leaves the explanation as it was --------------------
check("without a detail the explanation stays as it was", () => {
  assert.equal(withPostProcessFailureReason(en, "BASE", null), "BASE");
  assert.equal(withPostProcessFailureReason(en, "BASE", undefined), "BASE");
  assert.equal(withPostProcessFailureReason(en, "BASE", ""), "BASE");
});
check("a raw error isn't a reason, and shows as it is", () => {
  const raw = "The entry has no transcript to post-process";
  assert.equal(withPostProcessFailureReason(en, "BASE", raw), "BASE");
  assert.equal(isPostProcessFailureDetail(raw), false);
  assert.equal(isPostProcessFailureDetail(null), false);
  assert.equal(isPostProcessFailureDetail("42"), false);
  assert.equal(isPostProcessFailureDetail("[1]"), false);
});
check("a reason this app doesn't know: the explanation, and no code", () => {
  const newer = detail({ reason: "quota-exceeded", ...OPENAI });
  assert.equal(withPostProcessFailureReason(en, "BASE", newer), "BASE");
  assert.equal(isPostProcessFailureDetail(newer), true);
});
check("a reason without the field its sentence needs", () => {
  for (const fields of [
    { reason: "model-not-found", providerId: "custom", provider: "Custom" },
    { reason: "no-model" },
    { reason: "timeout", ...OPENAI },
    { reason: "status", ...OPENAI, status: "503" },
    { reason: "other", ...OPENAI },
  ]) {
    assert.equal(
      withPostProcessFailureReason(en, "BASE", detail(fields)),
      "BASE",
      JSON.stringify(fields),
    );
  }
});
check("the reasons are always details, never shown as they are", () => {
  for (const [, fields] of REASONS) {
    assert.equal(isPostProcessFailureDetail(detail(fields)), true);
  }
});

// ---- every locale ----------------------------------------------------------
check("every locale has a sentence for every reason, filled in", () => {
  const unfilled: string[] = [];
  for (const code of locales) {
    const t = tFor(code);
    for (const [name, fields] of REASONS) {
      const text = withPostProcessFailureReason(t, "BASE", detail(fields));
      if (
        text === "BASE" ||
        text.includes("{{") ||
        text.includes("postProcessFailure.")
      ) {
        unfilled.push(`${code}: ${name}: ${text}`);
      }
    }
  }
  assert.deepEqual(unfilled, []);
});
check("every locale names the model and the provider where it should", () => {
  const missing: string[] = [];
  for (const code of locales) {
    const text = withPostProcessFailureReason(
      tFor(code),
      "",
      detail({ reason: "model-not-found", ...OPENAI }),
    );
    if (!text.includes("gpt-5") || !text.includes("OpenAI")) {
      missing.push(`${code}: ${text}`);
    }
  }
  assert.deepEqual(missing, []);
});

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`postProcessFailure: all ${total} checks passed`);
