// DanyWhere: the onboarding with start and hold shortcuts as lists of their
// own (sidecar qa 2026-11-26_1810). It no longer sets a "Shortcut Behavior":
// it teaches holding with the first hold shortcut and tapping with the
// first start shortcut, which out of the box are the same keys (⌃ Space).
// Demo and Try it show those keys, and changing them there changes the first
// start shortcut and the hold shortcut on the same keys.
import { beforeEach, describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import { OnboardingFlow } from "./OnboardingFlow";
import { useOnboardingStore, type OnboardingStep } from "./onboardingStore";
import { callsOf, commandCalls } from "@/test/bindings-mock";
import {
  holdKey,
  settingsWith,
  startKey,
  type SettingsSpec,
} from "@/test/fixtures";
import i18n from "@/test/i18n";
import { press, pressKeys, renderPage } from "@/test/render";
import { mockIpc, setOsType } from "@/test/tauri-mock";
import { keycapLabel } from "@/lib/utils/keyboard";

const CHECK_ACCESSIBILITY =
  "plugin:macos-permissions|check_accessibility_permission";
const CHECK_MICROPHONE = "plugin:macos-permissions|check_microphone_permission";

const START = startKey("start", "ctrl+alt+d");
const DIRECT = holdKey("hold_direct", "ctrl+alt+d");
const POST = holdKey("hold_post", "ctrl+shift+space");
const OWN_KEYS: SettingsSpec = {
  defaults: { start_keys: [START], hold_keys: [DIRECT, POST] },
};

const renderAt = (at: OnboardingStep, spec: SettingsSpec = {}) => {
  setOsType("macos");
  mockIpc(CHECK_ACCESSIBILITY, () => true);
  mockIpc(CHECK_MICROPHONE, () => true);
  useOnboardingStore.getState().set({ step: at });
  return renderPage(<OnboardingFlow onComplete={() => {}} />, {
    settings: settingsWith({ ...spec, app: { onboarding_completed: false } }),
  });
};

/** The keycaps drawn for `keys`, by their label. */
const keycaps = (keys: string) =>
  screen.queryAllByRole("img", {
    name: keycapLabel(keys, "macos", i18n.t.bind(i18n)),
  });

beforeEach(() => {
  localStorage.clear();
});

describe("the onboarding and the start and hold shortcuts", () => {
  test("it sets no shortcut behavior any more", async () => {
    await renderAt("welcome", {
      defaults: { shortcut_activation: "toggle" },
    });

    const commands = commandCalls().map((call) => call.command);
    expect(commands).not.toContain("changeShortcutActivationSetting");
    expect(
      callsOf("setProfileValue").filter(
        ([, field]) => field === "shortcut_activation",
      ),
    ).toEqual([]);
  });

  test("Demo shows the first start shortcut's keys", async () => {
    await renderAt("demo", OWN_KEYS);

    expect(keycaps("ctrl+alt+d").length).toBeGreaterThan(0);
    expect(keycaps("ctrl+space")).toEqual([]);
  });

  test("Try it shows them too", async () => {
    await renderAt("try", OWN_KEYS);

    expect(keycaps("ctrl+alt+d").length).toBeGreaterThan(0);
    expect(keycaps("ctrl+space")).toEqual([]);
  });

  test("changing the keys on Demo changes the first start shortcut and its hold shortcut", async () => {
    await renderAt("demo", OWN_KEYS);

    await press(screen.getByRole("button", { name: /Change/ }));
    await pressKeys("ControlLeft", "AltLeft", "KeyK");

    expect(callsOf("changeStartKeysSetting")).toEqual([
      [[{ id: "start", shortcut: "ctrl+option+k" }]],
    ]);
    expect(callsOf("changeHoldKeysSetting")).toEqual([
      [[{ ...DIRECT, shortcut: "ctrl+option+k" }, POST]],
    ]);
    expect(callsOf("changeBinding")).toEqual([]);
  });
});
