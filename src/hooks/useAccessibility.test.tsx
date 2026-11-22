// DanyWhere: the one place the frontend asks about Accessibility
// (`lib/accessibility`, `useAccessibility`), through the pages that ask it
// and the reads of onboarding and `App.tsx`.
import { describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import { AccessibilitySettings } from "@/components/settings/application/AccessibilitySettings";
import { AppendTextInput } from "@/components/settings/AppendText";
import {
  askMacosForAccessibility,
  macosGrantsAccessibility,
  readChoseToGoWithout,
} from "@/lib/accessibility";
import { err, mockCommand, ok } from "@/test/bindings-mock";
import { settingsWith } from "@/test/fixtures";
import { renderPage } from "@/test/render";
import {
  ipcCalls,
  mockIpc,
  setOsType,
  type TestOsType,
} from "@/test/tauri-mock";
import { visibleText } from "@/test/visible";

const CHECK = "plugin:macos-permissions|check_accessibility_permission";
const REQUEST = "plugin:macos-permissions|request_accessibility_permission";

const SECOND_PRESS = "Dictation Shortcut Also Inserts";
const NO_KEYS =
  "Without Accessibility, key presses aren't sent; the text is still appended.";

/** The settings with the user's choice `skipped`. */
const withChoice = (skipped: boolean) =>
  settingsWith({ app: { accessibility_skipped: skipped } });

const CASES: { os: TestOsType; skipped: boolean; mode: boolean }[] = [
  { os: "macos", skipped: true, mode: true },
  { os: "macos", skipped: false, mode: false },
  { os: "windows", skipped: true, mode: false },
  { os: "linux", skipped: true, mode: false },
];

describe("Application settings › Accessibility", () => {
  for (const { os, skipped, mode } of CASES) {
    test(`${os}, skipped ${skipped}: the second press's switch ${mode ? "shows" : "is hidden"}`, async () => {
      setOsType(os);
      await renderPage(<AccessibilitySettings />, {
        settings: withChoice(skipped),
      });
      expect(visibleText().includes(SECOND_PRESS)).toBe(mode);
    });
  }
});

describe("the Append field", () => {
  for (const { os, skipped, mode } of CASES) {
    test(`${os}, skipped ${skipped}: the hint on key presses ${mode ? "shows" : "is hidden"}`, async () => {
      setOsType(os);
      await renderPage(<AppendTextInput value="" onChange={() => {}} />, {
        settings: withChoice(skipped),
      });
      expect(screen.queryByText(NO_KEYS) !== null).toBe(mode);
    });
  }
});

describe("the reads of onboarding and App", () => {
  test("the user's choice comes from the backend's settings", async () => {
    mockCommand("getAppSettings", async () => ok(withChoice(true)));
    expect(await readChoseToGoWithout()).toBe(true);
    mockCommand("getAppSettings", async () => ok(withChoice(false)));
    expect(await readChoseToGoWithout()).toBe(false);
  });

  test("settings that can't be read count as no choice", async () => {
    mockCommand("getAppSettings", async () => err("store"));
    expect(await readChoseToGoWithout()).toBe(false);
  });

  test("macOS's answer comes from the permissions plugin", async () => {
    mockIpc(CHECK, () => true);
    expect(await macosGrantsAccessibility()).toBe(true);
    mockIpc(CHECK, () => false);
    expect(await macosGrantsAccessibility()).toBe(false);
  });

  test("asking for the permission goes to the plugin", async () => {
    await askMacosForAccessibility();
    expect(ipcCalls().map(({ cmd }) => cmd)).toContain(REQUEST);
  });
});
