import assert from "node:assert/strict";
import { postProcessAvailability, retranscribeMode } from "./postProcessEntry";

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
  post_process_selected_prompt_id: "p1",
  post_process_prompts: [{ id: "p1", name: "Clean", prompt: "Fix it" }],
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
check("post-processing needs a selected prompt that exists", () => {
  assert.equal(
    postProcessAvailability({
      ...ready,
      post_process_selected_prompt_id: null,
    }),
    "not_set_up",
  );
  assert.equal(
    postProcessAvailability({
      ...ready,
      post_process_selected_prompt_id: "deleted",
    }),
    "not_set_up",
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

if (failures > 0) {
  console.error(`postProcessEntry: ${failures} check(s) failed`);
  process.exit(1);
}
console.log("postProcessEntry: all assertions passed");
