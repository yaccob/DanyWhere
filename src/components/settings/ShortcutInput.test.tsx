// DanyWhere: a shortcut field on a page of the defaults or of a profile
// (qa 2026-11-22_2202, "Noch nicht geprüft": "Ob die Seite eine Ablehnung
// beim Ändern einer Starttaste lesbar zeigt, ist offen"). A recorded
// combination goes to the scope; when the backend refuses it, the notice
// says why in words, once, as it does for the end shortcuts, never with the
// backend's code or a binding id. Start shortcuts are a list of their own
// since 2026-11-26 (start-keys/StartKeysSettings.test.tsx); the field is the
// other shortcuts', here "Paste Last Transcript".
import { describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { ShortcutInput } from "./ShortcutInput";
import { callsOf, err, mockCommand } from "@/test/bindings-mock";
import { binding, settingsWith } from "@/test/fixtures";
import {
  press,
  pressKeys,
  raisedNotices,
  renderPage,
  typeKeys,
} from "@/test/render";
import { setOsType } from "@/test/tauri-mock";

const WORK = "profile_1795300000001";

const PASTE = "paste_last_transcript";

const renderPaste = (scope: Scope = DEFAULTS_SCOPE) =>
  renderPage(<ShortcutInput shortcutId={PASTE} />, {
    settings: settingsWith({ profiles: [{ id: WORK, name: "Work" }] }),
    scope,
  });

/** Records Ctrl+E for the paste shortcut, whose combination its field
 * shows as `shown` (macOS writes it with symbols). */
const recordCtrlE = async (shown = "⌃⇧V") => {
  await press(screen.getByText(shown));
  await pressKeys("ControlLeft", "KeyE");
};

describe("a shortcut's field in a scope", () => {
  test("stores the recorded combination in the page's scope", async () => {
    await renderPaste();
    await recordCtrlE();

    expect(callsOf("setProfileEntry")).toEqual([
      [
        DEFAULTS_SCOPE,
        "bindings",
        PASTE,
        binding(PASTE, "ctrl+shift+v", { current_binding: "ctrl+e" }),
      ],
    ]);
    expect(raisedNotices()).toEqual([]);
  });

  test("a combination another shortcut has names that shortcut, once", async () => {
    mockCommand("setProfileEntry", async () =>
      err("conflict:copy_last_transcript"),
    );
    await renderPaste();
    await recordCtrlE();

    expect(raisedNotices()).toEqual([
      "Copy Last Transcript already uses this shortcut",
    ]);
  });

  test("a combination a hold shortcut has names that hold shortcut", async () => {
    mockCommand("setProfileEntry", async () =>
      err("conflict:hold_key:hold_post"),
    );
    await renderPaste();
    await recordCtrlE();

    expect(raisedNotices()).toEqual([
      "Hold shortcut ⌃⇧ Space held already uses this shortcut",
    ]);
  });

  test("in a profile too", async () => {
    mockCommand("setProfileEntry", async () => err("conflict:cancel"));
    await renderPaste({ kind: "profile", id: WORK });
    await recordCtrlE();

    expect(raisedNotices()).toEqual([
      "Cancel Shortcut already uses this shortcut",
    ]);
  });

  test("on Linux, a combination after which no end would work says so", async () => {
    setOsType("linux");
    mockCommand("setProfileEntry", async () => err("end-key-cannot-end"));
    await renderPaste();
    await recordCtrlE("Ctrl+Shift+V");

    expect(raisedNotices()).toEqual([
      "On Linux a recording could no longer be ended: keep an end shortcut with a start shortcut's combination.",
    ]);
  });

  test("another refusal keeps the backend's reason as the detail", async () => {
    mockCommand("setProfileEntry", async () => err("Invalid shortcut 'e'"));
    await renderPaste();
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
  test("a shortcut's field in a scope takes every key", async () => {
    await renderPage(<ShortcutInput shortcutId={PASTE} />, {
      settings: settingsWith({ app: { keyboard_implementation: "tauri" } }),
      scope: DEFAULTS_SCOPE,
    });
    await press(screen.getByText("⌃⇧V"));
    await typeKeys("ControlLeft", "AltLeft", "KeyY");

    expect(callsOf("setProfileEntry")).toEqual([
      [
        DEFAULTS_SCOPE,
        "bindings",
        PASTE,
        binding(PASTE, "ctrl+shift+v", {
          current_binding: "ctrl+option+y",
        }),
      ],
    ]);
  });
});
