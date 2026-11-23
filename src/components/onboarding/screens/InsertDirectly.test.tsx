// DanyWhere: onboarding's step "Insert Directly" (qa 2026-11-23_2110, part
// D). After "Continue without Accessibility" on a Mac whose build carries
// DanyWhere's input method, the flow leads to adding its input source in
// System Settings: what it does, the button, the steps and a live state.
// "Later" (or "Continue" once added) always goes on, and leaving the step
// marks the guide as seen, so the one-time guide at start never repeats it.
import { beforeEach, describe, expect, test } from "bun:test";
import { screen, waitFor } from "@testing-library/react";
import type { InputMethodStatus } from "@/bindings";
import { OnboardingFlow } from "../OnboardingFlow";
import { useOnboardingStore, type OnboardingStep } from "../onboardingStore";
import { callsOf, mockCommand } from "@/test/bindings-mock";
import { settingsWith } from "@/test/fixtures";
import { switchTestLanguage } from "@/test/i18n";
import { press, renderPage } from "@/test/render";
import { mockIpc, setOsType, type TestOsType } from "@/test/tauri-mock";
import { rawTextIn, visibleText } from "@/test/visible";

const CHECK_ACCESSIBILITY =
  "plugin:macos-permissions|check_accessibility_permission";
const CHECK_MICROPHONE = "plugin:macos-permissions|check_microphone_permission";

const TITLE = "Insert Directly";
const WHY =
  "Without Accessibility, a dictation waits until you insert it with ⌃⇧V. Once the input source is added, end shortcuts insert it right away.";
const DESCRIPTION =
  "Dictations go straight into the text field through DanyWhere's own input source, and the clipboard stays as it is.";
const SET_UP = "Add in System Settings";
const STEPS =
  "In Keyboard, click Edit… next to Input Sources, then +, search for “DanyWhere” and click Add.";
const NOT_ADDED = "Not added yet";
const ADDED = "Added. Your dictations now go straight into the text field.";
const SKIP = "Continue without Accessibility";

const status = (state: Partial<InputMethodStatus>): InputMethodStatus => ({
  available: true,
  installed: false,
  added: false,
  connected: false,
  ...state,
});

interface FlowSetup {
  os?: TestOsType;
  /** The user chose to continue without Accessibility before. */
  skipped?: boolean;
  /** macOS grants Accessibility. */
  granted?: boolean;
  /** The backend's input method; null keeps its default (none). */
  inputMethod?: Partial<InputMethodStatus> | null;
  /** The screen the saved progress points at. */
  at?: OnboardingStep;
}

/** First-run onboarding at `at`, as the flow resumes saved progress. */
const renderFlow = async ({
  os = "macos",
  skipped = false,
  granted = false,
  inputMethod = { available: true },
  at = "permissions",
}: FlowSetup = {}) => {
  setOsType(os);
  mockIpc(CHECK_ACCESSIBILITY, () => granted);
  mockIpc(CHECK_MICROPHONE, () => true);
  if (inputMethod)
    mockCommand("getInputMethodStatus", async () => status(inputMethod));
  useOnboardingStore.getState().set({ step: at });
  return renderPage(<OnboardingFlow onComplete={() => {}} />, {
    settings: settingsWith({
      app: { onboarding_completed: false, accessibility_skipped: skipped },
    }),
  });
};

const button = (name: string) => screen.queryByRole("button", { name });
const step = () => useOnboardingStore.getState().step;

beforeEach(() => {
  localStorage.clear();
  useOnboardingStore.getState().reset(false);
});

describe("onboarding: when Insert Directly comes", () => {
  test("after continuing without Accessibility on a Mac with the input method", async () => {
    await renderFlow();
    await press(button(SKIP)!);
    await press(button("Continue")!);
    expect(step()).toBe("insertDirectly");
    const text = visibleText();
    expect(text).toContain(TITLE);
    expect(text).toContain(DESCRIPTION);
    expect(text).toContain(WHY);
    expect(text).toContain(STEPS);
    expect(button(SET_UP)).not.toBeNull();
    expect(text).toContain(NOT_ADDED);
    expect(rawTextIn()).toEqual([]);
  });

  test("a build without the input method goes on to the model", async () => {
    await renderFlow({ inputMethod: null });
    await press(button(SKIP)!);
    await press(button("Continue")!);
    expect(step()).toBe("model");
    expect(visibleText()).not.toContain(TITLE);
  });

  test("with Accessibility there is no such step", async () => {
    await renderFlow({ granted: true, at: "insertDirectly" });
    expect(visibleText()).not.toContain(TITLE);
    expect(button(SET_UP)).toBeNull();
  });

  for (const os of ["windows", "linux"] as TestOsType[]) {
    test(`${os}: no such step, and nothing is asked`, async () => {
      await renderFlow({ os, at: "insertDirectly" });
      expect(visibleText()).not.toContain(TITLE);
      expect(callsOf("getInputMethodStatus")).toEqual([]);
    });
  }

  test("counts as a step of the flow", async () => {
    await renderFlow({ skipped: true, at: "insertDirectly" });
    // Welcome, Insert Directly, Model, Demo, Try it, All set: the
    // permissions were already given.
    expect(screen.queryByRole("img", { name: "Step 2 of 6" })).not.toBeNull();
  });
});

describe("onboarding › Insert Directly: what it does", () => {
  test("the button sets up the input method", async () => {
    await renderFlow({ skipped: true, at: "insertDirectly" });
    await press(button(SET_UP)!);
    expect(callsOf("setUpInputMethod")).toEqual([[]]);
  });

  test("Later goes on and marks the guide as seen", async () => {
    await renderFlow({ skipped: true, at: "insertDirectly" });
    expect(button("Continue")).toBeNull();
    await press(button("Later")!);
    expect(step()).toBe("model");
    expect(callsOf("setInputMethodGuideSeen")).toEqual([[true]]);
  });

  test("Back goes to the screen before it", async () => {
    await renderFlow({ skipped: true, at: "insertDirectly" });
    await press(button("Back")!);
    expect(step()).toBe("welcome");
  });

  test("the state follows System Settings: added, then Continue", async () => {
    let added = false;
    mockCommand("getInputMethodStatus", async () =>
      status({ installed: true, added }),
    );
    await renderFlow({
      skipped: true,
      at: "insertDirectly",
      inputMethod: null,
    });
    expect(visibleText()).toContain(NOT_ADDED);
    added = true;
    await waitFor(() => expect(visibleText()).toContain(ADDED), {
      timeout: 2500,
    });
    expect(button(SET_UP)).toBeNull();
    expect(button("Later")).toBeNull();
    expect(rawTextIn()).toEqual([]);
    await press(button("Continue")!);
    expect(step()).toBe("model");
    expect(callsOf("setInputMethodGuideSeen")).toEqual([[true]]);
  });
});

describe("onboarding › Insert Directly in German", () => {
  test("title, button and Later", async () => {
    await switchTestLanguage("de");
    await renderFlow({ skipped: true, at: "insertDirectly" });
    const text = visibleText();
    expect(text).toContain("Direkt einfügen");
    expect(text).toContain("Bedienungshilfen");
    expect(text).toContain("„Bearbeiten …“");
    expect(button("In den Systemeinstellungen hinzufügen")).not.toBeNull();
    expect(button("Später")).not.toBeNull();
    expect(rawTextIn()).toEqual([]);
  });
});
