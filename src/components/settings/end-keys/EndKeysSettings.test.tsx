// DanyWhere: the page of the end shortcuts (qa 2026-11-22_2202, "Noch nicht
// geprüft"): each end's name field and combination, the hint for an end with
// a start shortcut's combination, the last end that can't go, "Add End
// Shortcut", and refusals of the backend as sentences. These replace the
// checks of the page that were left for a GUI run in Dev.
import { describe, expect, test } from "bun:test";
import { fireEvent, screen } from "@testing-library/react";
import type { EndKey } from "@/bindings";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { EndKeysSettings } from "./EndKeysSettings";
import { callsOf, err, mockCommand } from "@/test/bindings-mock";
import { endKey, settingsWith, type SettingsSpec } from "@/test/fixtures";
import {
  pressKeys,
  press,
  raisedNotices,
  renderPage,
  settle,
  typeKeys,
} from "@/test/render";
import { switchTestLanguage } from "@/test/i18n";
import { setOsType } from "@/test/tauri-mock";
import { rawTextIn, visibleText } from "@/test/visible";

const PLAIN = endKey("plain", "ctrl+space");
const SEND = endKey("end_1795307754299", "ctrl+return", { name: "Senden" });
const UNNAMED = endKey("end_1795307800000", "ctrl+alt+j");
const WITH_POST = endKey("end_1795307811111", "ctrl+alt+k", {
  post_process: true,
});

const renderEnds = (
  ends: EndKey[],
  scope: Scope = DEFAULTS_SCOPE,
  spec: SettingsSpec = {},
  acceptWrites = true,
) =>
  renderPage(<EndKeysSettings />, {
    settings: settingsWith({
      ...spec,
      defaults: { end_keys: ends, ...spec.defaults },
    }),
    scope,
    acceptWrites,
  });

/** The list the page last asked the backend to store. */
const storedEnds = (): EndKey[] => {
  const calls = callsOf("setProfileValue");
  expect(calls.length).toBeGreaterThan(0);
  const [scope, field, value] = calls[calls.length - 1];
  expect(scope).toEqual(DEFAULTS_SCOPE);
  expect(field).toBe("end_keys");
  return value as unknown as EndKey[];
};

const nameFields = () => screen.getAllByLabelText("Name") as HTMLInputElement[];

describe("End Shortcuts under the defaults", () => {
  test("each end shows its own name, or else its combination, under the label Name", async () => {
    await renderEnds([PLAIN, SEND, UNNAMED]);

    expect(nameFields().map((field) => field.value)).toEqual([
      "⌃ Space",
      "Senden",
      "⌥⌃J",
    ]);
    // The combinations on their buttons, written as macOS does.
    for (const keys of ["⌃ Space", "⌃↩", "⌥⌃J"]) {
      expect(screen.getAllByText(keys).length).toBeGreaterThan(0);
    }
    expect(rawTextIn()).toEqual([]);
  });

  test("an end with a start shortcut's combination names that start shortcut", async () => {
    await renderEnds([
      PLAIN,
      endKey("post_process", "ctrl+shift+space", { post_process: true }),
      SEND,
    ]);

    const text = visibleText();
    expect(text).toContain(
      "Same combination as “Start Shortcut”: pressing it again during a recording ends this way.",
    );
    expect(text).toContain(
      "Same combination as “Second Start Shortcut”: pressing it again during a recording ends this way.",
    );
    expect(text.match(/Same combination as/g)?.length).toBe(2);
  });

  test("the last end can't be deleted, and its button says why", async () => {
    await renderEnds([SEND]);

    const remove = screen.getByRole("button", { name: "Delete Senden" });
    expect((remove as HTMLButtonElement).disabled).toBe(true);
    expect(remove.getAttribute("title")).toBe(
      "One end shortcut must stay, so that a recording can always be ended.",
    );
  });

  test("deleting an end stores the list without it", async () => {
    await renderEnds([PLAIN, SEND, UNNAMED]);

    const remove = screen.getByRole("button", { name: "Delete Senden" });
    expect((remove as HTMLButtonElement).disabled).toBe(false);
    await press(remove);

    expect(storedEnds().map((end) => end.id)).toEqual([
      "plain",
      "end_1795307800000",
    ]);
  });

  test("a typed name is stored when the field loses focus", async () => {
    await renderEnds([PLAIN, SEND, UNNAMED]);

    const field = nameFields()[2];
    fireEvent.change(field, { target: { value: "Absenden" } });
    fireEvent.blur(field);
    await settle();

    const stored = storedEnds();
    expect(stored.find((end) => end.id === UNNAMED.id)?.name).toBe("Absenden");
    expect(stored.find((end) => end.id === SEND.id)?.name).toBe("Senden");
  });

  test("a blank name stores no name, and the field shows the combination again", async () => {
    await renderEnds([PLAIN, SEND]);

    const field = nameFields()[1];
    fireEvent.change(field, { target: { value: "  " } });
    fireEvent.blur(field);
    await settle();

    expect(storedEnds().find((end) => end.id === SEND.id)?.name).toBe("");
    expect(nameFields()[1].value).toBe("⌃↩");
  });

  test("typing the combination itself as the name stores nothing", async () => {
    await renderEnds([PLAIN, UNNAMED]);

    const field = nameFields()[1];
    fireEvent.change(field, { target: { value: "⌥⌃J" } });
    fireEvent.blur(field);
    await settle();

    expect(callsOf("setProfileValue")).toEqual([]);
  });

  test("Send Context can be checked only together with Post-process", async () => {
    await renderEnds([PLAIN, WITH_POST]);

    const context = screen.getAllByRole("checkbox", {
      name: "Send Context",
    }) as HTMLInputElement[];
    expect(context.map((box) => box.disabled)).toEqual([true, false]);

    const post = screen.getAllByRole("checkbox", { name: "Post-process" });
    await press(post[0]);
    expect(storedEnds()[0]).toEqual({ ...PLAIN, post_process: true });
  });
});

describe("Add End Shortcut", () => {
  test("records a combination first and adds the end only with it", async () => {
    await renderEnds([PLAIN, SEND]);

    await press(screen.getByRole("button", { name: "Add End Shortcut" }));
    expect(
      screen.getByText("Press the combination for the new end shortcut"),
    ).toBeTruthy();
    expect(callsOf("suspendAllBindings")).toEqual([[]]);
    expect(callsOf("setProfileValue")).toEqual([]);

    await pressKeys("ControlLeft", "KeyE");

    const stored = storedEnds();
    expect(stored.slice(0, 2)).toEqual([PLAIN, SEND]);
    expect(stored).toHaveLength(3);
    const added = stored[2];
    expect(added.id).toMatch(/^end_\d+$/);
    expect(added).toEqual({
      id: added.id,
      name: "",
      shortcut: "ctrl+e",
      append_text: "",
      post_process: false,
      send_context: false,
    });
    expect(callsOf("resumeAllBindings")).toEqual([[]]);
    expect(
      screen.getByRole("button", { name: "Add End Shortcut" }),
    ).toBeTruthy();
  });

  test("Escape adds nothing", async () => {
    await renderEnds([PLAIN, SEND]);

    await press(screen.getByRole("button", { name: "Add End Shortcut" }));
    await pressKeys("Escape");

    expect(callsOf("setProfileValue")).toEqual([]);
    expect(callsOf("resumeAllBindings")).toEqual([[]]);
    expect(
      screen.getByRole("button", { name: "Add End Shortcut" }),
    ).toBeTruthy();
  });

  test("a press elsewhere adds nothing", async () => {
    await renderEnds([PLAIN, SEND]);

    await press(screen.getByRole("button", { name: "Add End Shortcut" }));
    fireEvent.pointerDown(document.body);
    await settle();

    expect(callsOf("setProfileValue")).toEqual([]);
    expect(
      screen.getByRole("button", { name: "Add End Shortcut" }),
    ).toBeTruthy();
  });
});

// Without Accessibility (the Tauri backend) a combination is recorded from
// this window's own key events, one at a time, and the page renders after
// each (qa 2026-11-23_1747, point 2: on the company Mac only the last key
// stayed, and the capture never ended).
describe("recording key by key, as without Accessibility", () => {
  const TAURI = { app: { keyboard_implementation: "tauri" as const } };

  test("Add End Shortcut takes every key of the combination", async () => {
    await renderEnds([PLAIN, SEND], DEFAULTS_SCOPE, TAURI);

    await press(screen.getByRole("button", { name: "Add End Shortcut" }));
    await typeKeys("ControlLeft", "ShiftLeft", "KeyY");

    const stored = storedEnds();
    expect(stored).toHaveLength(3);
    expect(stored[2].shortcut).toBe("ctrl+shift+y");
    expect(callsOf("resumeAllBindings")).toEqual([[]]);
    expect(
      screen.getByRole("button", { name: "Add End Shortcut" }),
    ).toBeTruthy();
  });

  test("an end's own combination takes a modifier with Enter", async () => {
    await renderEnds([PLAIN, SEND], DEFAULTS_SCOPE, TAURI);

    await press(screen.getByRole("button", { name: "⌃↩" }));
    await typeKeys("ShiftLeft", "Enter");

    expect(storedEnds().find((end) => end.id === SEND.id)?.shortcut).toBe(
      "shift+enter",
    );
    expect(callsOf("resumeAllBindings")).toEqual([[]]);
  });
});

describe("refusals of the backend", () => {
  const refuse = async (error: string) => {
    mockCommand("setProfileValue", async () => err(error));
    await renderEnds([PLAIN, SEND], DEFAULTS_SCOPE, {}, false);
    await press(screen.getByRole("button", { name: "Delete Senden" }));
    return raisedNotices();
  };

  test("an end shortcut's own refusal reads as its sentence", async () => {
    const notices = await refuse("end-key-shortcut-twice:ctrl+return");
    expect(notices).toEqual(["Two end shortcuts can't share a shortcut."]);
  });

  test("a combination another shortcut has names that shortcut", async () => {
    const notices = await refuse("conflict:paste_last_transcript");
    expect(notices).toEqual([
      "Paste Last Transcript already uses this shortcut",
    ]);
  });

  test("on Linux, a change that would leave no way to end says so", async () => {
    const notices = await refuse("end-key-cannot-end");
    expect(notices).toEqual([
      "On Linux a recording could no longer be ended: keep an end shortcut with a start shortcut's combination, or choose “Hold”.",
    ]);
  });
});

describe("the hint for Linux", () => {
  const HINT = "On Linux, the combinations of end shortcuts aren't taken";

  test("shows on Linux", async () => {
    setOsType("linux");
    await renderEnds([PLAIN]);
    expect(visibleText()).toContain(HINT);
  });

  test("doesn't show on macOS", async () => {
    await renderEnds([PLAIN]);
    expect(visibleText()).not.toContain(HINT);
  });
});

describe("End Shortcuts in a profile", () => {
  const WORK = "profile_1795300000001";
  const OWN = endKey("end_1795307930035", "ctrl+alt+l", { name: "PEnde" });
  const spec: SettingsSpec = {
    profiles: [
      {
        id: WORK,
        name: "Work",
        values: { end_keys: [PLAIN, OWN] },
        ownEntries: { end_keys: { own: [OWN.id], off: [SEND.id] } },
      },
      { id: "profile_1795300000002", name: "Private" },
    ],
  };

  test("the profile's own and switched-off ends are named, not shown by their id", async () => {
    await renderEnds([PLAIN, SEND], { kind: "profile", id: WORK }, spec);

    const text = visibleText();
    expect(text).toContain("PEnde own");
    expect(text).toContain("Senden off");
    expect(screen.getByRole("button", { name: "Reset PEnde" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reset Senden" })).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
  });

  test("under the defaults, the deviating profile lists its ends by name", async () => {
    await renderEnds([PLAIN, SEND], DEFAULTS_SCOPE, spec);

    const text = visibleText();
    expect(text).toContain("Work own: PEnde · off: Senden");
    expect(text).toContain("Private follows");
    expect(rawTextIn()).toEqual([]);
  });
});

// With post-processing off no end post-processes (user, 2026-11-23, option
// a; `app_profiles::effective_post_process`): an end whose "Post-process" is
// checked says so and where to turn it on, for the scope shown. Its box
// stays usable: its value counts once post-processing is on.
describe("End Shortcuts with post-processing off", () => {
  const HINT =
    "Post-processing is off: a dictation ended this way isn't post-processed. Turn on “Post Processing” under Advanced.";
  const hints = () => visibleText().split(HINT).length - 1;
  const WORK = "profile_1795300000001";
  /** "Work" with post-processing as its own value `postProcess`, the
   * defaults with the other one. */
  const work = (postProcess: boolean): SettingsSpec => ({
    defaults: { post_process_enabled: !postProcess },
    profiles: [
      {
        id: WORK,
        name: "Work",
        own: ["post_process_enabled"],
        values: { post_process_enabled: postProcess },
      },
    ],
  });

  test("an end with Post-process checked says that it doesn't post-process", async () => {
    await renderEnds([PLAIN, WITH_POST, SEND]);

    expect(hints()).toBe(1);
    const post = screen.getAllByRole("checkbox", {
      name: "Post-process",
    }) as HTMLInputElement[];
    expect(post.map((box) => [box.checked, box.disabled])).toEqual([
      [false, false],
      [true, false],
      [false, false],
    ]);
    expect(rawTextIn()).toEqual([]);
  });

  test("with post-processing on, no end says so", async () => {
    await renderEnds([PLAIN, WITH_POST], DEFAULTS_SCOPE, {
      defaults: { post_process_enabled: true },
    });

    expect(hints()).toBe(0);
  });

  test("checking Post-process with post-processing off brings the hint", async () => {
    await renderEnds([PLAIN, SEND]);
    expect(hints()).toBe(0);

    const post = screen.getAllByRole("checkbox", { name: "Post-process" });
    await press(post[1]);

    expect(storedEnds()[1]).toEqual({ ...SEND, post_process: true });
    expect(hints()).toBe(1);
  });

  test("in a profile its own switch counts, not the defaults'", async () => {
    const scope: Scope = { kind: "profile", id: WORK };

    await renderEnds([PLAIN, WITH_POST], scope, work(false));
    expect(hints()).toBe(1);
    expect(rawTextIn()).toEqual([]);
  });

  test("in a profile with post-processing on, no end says so", async () => {
    await renderEnds(
      [PLAIN, WITH_POST],
      { kind: "profile", id: WORK },
      work(true),
    );

    expect(hints()).toBe(0);
  });

  test("the hint reads in German too", async () => {
    await switchTestLanguage("de");
    await renderEnds([PLAIN, WITH_POST]);

    expect(visibleText()).toContain(
      "Die Nachbearbeitung ist aus: Ein so beendetes Diktat wird nicht nachbearbeitet. Schalte „Nachbearbeitung“ unter Erweitert ein.",
    );
    expect(rawTextIn()).toEqual([]);
  });
});

// DanyWhere: the "?" next to the group's title opens the manual's chapter on
// start and end shortcuts (`open_manual`, src-tauri/src/manual.rs).
describe("the manual's chapter from End Shortcuts", () => {
  const CHAPTER = "how-it-works/start-and-end-shortcuts/";
  const helpButton = () =>
    screen.getByRole("button", {
      name: "Open the manual: Start and End Shortcuts",
    });

  test("a button next to the title opens the chapter", async () => {
    await renderEnds([PLAIN, SEND]);

    const title = screen.getByRole("heading", { name: "End Shortcuts" });
    expect(title.parentElement?.contains(helpButton())).toBe(true);

    await press(helpButton());

    expect(callsOf("openManual")).toEqual([[CHAPTER]]);
    expect(callsOf("setProfileValue")).toEqual([]);
  });

  test("shows a question mark icon and a tooltip", async () => {
    await renderEnds([PLAIN, SEND]);

    // Lucide's CircleHelp, the same "?" in a circle as the Models legend.
    const icon = helpButton().querySelector("svg");
    expect(icon?.getAttribute("class")).toContain(
      "lucide-circle-question-mark",
    );
    expect(icon?.getAttribute("aria-hidden")).toBe("true");
    expect(helpButton().textContent).toBe("");
    expect(helpButton().getAttribute("title")).toBe(
      "Open the manual: Start and End Shortcuts",
    );
    expect(rawTextIn()).toEqual([]);
  });

  test("is there in a profile too", async () => {
    const work = "profile_1795300000001";
    await renderEnds(
      [PLAIN, SEND],
      { kind: "profile", id: work },
      { profiles: [{ id: work, name: "Work" }] },
    );

    await press(helpButton());

    expect(callsOf("openManual")).toEqual([[CHAPTER]]);
  });

  test("names the chapter in German", async () => {
    await switchTestLanguage("de");
    await renderEnds([PLAIN, SEND]);

    await press(
      screen.getByRole("button", {
        name: "Handbuch öffnen: Start- und Endtasten",
      }),
    );

    expect(callsOf("openManual")).toEqual([[CHAPTER]]);
    expect(rawTextIn()).toEqual([]);
  });

  test("says so when the manual can't open", async () => {
    mockCommand("openManual", async () => err("no window"));
    await renderEnds([PLAIN, SEND]);

    await press(helpButton());

    expect(raisedNotices()).toEqual(["Couldn't open the manual: no window"]);
  });
});
