// DanyWhere: a shortcut field on a page of the defaults or of a profile
// (qa 2026-11-22_2202, "Noch nicht geprüft": "Ob die Seite eine Ablehnung
// beim Ändern einer Starttaste lesbar zeigt, ist offen"). A recorded
// combination goes to the scope; when the backend refuses it, the notice
// says why in words, once, as it does for the end shortcuts, never with the
// backend's code or a binding id.
import { describe, expect, test } from "bun:test";
import { act, fireEvent, screen } from "@testing-library/react";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { ShortcutInput } from "./ShortcutInput";
import { callsOf, err, mockCommand } from "@/test/bindings-mock";
import { binding, settingsWith } from "@/test/fixtures";
import i18n, { switchTestLanguage } from "@/test/i18n";
import {
  press,
  pressKeys,
  raisedNotices,
  renderPage,
  typeKeys,
} from "@/test/render";
import { setOsType } from "@/test/tauri-mock";
import { rawTextIn } from "@/test/visible";

const WORK = "profile_1795300000001";

const renderStart = (scope: Scope = DEFAULTS_SCOPE) =>
  renderPage(<ShortcutInput shortcutId="transcribe" />, {
    settings: settingsWith({ profiles: [{ id: WORK, name: "Work" }] }),
    scope,
  });

/** Records Ctrl+E for the start shortcut, whose combination its field
 * shows as `shown` (macOS writes it with symbols). */
const recordCtrlE = async (shown = "⌃ Space") => {
  await press(screen.getByText(shown));
  await pressKeys("ControlLeft", "KeyE");
};

describe("a start shortcut's field in a scope", () => {
  test("stores the recorded combination in the page's scope", async () => {
    await renderStart();
    await recordCtrlE();

    expect(callsOf("setProfileEntry")).toEqual([
      [
        DEFAULTS_SCOPE,
        "bindings",
        "transcribe",
        binding("transcribe", "ctrl+space", { current_binding: "ctrl+e" }),
      ],
    ]);
    expect(raisedNotices()).toEqual([]);
  });

  test("a combination another shortcut has names that shortcut, once", async () => {
    mockCommand("setProfileEntry", async () =>
      err("conflict:paste_last_transcript"),
    );
    await renderStart();
    await recordCtrlE();

    expect(raisedNotices()).toEqual([
      "Paste Last Transcript already uses this shortcut",
    ]);
  });

  test("in a profile too", async () => {
    mockCommand("setProfileEntry", async () => err("conflict:cancel"));
    await renderStart({ kind: "profile", id: WORK });
    await recordCtrlE();

    expect(raisedNotices()).toEqual([
      "Cancel Shortcut already uses this shortcut",
    ]);
  });

  test("on Linux, a combination after which no end would work says so", async () => {
    setOsType("linux");
    mockCommand("setProfileEntry", async () => err("end-key-cannot-end"));
    await renderStart();
    await recordCtrlE("Ctrl+Space");

    expect(raisedNotices()).toEqual([
      "On Linux a recording could no longer be ended: keep an end shortcut with a start shortcut's combination, or choose “Hold”.",
    ]);
  });

  test("another refusal keeps the backend's reason as the detail", async () => {
    mockCommand("setProfileEntry", async () => err("Invalid shortcut 'e'"));
    await renderStart();
    await recordCtrlE();

    expect(raisedNotices()).toEqual([
      "Couldn't save the change | Invalid shortcut 'e'",
    ]);
  });
});

// Without Accessibility (the Tauri backend) the field records this window's
// own key events, one at a time, with the page rendering after each
// (qa 2026-11-23_1747, point 2).
describe("recording key by key, as without Accessibility", () => {
  test("a start shortcut's field in a scope takes every key", async () => {
    await renderPage(<ShortcutInput shortcutId="transcribe" />, {
      settings: settingsWith({ app: { keyboard_implementation: "tauri" } }),
      scope: DEFAULTS_SCOPE,
    });
    await press(screen.getByText("⌃ Space"));
    await typeKeys("ControlLeft", "AltLeft", "KeyY");

    expect(callsOf("setProfileEntry")).toEqual([
      [
        DEFAULTS_SCOPE,
        "bindings",
        "transcribe",
        binding("transcribe", "ctrl+space", {
          current_binding: "ctrl+option+y",
        }),
      ],
    ]);
  });
});

// What the second start shortcut does out of the box differs by system
// since macOS has twelve factory ends (end_keys.rs, `factory_end_keys_on`,
// `factory_hold_ends_on`): there its second press is "Direct · No space",
// without post-processing, and releasing it after holding post-processes.
// Windows and Linux keep an end with its combination that post-processes.
describe("the second start shortcut's description", () => {
  const renderSecond = () =>
    renderPage(<ShortcutInput shortcutId="transcribe_with_post_process" />, {
      settings: settingsWith(),
      scope: DEFAULTS_SCOPE,
    });

  /** The text of the info button's tooltip, as a press shows it, in the
   * test's language. */
  const description = async (): Promise<string | null> => {
    const info = screen.getByRole("button", {
      name: i18n.t("common.moreInformation"),
    });
    await act(async () => {
      fireEvent.pointerDown(info);
    });
    return screen.getByRole("tooltip").textContent;
  };

  test("on macOS, says that holding post-processes and a second press doesn't", async () => {
    await renderSecond();

    expect(await description()).toBe(
      "Optional: a second combination that starts a recording. Out of the box, releasing it after holding it post-processes the text; pressing it again ends without post-processing, as every end shortcut without ⌥ does. Taken only while post-processing is on.",
    );
    expect(rawTextIn()).toEqual([]);
  });

  test("on macOS in German too", async () => {
    await switchTestLanguage("de");
    await renderSecond();

    expect(await description()).toBe(
      "Optional: eine zweite Tastenkombination, die eine Aufnahme startet. Ab Werk wird der Text nachbearbeitet, wenn man sie hält und loslässt; ein erneuter Druck beendet ohne Nachbearbeitung, wie jede Endtaste ohne ⌥. Nur belegt, solange die Nachbearbeitung eingeschaltet ist.",
    );
  });

  const ELSEWHERE =
    "Optional: a second combination that starts a recording. Out of the box, an end shortcut with the same combination post-processes the text. Taken only while post-processing is on.";

  test("on Windows, says that the end with its combination post-processes", async () => {
    setOsType("windows");
    await renderSecond();

    expect(await description()).toBe(ELSEWHERE);
    expect(rawTextIn()).toEqual([]);
  });

  test("on Linux too", async () => {
    setOsType("linux");
    await renderSecond();

    expect(await description()).toBe(ELSEWHERE);
  });
});
