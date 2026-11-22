// DanyWhere: a shortcut field on a page of the defaults or of a profile
// (qa 2026-11-22_2202, "Noch nicht geprüft": "Ob die Seite eine Ablehnung
// beim Ändern einer Starttaste lesbar zeigt, ist offen"). A recorded
// combination goes to the scope; when the backend refuses it, the notice
// says why in words, once, as it does for the end shortcuts, never with the
// backend's code or a binding id.
import { describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { ShortcutInput } from "./ShortcutInput";
import { callsOf, err, mockCommand } from "@/test/bindings-mock";
import { binding, settingsWith } from "@/test/fixtures";
import { press, pressKeys, raisedNotices, renderPage } from "@/test/render";
import { setOsType } from "@/test/tauri-mock";

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
