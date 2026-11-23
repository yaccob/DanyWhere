// DanyWhere: Application Settings › Accessibility, the group "Insert Without
// the Clipboard" (qa 2026-11-23_2110, part D): DanyWhere's own input method
// as macOS's way of inserting. The group shows only on macOS in a build that
// carries the input method; it offers the setup until the user has added the
// input source in System Settings, then says that dictations go in directly
// and how to stop. While the page is open it asks every second, so the state
// follows what the user does in System Settings.
import { describe, expect, test } from "bun:test";
import { screen, waitFor } from "@testing-library/react";
import type { InputMethodStatus } from "@/bindings";
import { AccessibilitySettings } from "./AccessibilitySettings";
import { callsOf, err, mockCommand } from "@/test/bindings-mock";
import { settingsWith } from "@/test/fixtures";
import { switchTestLanguage } from "@/test/i18n";
import { press, raisedNotices, renderPage } from "@/test/render";
import { mockIpc, setOsType, type TestOsType } from "@/test/tauri-mock";
import { rawTextIn, visibleText } from "@/test/visible";

const TITLE = "Insert Without the Clipboard";
const DESCRIPTION =
  "Dictations go straight into the text field through DanyWhere's own input source, and the clipboard stays as it is.";
const HOW =
  "The input source is selected only while a dictation runs, and typing keeps your keyboard layout.";
const OFFER =
  "Optional: with Accessibility, DanyWhere already pastes dictations through the clipboard.";
const WITHOUT =
  "Once it's added, end shortcuts insert the text right away instead of leaving it for “Paste Last Transcript”.";
const SET_UP = "Add in System Settings";
const steps = (name: string) =>
  `In Keyboard, click Edit… next to Input Sources, then +, search for “${name}” and click Add.`;
const FAILED = "Couldn't set up the input source";
const ADDED = "Dictations are inserted directly, without the clipboard.";
const stop = (name: string) =>
  `To stop, remove “${name}” from Input Sources in System Settings.`;

const status = (state: Partial<InputMethodStatus>): InputMethodStatus => ({
  available: true,
  installed: false,
  added: false,
  connected: false,
  ...state,
});

/** The page with the backend's input method in `state`, without
 * Accessibility (the mode chosen in onboarding) or with it. */
const renderAccessibilityPage = async (
  state: Partial<InputMethodStatus> | null,
  { withoutAccessibility = true } = {},
) => {
  if (state) mockCommand("getInputMethodStatus", async () => status(state));
  return renderPage(<AccessibilitySettings />, {
    settings: settingsWith({
      app: { accessibility_skipped: withoutAccessibility },
    }),
  });
};

const setUpButton = () => screen.queryByRole("button", { name: SET_UP });

describe("Accessibility › Insert Without the Clipboard: when it shows", () => {
  test("a build without the input method shows no group", async () => {
    // The backend's default: NotAvailable.
    await renderAccessibilityPage(null);
    expect(visibleText()).not.toContain(TITLE);
    expect(setUpButton()).toBeNull();
  });

  for (const os of ["windows", "linux"] as TestOsType[]) {
    test(`${os} shows no group and doesn't ask`, async () => {
      setOsType(os);
      await renderAccessibilityPage({ available: true });
      expect(visibleText()).not.toContain(TITLE);
      expect(callsOf("getInputMethodStatus")).toEqual([]);
    });
  }
});

describe("Accessibility › Insert Without the Clipboard: not added yet", () => {
  test("without Accessibility: what it does, that end shortcuts then insert, the steps and the button", async () => {
    await renderAccessibilityPage({ available: true });
    const text = visibleText();
    expect(text).toContain(TITLE);
    expect(text).toContain(DESCRIPTION);
    expect(text).toContain(HOW);
    expect(text).toContain(WITHOUT);
    expect(text).not.toContain(OFFER);
    expect(text).toContain(steps("DanyWhere"));
    expect(setUpButton()).not.toBeNull();
    expect(text).not.toContain(ADDED);
    expect(rawTextIn()).toEqual([]);
  });

  test("with Accessibility it is an offer", async () => {
    await renderAccessibilityPage(
      { available: true },
      { withoutAccessibility: false },
    );
    const text = visibleText();
    expect(text).toContain(TITLE);
    expect(text).toContain(OFFER);
    expect(text).not.toContain(WITHOUT);
    expect(text).toContain(steps("DanyWhere"));
    expect(setUpButton()).not.toBeNull();
    expect(rawTextIn()).toEqual([]);
  });

  test("the steps name the input source as the app is named (Dev)", async () => {
    mockIpc("plugin:app|name", () => "DanyWhere Dev");
    await renderAccessibilityPage({ available: true });
    expect(visibleText()).toContain(steps("DanyWhere Dev"));
  });

  test("the button sets up the input method", async () => {
    await renderAccessibilityPage({ available: true });
    await press(setUpButton()!);
    expect(callsOf("setUpInputMethod")).toEqual([[]]);
    expect(raisedNotices()).toEqual([]);
  });

  test("a failed setup shows a notice with the reason", async () => {
    mockCommand("setUpInputMethod", async () => err("copy failed"));
    await renderAccessibilityPage({ available: true });
    await press(setUpButton()!);
    expect(raisedNotices()).toEqual([`${FAILED} | copy failed`]);
  });
});

describe("Accessibility › Insert Without the Clipboard: added", () => {
  test("says dictations go in directly and how to stop; no button", async () => {
    await renderAccessibilityPage({
      available: true,
      installed: true,
      added: true,
      connected: true,
    });
    const text = visibleText();
    expect(text).toContain(TITLE);
    expect(text).toContain(ADDED);
    expect(text).toContain(stop("DanyWhere"));
    expect(text).not.toContain(steps("DanyWhere"));
    expect(setUpButton()).toBeNull();
    expect(rawTextIn()).toEqual([]);
  });

  test("with Accessibility too", async () => {
    await renderAccessibilityPage(
      { available: true, installed: true, added: true },
      { withoutAccessibility: false },
    );
    expect(visibleText()).toContain(ADDED);
    expect(setUpButton()).toBeNull();
  });
});

describe("Accessibility › Insert Without the Clipboard: the live state", () => {
  test("the state flips once the user adds the source in System Settings", async () => {
    let added = false;
    mockCommand("getInputMethodStatus", async () =>
      status({ installed: true, added }),
    );
    await renderAccessibilityPage(null);
    expect(setUpButton()).not.toBeNull();
    added = true;
    await waitFor(() => expect(visibleText()).toContain(ADDED), {
      timeout: 2500,
    });
    expect(setUpButton()).toBeNull();
  });

  test("it stops asking when the page closes", async () => {
    mockCommand("getInputMethodStatus", async () => status({}));
    const page = await renderAccessibilityPage(null);
    await waitFor(
      () => expect(callsOf("getInputMethodStatus").length).toBeGreaterThan(1),
      { timeout: 2500 },
    );
    page.unmount();
    const asked = callsOf("getInputMethodStatus").length;
    await new Promise((resolve) => setTimeout(resolve, 1300));
    expect(callsOf("getInputMethodStatus").length).toBe(asked);
  });
});

describe("Accessibility › Insert Without the Clipboard in German", () => {
  test("the terms of macOS and of the app", async () => {
    await switchTestLanguage("de");
    await renderAccessibilityPage({ available: true });
    const text = visibleText();
    expect(text).toContain("Ohne Zwischenablage einfügen");
    expect(text).toContain("Eingabequelle");
    expect(text).toContain("Endtasten");
    expect(text).toContain("„Letzte Transkription einfügen“");
    expect(text).toContain(
      "Klick unter „Tastatur“ neben „Eingabequellen“ auf „Bearbeiten …“, dann auf „+“, such nach „DanyWhere“ und klick auf „Hinzufügen“.",
    );
    expect(
      screen.queryByRole("button", {
        name: "In den Systemeinstellungen hinzufügen",
      }),
    ).not.toBeNull();
    expect(text).not.toContain(TITLE);
    expect(rawTextIn()).toEqual([]);
  });
});
