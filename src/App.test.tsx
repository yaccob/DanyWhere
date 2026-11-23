// DanyWhere: the one-time guide to the input method (qa 2026-11-23_2110,
// part D). Installations that run without Accessibility finished onboarding
// before the input method existed, so they never saw its step. At the next
// start they get the same screen once, as a screen of its own: only on macOS,
// without Accessibility, in a build with the input method, while the source
// isn't added and the guide wasn't seen. Leaving it marks it as seen.
import { describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import type { AppSettings, InputMethodStatus } from "@/bindings";
import App from "./App";
import { callsOf, mockCommand } from "@/test/bindings-mock";
import { settingsWith } from "@/test/fixtures";
import { press, renderPage } from "@/test/render";
import { mockIpc, setOsType, type TestOsType } from "@/test/tauri-mock";
import { rawTextIn, visibleText } from "@/test/visible";

const CHECK_ACCESSIBILITY =
  "plugin:macos-permissions|check_accessibility_permission";
const CHECK_MICROPHONE = "plugin:macos-permissions|check_microphone_permission";

const TITLE = "Insert Directly";
const STEPS =
  "In Keyboard, click Edit… next to Input Sources, then +, search for “DanyWhere” and click Add.";

interface StartSetup {
  os?: TestOsType;
  /** The mode without Accessibility (chosen in onboarding). */
  skipped?: boolean;
  /** macOS grants Accessibility. */
  granted?: boolean;
  seen?: boolean;
  inputMethod?: Partial<InputMethodStatus>;
}

/** The app's start for someone who finished onboarding long ago. */
const start = async ({
  os = "macos",
  skipped = true,
  granted = false,
  seen = false,
  inputMethod = { available: true },
}: StartSetup = {}) => {
  setOsType(os);
  mockIpc(CHECK_ACCESSIBILITY, () => granted);
  mockIpc(CHECK_MICROPHONE, () => true);
  mockCommand("getInputMethodStatus", async () => ({
    available: false,
    installed: false,
    added: false,
    connected: false,
    ...inputMethod,
  }));
  const app: Partial<AppSettings> = {
    onboarding_completed: true,
    accessibility_skipped: skipped,
    input_method_guide_seen: seen,
  };
  return renderPage(<App />, { settings: settingsWith({ app }) });
};

const guideShown = () => visibleText().includes(TITLE);

describe("the one-time guide at start", () => {
  test("shows once without Accessibility, as a screen of its own", async () => {
    await start();
    expect(guideShown()).toBe(true);
    expect(visibleText()).toContain(STEPS);
    // A screen of its own: no step dashes, no Back.
    expect(screen.queryByRole("img", { name: /^Step \d+ of \d+$/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
    // The window may start hidden.
    expect(callsOf("showMainWindowCommand").length).toBe(1);
    expect(rawTextIn()).toEqual([]);
  });

  test("Later leaves it for the settings and marks it as seen", async () => {
    await start();
    await press(screen.getByRole("button", { name: "Later" }));
    expect(guideShown()).toBe(false);
    expect(callsOf("setInputMethodGuideSeen")).toEqual([[true]]);
  });

  const WITHOUT_GUIDE: [string, StartSetup][] = [
    ["once seen", { seen: true }],
    ["with Accessibility", { skipped: false, granted: true }],
    [
      "when the source is already added",
      { inputMethod: { available: true, installed: true, added: true } },
    ],
    ["in a build without the input method", { inputMethod: {} }],
    ["on Windows", { os: "windows" }],
    ["on Linux", { os: "linux" }],
  ];
  for (const [when, setup] of WITHOUT_GUIDE) {
    test(`not ${when}`, async () => {
      await start(setup);
      expect(guideShown()).toBe(false);
      expect(callsOf("setInputMethodGuideSeen")).toEqual([]);
    });
  }
});
