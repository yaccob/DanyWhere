import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  canRunChosenPrompt,
  postProcessAvailability,
  retranscribeMode,
  retranscribeTitleKey,
} from "./postProcessEntry";

let failures = 0;
const check = (name: string, run: () => void) => {
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

const ready = {
  post_process_enabled: true,
  post_process_provider_id: "anthropic",
  post_process_providers: [
    { id: "anthropic", label: "Anthropic", base_url: "https://api" },
  ],
  post_process_models: { anthropic: "claude" },
  post_process_prompts: [
    { id: "p1", name: "Clean", prompt: "Fix it" },
    { id: "p2", name: "Mail", prompt: "Make an email" },
  ],
  post_process_prompt_order: [
    { id: "p2", active: false },
    { id: "p1", active: true },
  ],
};

check("post-processing is ready when everything is set", () => {
  assert.equal(postProcessAvailability(ready), "ready");
});
check("post-processing is off when switched off or unknown", () => {
  assert.equal(
    postProcessAvailability({ ...ready, post_process_enabled: false }),
    "off",
  );
  assert.equal(postProcessAvailability(null), "off");
});
check("post-processing needs a known provider", () => {
  assert.equal(
    postProcessAvailability({ ...ready, post_process_provider_id: "gone" }),
    "not_set_up",
  );
});
check("post-processing needs a model for the provider", () => {
  assert.equal(
    postProcessAvailability({ ...ready, post_process_models: {} }),
    "not_set_up",
  );
  assert.equal(
    postProcessAvailability({
      ...ready,
      post_process_models: { anthropic: " " },
    }),
    "not_set_up",
  );
});
// DanyWhere: the prompt library (src/lib/promptLibrary.ts).
check("post-processing needs at least one active prompt that exists", () => {
  assert.equal(
    postProcessAvailability({
      ...ready,
      post_process_prompt_order: [
        { id: "p1", active: false },
        { id: "p2", active: false },
      ],
    }),
    "not_set_up",
  );
  assert.equal(
    postProcessAvailability({
      ...ready,
      post_process_prompt_order: [{ id: "deleted", active: true }],
    }),
    "not_set_up",
  );
  assert.equal(
    postProcessAvailability({
      ...ready,
      post_process_prompt_order: [
        { id: "p1", active: true },
        { id: "p2", active: true },
      ],
    }),
    "ready",
  );
});

check("re-transcribe with Shift also post-processes when ready", () => {
  assert.equal(retranscribeMode(true, "ready"), "transcribe_and_post_process");
});
check("re-transcribe without Shift transcribes as dictated", () => {
  assert.equal(retranscribeMode(false, "ready"), "transcribe");
});
check("Shift does nothing while post-processing can't run", () => {
  assert.equal(retranscribeMode(true, "off"), "transcribe");
  assert.equal(retranscribeMode(true, "not_set_up"), "transcribe");
});

// H6 (strand E2): a prompt chosen in the wand's menu runs without being
// active; post-processing must be on and the provider set up.
check("a chosen prompt runs without any active prompt", () => {
  assert.equal(
    canRunChosenPrompt({ ...ready, post_process_prompt_order: [] }),
    true,
  );
});
check("a chosen prompt needs post-processing on", () => {
  assert.equal(
    canRunChosenPrompt({ ...ready, post_process_enabled: false }),
    false,
  );
});
check("a chosen prompt needs the provider's model", () => {
  assert.equal(
    canRunChosenPrompt({ ...ready, post_process_models: {} }),
    false,
  );
});

// The tooltip says what a click does (qa 2026-11-22_0107, E6): an entry
// that was post-processed before is post-processed again without Shift.
check(
  "re-transcribing a post-processed entry says it post-processes again",
  () => {
    assert.equal(
      retranscribeTitleKey(true, "ready"),
      "settings.history.retranscribeAgain",
    );
  },
);
check("re-transcribing an entry never post-processed offers Shift", () => {
  assert.equal(
    retranscribeTitleKey(false, "ready"),
    "settings.history.retranscribeShift",
  );
});
check("every tooltip of Re-transcribe has an English text", () => {
  const english = JSON.parse(
    readFileSync(
      new URL("../../../i18n/locales/en/translation.json", import.meta.url),
      "utf8",
    ),
  ) as { settings: { history: Record<string, unknown> } };
  for (const key of [
    retranscribeTitleKey(true, "ready"),
    retranscribeTitleKey(false, "ready"),
    retranscribeTitleKey(false, "off"),
  ]) {
    const name = key.replace("settings.history.", "");
    assert.equal(typeof english.settings.history[name], "string", key);
  }
});
check("without post-processing, re-transcribing only transcribes", () => {
  assert.equal(
    retranscribeTitleKey(true, "off"),
    "settings.history.retranscribe",
  );
  assert.equal(
    retranscribeTitleKey(false, "not_set_up"),
    "settings.history.retranscribe",
  );
});

if (failures > 0) {
  console.error(`postProcessEntry: ${failures} check(s) failed`);
  process.exit(1);
}
console.log("postProcessEntry: all assertions passed");
