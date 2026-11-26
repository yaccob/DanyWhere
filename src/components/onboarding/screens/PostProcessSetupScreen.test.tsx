// DanyWhere: onboarding's screen "Improve your texts" (sidecar qa
// 2026-11-26_1840, section 5: the questions come at the first
// installation, as the last screen, with "Later"; 2026-11-26_2250, 4.5).
// It comes after Try it and before All set, holds the same setup as the
// page "Post Process", and "Later" is its most prominent button.
import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import { OnboardingFlow } from "../OnboardingFlow";
import {
  STEP_ORDER,
  useOnboardingStore,
  type OnboardingStep,
} from "../onboardingStore";
import { callsOf } from "@/test/bindings-mock";
import { switchTestLanguage } from "@/test/i18n";
import { press, renderPage } from "@/test/render";
import { mockIpc, setOsType } from "@/test/tauri-mock";
import { rawTextIn, visibleText } from "@/test/visible";
import {
  BROWSER_PAGE,
  answerStatus,
  askForLessMotion,
  lastAnswers,
  notReady,
  ready,
  setupSettings,
  signInStarts,
} from "@/test/postProcessSetupFixtures";
import type { SetupStatus } from "@/bindings";

const CHECK_ACCESSIBILITY =
  "plugin:macos-permissions|check_accessibility_permission";
const CHECK_MICROPHONE = "plugin:macos-permissions|check_microphone_permission";

const TITLE = "Improve your texts";

const renderAt = async (
  at: OnboardingStep,
  status: SetupStatus = notReady(),
) => {
  setOsType("macos");
  mockIpc(CHECK_ACCESSIBILITY, () => true);
  mockIpc(CHECK_MICROPHONE, () => true);
  answerStatus(status);
  useOnboardingStore.getState().set({ step: at });
  return renderPage(<OnboardingFlow onComplete={() => {}} />, {
    settings: setupSettings(null, { app: { onboarding_completed: false } }),
  });
};

const button = (name: string | RegExp) =>
  screen.queryByRole("button", { name });
const step = () => useOnboardingStore.getState().step;

let restoreMotion: (() => void) | null = null;

beforeEach(() => {
  localStorage.clear();
  useOnboardingStore.getState().reset(false);
});

afterEach(() => {
  restoreMotion?.();
  restoreMotion = null;
});

describe("onboarding: Improve your texts", () => {
  test("comes after Try it and before All set", () => {
    const at = STEP_ORDER.indexOf("postProcessSetup" as OnboardingStep);
    expect(at).toBeGreaterThan(-1);
    expect(STEP_ORDER[at - 1]).toBe("try");
    expect(STEP_ORDER[at + 1]).toBe("done");
  });

  test("Try it leads to it", async () => {
    restoreMotion = askForLessMotion();
    await renderAt("try");
    await press(button("Skip")!);
    expect(step()).toBe("postProcessSetup");
    expect(visibleText()).toContain(TITLE);
  });

  test("shows the questions, and Later is the most prominent button", async () => {
    await renderAt("postProcessSetup");
    const text = visibleText();
    expect(text).toContain(TITLE);
    expect(text).toContain("1. Where may your texts go?");
    // Welcome, Model, Demo, Try it, this one, All set: the permissions
    // were already given.
    expect(screen.queryByRole("img", { name: "Step 5 of 6" })).not.toBeNull();

    const later = button("Later")!;
    expect(later.className).toContain("bg-background-ui");
    // No other button looks like the next step.
    const primaries = screen
      .getAllByRole("button")
      .filter((b) => b.className.includes("bg-background-ui"));
    expect(primaries).toEqual([later]);
    expect(rawTextIn()).toEqual([]);
  });

  test("Later goes on to All set and puts the setup off", async () => {
    await renderAt("postProcessSetup");
    await press(button("Later")!);
    expect(step()).toBe("done");
    expect(lastAnswers()?.skipped).toBe(true);
  });

  test("Back goes to Try it", async () => {
    await renderAt("postProcessSetup");
    await press(button("Back")!);
    expect(step()).toBe("try");
  });

  test("the quick way works here too", async () => {
    await renderAt("postProcessSetup");
    await press(
      screen.getByRole("radio", { name: /^Also to free services\./ }),
    );
    await press(screen.getByRole("radio", { name: /^None of these/ }));
    // Later still leads: the connect button isn't the next step.
    const connect = button("Connect OpenRouter")!;
    expect(connect.className).not.toContain("bg-background-ui");
    expect(button("Later")!.className).toContain("bg-background-ui");
    signInStarts();
    await press(connect);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["openrouter", BROWSER_PAGE],
    ]);
    expect(visibleText()).toContain("Waiting for the browser …");
  });

  test("once set up, Continue", async () => {
    await renderAt("postProcessSetup", ready());
    expect(visibleText()).toContain("Connected to OpenRouter");
    expect(button("Later")).toBeNull();
    await press(button("Continue")!);
    expect(step()).toBe("done");
    expect(lastAnswers()?.skipped ?? false).toBe(false);
  });

  test("in German", async () => {
    await switchTestLanguage("de");
    await renderAt("postProcessSetup");
    const text = visibleText();
    expect(text).toContain("Texte verbessern lassen");
    expect(text).toContain("1. Wohin dürfen deine Texte?");
    expect(button("Später")).not.toBeNull();
    expect(rawTextIn()).toEqual([]);
  });
});
