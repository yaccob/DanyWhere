// DanyWhere: the word lists on the Advanced page with a speech model that
// takes no vocabulary (Canary, Parakeet …; sidecar
// qa/2026-11-24_0130_wortlisten-llm-pruefung.md, point 7). The line under
// the heading says what the lists do with the page's model; without
// vocabulary the page offers the LLM check (`word_list_llm_check`), which
// needs a post-processing provider with a model, a button to Whisper Large
// v3 Turbo, which takes them, and without a provider a way to set one up.
import { beforeEach, describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import React, { useState } from "react";
import type { ModelInfo } from "@/bindings";
import type { NavLocation } from "@/lib/navigation";
import { SettingsPage } from "../SettingsPage";
import { useModelStore } from "@/stores/modelStore";
import { callsOf, mockCommand, ok } from "@/test/bindings-mock";
import { modelInfo, settingsWith, type SettingsSpec } from "@/test/fixtures";
import { switchTestLanguage } from "@/test/i18n";
import { press, renderPage } from "@/test/render";
import { isVisible, rawTextIn, visibleText } from "@/test/visible";

const WORK = "profile_1795300000001";
const TURBO_ID = "whisper-large-v3-turbo";
const CANARY = modelInfo("canary-1b-v2", "Canary 1B v2");
const TURBO = modelInfo(TURBO_ID, "Whisper Large v3 Turbo", {
  supports_vocabulary: true,
});

const OPENAI = {
  id: "openai",
  label: "OpenAI",
  base_url: "https://api.openai.com/v1",
};
const APPLE = {
  id: "apple_intelligence",
  label: "Apple Intelligence",
  base_url: "",
};

/** A provider with a model: the LLM check can run. */
const READY = {
  post_process_provider_id: "openai",
  post_process_providers: [OPENAI, APPLE],
  post_process_models: { openai: "gpt-5-mini" },
};
/** A provider without a model: it can't. */
const NOT_READY = { ...READY, post_process_models: {} };

const LINE_USES =
  "Whisper Large v3 Turbo uses the word lists while it recognizes speech.";
const LINE_WITH_LLM =
  "Canary 1B v2 doesn't use word lists itself. They work in dictations with post-processing, where the LLM gets them as known terms.";
const LINE_NO_EFFECT =
  "Canary 1B v2 doesn't use word lists itself, and without post-processing they have no effect.";
const SWITCH = "Check Names with the LLM, Also Without Post-Processing";
const SWITCH_ABOUT =
  "Only in dictations with a word that resembles a word list entry. The sentence then goes to OpenAI.";
const NEEDS_PROVIDER = "Needs a post-processing provider with a model.";
const NOT_WHILE_NEVER =
  "Not while post-processing is set to “Never”: then nothing goes to a provider.";
const NOT_WITH_APPLE =
  "Not with Apple Intelligence: needs another post-processing provider with a model.";
const SET_UP = "Set Up Post-Processing";
const TO_TURBO = "Switch to Whisper Large v3 Turbo";
const TURBO_NOTE =
  "Uses the word lists while it recognizes speech, but transcribes noticeably slower.";

const navigated: NavLocation[] = [];

/** The settings window's navigation around the page, as App keeps it. */
const Window: React.FC<{ start: NavLocation }> = ({ start }) => {
  const [location, setLocation] = useState(start);
  return (
    <SettingsPage
      location={location}
      onNavigate={(next) => {
        navigated.push(next);
        setLocation(next);
      }}
      onPreviewOnboarding={() => {}}
      onRunOnboarding={() => {}}
      onRunModelMigration={() => {}}
    />
  );
};

const DEFAULTS_ADVANCED: NavLocation = { kind: "defaults", page: "advanced" };
const WORK_ADVANCED: NavLocation = {
  kind: "profile",
  profileId: WORK,
  page: "advanced",
};

/** The Advanced page at `start` with these settings and speech models. */
const openAdvanced = async (
  spec: SettingsSpec,
  models: ModelInfo[] = [CANARY, TURBO],
  start: NavLocation = DEFAULTS_ADVANCED,
) => {
  useModelStore.setState({ models, initialized: true });
  mockCommand("getAvailableModels", () => ok(models));
  await renderPage(<Window start={start} />, {
    settings: settingsWith(spec),
    acceptWrites: true,
  });
};

/** The LLM check's switch, when the page shows it. */
const llmSwitch = (): HTMLInputElement | null => {
  const found = screen
    .queryAllByRole("checkbox", { name: SWITCH })
    .filter(isVisible);
  return (found[0] as HTMLInputElement | undefined) ?? null;
};

const button = (name: string): HTMLElement | null =>
  screen.queryAllByRole("button", { name }).filter(isVisible)[0] ?? null;

/** Clicks what the page shows, after checking that it shows it. */
const pressShown = async (element: HTMLElement | null) => {
  expect(element).not.toBeNull();
  await press(element!);
};

beforeEach(() => {
  navigated.length = 0;
});

describe("the line under the word lists' heading", () => {
  test("a model with vocabulary: it uses them, nothing else to offer", async () => {
    await openAdvanced({
      defaults: { selected_model: TURBO_ID, ...NOT_READY },
    });

    expect(visibleText()).toContain(LINE_USES);
    expect(llmSwitch()).toBeNull();
    expect(button(SET_UP)).toBeNull();
    expect(button(TO_TURBO)).toBeNull();
    expect(rawTextIn()).toEqual([]);
  });

  test("without vocabulary, with a provider: post-processing, the switch, Whisper", async () => {
    await openAdvanced({ defaults: { selected_model: CANARY.id, ...READY } });

    const text = visibleText();
    expect(text).toContain(LINE_WITH_LLM);
    expect(llmSwitch()?.disabled).toBe(false);
    expect(llmSwitch()?.checked).toBe(false);
    expect(text).toContain(SWITCH_ABOUT);
    expect(button(TO_TURBO)).not.toBeNull();
    expect(text).toContain(TURBO_NOTE);
    expect(button(SET_UP)).toBeNull();
    expect(rawTextIn()).toEqual([]);
  });

  test("without vocabulary, without a provider: no effect, the switch off limits, set-up and Whisper", async () => {
    await openAdvanced({
      defaults: { selected_model: CANARY.id, ...NOT_READY },
    });

    const text = visibleText();
    expect(text).toContain(LINE_NO_EFFECT);
    expect(llmSwitch()?.disabled).toBe(true);
    expect(text).toContain(NEEDS_PROVIDER);
    expect(text).not.toContain(SWITCH_ABOUT);
    expect(button(SET_UP)).not.toBeNull();
    expect(button(TO_TURBO)).not.toBeNull();
    expect(rawTextIn()).toEqual([]);
  });

  test("neither an instruction shortcut nor a prompt makes a provider ready", async () => {
    await openAdvanced({
      defaults: {
        selected_model: CANARY.id,
        ...READY,
        post_process_mode: "with_instruction_key",
        post_process_prompt_order: [],
      },
    });

    expect(visibleText()).toContain(LINE_WITH_LLM);
    expect(llmSwitch()?.disabled).toBe(false);
  });

  // With post-processing set to "never" nothing goes to a provider, the
  // check neither (sidecar qa 2026-11-26_1841, "Kleinere Folgen"): the line
  // says the lists have no effect, the switch why it can't run, and Set Up
  // Post-Processing leads out of "never".
  test("with post-processing Never: no effect, the switch says why, set-up offered", async () => {
    await openAdvanced({
      defaults: {
        selected_model: CANARY.id,
        ...READY,
        post_process_mode: "never",
      },
    });

    const text = visibleText();
    expect(text).toContain(LINE_NO_EFFECT);
    expect(llmSwitch()?.disabled).toBe(true);
    expect(text).toContain(NOT_WHILE_NEVER);
    expect(text).not.toContain(SWITCH_ABOUT);
    expect(button(SET_UP)).not.toBeNull();
    expect(rawTextIn()).toEqual([]);
  });

  test("Apple Intelligence: the switch says why it can't", async () => {
    await openAdvanced({
      defaults: {
        selected_model: CANARY.id,
        ...READY,
        post_process_provider_id: APPLE.id,
        post_process_models: { apple_intelligence: "apple-intelligence" },
      },
    });

    const text = visibleText();
    expect(text).toContain(LINE_NO_EFFECT);
    expect(llmSwitch()?.disabled).toBe(true);
    expect(text).toContain(NOT_WITH_APPLE);
    expect(rawTextIn()).toEqual([]);
  });

  test("a profile's page follows the profile's model and provider", async () => {
    await openAdvanced(
      {
        defaults: { selected_model: TURBO_ID, ...NOT_READY },
        profiles: [
          {
            id: WORK,
            name: "Work",
            own: ["selected_model", "post_process_provider_id"],
            values: { selected_model: CANARY.id, ...READY },
          },
        ],
      },
      [CANARY, TURBO],
      WORK_ADVANCED,
    );

    expect(visibleText()).toContain(LINE_WITH_LLM);
    expect(llmSwitch()?.disabled).toBe(false);
  });
});

describe("the switch", () => {
  test("writes the defaults' value on the defaults' page", async () => {
    await openAdvanced({ defaults: { selected_model: CANARY.id, ...READY } });

    await pressShown(llmSwitch());

    expect(callsOf("setProfileValue")).toEqual([
      [{ kind: "defaults" }, "word_list_llm_check", true],
    ]);
    expect(llmSwitch()?.checked).toBe(true);
  });

  test("writes the profile's value on a profile's page", async () => {
    await openAdvanced(
      {
        defaults: { selected_model: CANARY.id, ...READY },
        profiles: [{ id: WORK, name: "Work" }],
      },
      [CANARY, TURBO],
      WORK_ADVANCED,
    );

    await pressShown(llmSwitch());

    expect(callsOf("setProfileValue")).toEqual([
      [{ kind: "profile", id: WORK }, "word_list_llm_check", true],
    ]);
  });

  test("shows a value that is on", async () => {
    await openAdvanced({
      defaults: {
        selected_model: CANARY.id,
        ...READY,
        word_list_llm_check: true,
      },
    });

    expect(llmSwitch()?.checked).toBe(true);
  });

  test("one that is on can be turned off without a provider", async () => {
    await openAdvanced({
      defaults: {
        selected_model: CANARY.id,
        ...NOT_READY,
        word_list_llm_check: true,
      },
    });

    expect(llmSwitch()?.disabled).toBe(false);
    await pressShown(llmSwitch());

    expect(callsOf("setProfileValue")).toEqual([
      [{ kind: "defaults" }, "word_list_llm_check", false],
    ]);
    expect(llmSwitch()?.disabled).toBe(true);
  });
});

describe("Switch to Whisper Large v3 Turbo", () => {
  test("selects it as the defaults' model when it's on disk", async () => {
    await openAdvanced({ defaults: { selected_model: CANARY.id, ...READY } });

    await pressShown(button(TO_TURBO));

    expect(callsOf("downloadModel")).toEqual([]);
    expect(callsOf("setProfileValue")).toEqual([
      [{ kind: "defaults" }, "selected_model", TURBO_ID],
    ]);
  });

  test("downloads it first, then selects it for the profile", async () => {
    await openAdvanced(
      {
        defaults: { selected_model: CANARY.id, ...READY },
        profiles: [{ id: WORK, name: "Work" }],
      },
      [CANARY, { ...TURBO, is_downloaded: false }],
      WORK_ADVANCED,
    );

    await pressShown(button(TO_TURBO));

    expect(callsOf("downloadModel")).toEqual([[TURBO_ID]]);
    expect(callsOf("setProfileValue")).toEqual([
      [{ kind: "profile", id: WORK }, "selected_model", TURBO_ID],
    ]);
  });

  test("isn't offered when the app doesn't list that model", async () => {
    await openAdvanced({ defaults: { selected_model: CANARY.id, ...READY } }, [
      CANARY,
    ]);

    expect(visibleText()).toContain(LINE_WITH_LLM);
    expect(button(TO_TURBO)).toBeNull();
    expect(visibleText()).not.toContain(TURBO_NOTE);
  });
});

describe("Set Up Post-Processing", () => {
  test("turns Never into With Instruction Shortcut and opens its page of the defaults", async () => {
    await openAdvanced({
      defaults: {
        selected_model: CANARY.id,
        ...NOT_READY,
        post_process_mode: "never",
      },
    });

    await pressShown(button(SET_UP));

    expect(callsOf("setProfileValue")).toEqual([
      [{ kind: "defaults" }, "post_process_mode", "with_instruction_key"],
    ]);
    expect(navigated).toEqual([{ kind: "defaults", page: "postprocessing" }]);
  });

  test("opens the profile's page, post-processing already allowed", async () => {
    await openAdvanced(
      {
        defaults: {
          selected_model: CANARY.id,
          ...NOT_READY,
          post_process_mode: "always",
        },
        profiles: [{ id: WORK, name: "Work" }],
      },
      [CANARY, TURBO],
      WORK_ADVANCED,
    );

    await pressShown(button(SET_UP));

    expect(callsOf("setProfileValue")).toEqual([]);
    expect(navigated).toEqual([
      { kind: "profile", profileId: WORK, page: "postprocessing" },
    ]);
  });
});

describe("in German", () => {
  test("every text of the three states is translated", async () => {
    await switchTestLanguage("de");
    await openAdvanced({
      defaults: { selected_model: CANARY.id, ...NOT_READY },
    });

    const text = visibleText();
    expect(text).toContain(
      "Canary 1B v2 nutzt Wortlisten nicht selbst, und ohne Nachbearbeitung wirken sie nicht.",
    );
    expect(text).toContain(
      "Namen mit dem LLM prüfen, auch ohne Nachbearbeitung",
    );
    expect(text).toContain(
      "Braucht einen Anbieter für die Nachbearbeitung mit einem Modell.",
    );
    expect(button("Nachbearbeitung einrichten")).not.toBeNull();
    expect(button("Zu Whisper Large v3 Turbo wechseln")).not.toBeNull();
    expect(text).not.toContain(SET_UP);
    expect(rawTextIn()).toEqual([]);
  });
});
