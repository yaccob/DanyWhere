// DanyWhere: the card Start Shortcuts on General (sidecar qa 2026-11-26_1713,
// "Konzeptvorschlag"; 2026-11-26_1810, "Schnittstelle Phase 1"): a list of
// combinations without names, each with "Also a hold shortcut" where a hold
// shortcut has exactly its keys. "Add Start Shortcut" records a combination
// first; the last start or hold shortcut of a layer can't go; the backend's
// refusals read as sentences; a profile inherits, overrides and switches off
// entries one by one.
import { describe, expect, test } from "bun:test";
import { fireEvent, screen, within } from "@testing-library/react";
import { GeneralSettings } from "@/components/settings/general/GeneralSettings";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { callsOf, err, mockCommand } from "@/test/bindings-mock";
import {
  holdKey,
  settingsWith,
  startKey,
  type SettingsSpec,
} from "@/test/fixtures";
import { switchTestLanguage } from "@/test/i18n";
import {
  press,
  pressKeys,
  raisedNotices,
  renderPage,
  settle,
} from "@/test/render";
import { setOsType } from "@/test/tauri-mock";
import { rawTextIn, visibleText } from "@/test/visible";

const START = startKey("start", "ctrl+space");
const OTHER = startKey("start_1795307900001", "ctrl+alt+e");
const DIRECT = holdKey("hold_direct", "ctrl+space");
const POST = holdKey("hold_post", "ctrl+shift+space");

const renderGeneral = (
  spec: SettingsSpec = {},
  scope: Scope | null = DEFAULTS_SCOPE,
  acceptWrites = true,
) =>
  renderPage(<GeneralSettings />, {
    settings: settingsWith(spec),
    scope,
    acceptWrites,
  });

/** The card under the heading `title`. */
const card = (title: string): HTMLElement => {
  const heading = screen.getByRole("heading", { level: 2, name: title });
  const group = heading.closest(".space-y-2");
  if (!(group instanceof HTMLElement)) throw new Error(`no card ${title}`);
  return group;
};
const startCard = () => card("Start Shortcuts");

/** The value the page last asked the backend to store under `field`. */
const stored = (field: string): unknown => {
  const calls = callsOf("setProfileValue").filter(([, f]) => f === field);
  expect(calls.length).toBeGreaterThan(0);
  const [scope, , value] = calls[calls.length - 1];
  expect(scope).toEqual(DEFAULTS_SCOPE);
  return value;
};

const ALSO_HOLD = "Also a hold shortcut";
const LAST_ONE =
  "One start or hold shortcut must stay, so that a recording can always be started.";

describe("Start Shortcuts under the defaults", () => {
  test("each start shortcut shows its combination, and Also a hold shortcut where a hold shortcut has exactly its keys", async () => {
    await renderGeneral({
      defaults: {
        start_keys: [START, OTHER],
        hold_keys: [holdKey("hold_direct", "ctrl_left+space"), POST],
      },
    });

    // ⌃ Space against a held left ⌃ Space is no exact match.
    expect(visibleText(startCard())).not.toContain(ALSO_HOLD);
    expect(within(startCard()).getByText("⌥⌃E")).toBeTruthy();
  });

  test("the hint shows once per start shortcut that is also a hold shortcut", async () => {
    await renderGeneral({
      defaults: { start_keys: [START, OTHER], hold_keys: [DIRECT, POST] },
    });

    const text = visibleText(startCard());
    expect(text.split(ALSO_HOLD).length - 1).toBe(1);
    expect(text.indexOf("⌃ Space")).toBeLessThan(text.indexOf(ALSO_HOLD));
    expect(text.indexOf(ALSO_HOLD)).toBeLessThan(text.indexOf("⌥⌃E"));
  });

  test("deleting a start shortcut stores the list without it", async () => {
    await renderGeneral({
      defaults: { start_keys: [START, OTHER], hold_keys: [DIRECT, POST] },
    });

    const remove = screen.getByRole("button", {
      name: "Delete start shortcut ⌥⌃E",
    });
    expect((remove as HTMLButtonElement).disabled).toBe(false);
    await press(remove);

    expect(stored("start_keys")).toEqual([START]);
  });

  test("a start shortcut's own combination: click it to record another one", async () => {
    await renderGeneral();

    await press(within(startCard()).getByRole("button", { name: "⌃ Space" }));
    await pressKeys("ControlLeft", "AltLeft", "KeyS");

    expect(stored("start_keys")).toEqual([
      { id: "start", shortcut: "ctrl+option+s" },
    ]);
    expect(callsOf("resumeAllBindings")).toEqual([[]]);
  });
});

describe("the last start or hold shortcut", () => {
  test("the only start shortcut, without hold shortcuts, can't be deleted, and its button says why", async () => {
    await renderGeneral({ defaults: { start_keys: [START], hold_keys: [] } });

    const remove = screen.getByRole("button", {
      name: "Delete start shortcut ⌃ Space",
    });
    expect((remove as HTMLButtonElement).disabled).toBe(true);
    expect(remove.getAttribute("title")).toBe(LAST_ONE);
  });

  test("the only hold shortcut, without start shortcuts, can't be deleted either", async () => {
    await renderGeneral({ defaults: { start_keys: [], hold_keys: [DIRECT] } });

    const remove = screen.getByRole("button", { name: "Delete ⌃ Space held" });
    expect((remove as HTMLButtonElement).disabled).toBe(true);
    expect(remove.getAttribute("title")).toBe(LAST_ONE);
  });

  test("with a hold shortcut left, the only start shortcut may go", async () => {
    await renderGeneral({
      defaults: { start_keys: [START], hold_keys: [DIRECT] },
    });

    const remove = screen.getByRole("button", {
      name: "Delete start shortcut ⌃ Space",
    });
    expect((remove as HTMLButtonElement).disabled).toBe(false);
    await press(remove);
    expect(stored("start_keys")).toEqual([]);
  });
});

describe("Add Start Shortcut", () => {
  test("records a combination first and adds the start shortcut only with it", async () => {
    await renderGeneral();

    await press(screen.getByRole("button", { name: "Add Start Shortcut" }));
    expect(
      screen.getByText("Press the combination for the new start shortcut"),
    ).toBeTruthy();
    expect(callsOf("suspendAllBindings")).toEqual([[]]);
    expect(callsOf("setProfileValue")).toEqual([]);

    await pressKeys("ControlLeft", "KeyE");

    const list = stored("start_keys") as { id: string; shortcut: string }[];
    expect(list).toHaveLength(2);
    expect(list[0]).toEqual(START);
    expect(list[1].id).toMatch(/^start_\d+$/);
    expect(list[1]).toEqual({ id: list[1].id, shortcut: "ctrl+e" });
    expect(callsOf("resumeAllBindings")).toEqual([[]]);
    expect(
      screen.getByRole("button", { name: "Add Start Shortcut" }),
    ).toBeTruthy();
  });

  test("Escape adds nothing", async () => {
    await renderGeneral();

    await press(screen.getByRole("button", { name: "Add Start Shortcut" }));
    await pressKeys("Escape");

    expect(callsOf("setProfileValue")).toEqual([]);
    expect(callsOf("resumeAllBindings")).toEqual([[]]);
  });

  test("a press elsewhere adds nothing", async () => {
    await renderGeneral();

    await press(screen.getByRole("button", { name: "Add Start Shortcut" }));
    fireEvent.pointerDown(document.body);
    await settle();

    expect(callsOf("setProfileValue")).toEqual([]);
    expect(
      screen.getByRole("button", { name: "Add Start Shortcut" }),
    ).toBeTruthy();
  });
});

describe("outside a scope", () => {
  test("a change goes to the values in use through its own command", async () => {
    await renderGeneral(
      { defaults: { start_keys: [START, OTHER], hold_keys: [DIRECT, POST] } },
      null,
      false,
    );

    await press(
      screen.getByRole("button", { name: "Delete start shortcut ⌥⌃E" }),
    );

    expect(callsOf("changeStartKeysSetting")).toEqual([[[START]]]);
    expect(callsOf("setProfileValue")).toEqual([]);
  });
});

/** The refusals of the backend (qa 2026-11-26_1810, "Prüfung und
 * Garantien") and what the notice says. */
const REFUSALS: Array<[string, string]> = [
  [
    "start-key-needs-shortcut:start_1795307900001",
    "Every start shortcut needs a key combination.",
  ],
  [
    "start-key-shortcut-twice:ctrl+space",
    "Two start shortcuts can't share a combination.",
  ],
  [
    "start-key-last",
    "The last start or hold shortcut can't go: a recording must always be possible to start.",
  ],
  [
    "start-key-cannot-end:start",
    "On Linux a recording that ⌃ Space starts couldn't be ended: keep an end shortcut with the same combination.",
  ],
  [
    "hold-key-needs-shortcut:hold_post",
    "Every hold shortcut needs a key combination.",
  ],
  [
    "hold-key-shortcut-twice:ctrl+space",
    "Two hold shortcuts can't share a combination.",
  ],
  [
    "conflict:start_key:start",
    "Start shortcut ⌃ Space already uses this shortcut",
  ],
  [
    "conflict:hold_key:hold_post",
    "Hold shortcut ⌃⇧ Space held already uses this shortcut",
  ],
  ["conflict:end_key:plain", "End shortcut ⌃ Space already uses this shortcut"],
  [
    "conflict:paste_last_transcript",
    "Paste Last Transcript already uses this shortcut",
  ],
];

describe("refusals of the backend", () => {
  for (const [error, text] of REFUSALS) {
    test(`${error.split(":")[0]} ${error.split(":")[1] ?? ""} reads as its sentence`, async () => {
      mockCommand("setProfileValue", async () => err(error));
      await renderGeneral(
        { defaults: { start_keys: [START, OTHER], hold_keys: [DIRECT, POST] } },
        DEFAULTS_SCOPE,
        false,
      );
      await press(
        screen.getByRole("button", { name: "Delete start shortcut ⌥⌃E" }),
      );

      expect(raisedNotices()).toEqual([text]);
    });
  }

  test("on Linux the start shortcut that couldn't end is named as Linux writes it", async () => {
    setOsType("linux");
    mockCommand("setProfileValue", async () =>
      err("start-key-cannot-end:start"),
    );
    await renderGeneral(
      { defaults: { start_keys: [START, OTHER], hold_keys: [DIRECT, POST] } },
      DEFAULTS_SCOPE,
      false,
    );
    await press(
      screen.getByRole("button", { name: "Delete start shortcut Ctrl+Alt+E" }),
    );

    expect(raisedNotices()).toEqual([
      "On Linux a recording that Ctrl+Space starts couldn't be ended: keep an end shortcut with the same combination.",
    ]);
  });
});

describe("Start Shortcuts in a profile", () => {
  const WORK = "profile_1795300000001";
  const spec: SettingsSpec = {
    profiles: [
      {
        id: WORK,
        name: "Work",
        values: { start_keys: [OTHER] },
        ownEntries: { start_keys: { own: [OTHER.id], off: [START.id] } },
      },
      { id: "profile_1795300000002", name: "Private" },
    ],
  };

  test("the profile's own and switched-off start shortcuts are named by their keys, not by their id", async () => {
    await renderGeneral(spec, { kind: "profile", id: WORK });

    const text = visibleText(startCard());
    expect(text).toContain("⌥⌃E own");
    expect(text).toContain("⌃ Space off");
    expect(screen.getByRole("button", { name: "Reset ⌥⌃E" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reset ⌃ Space" })).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
  });

  test("a start shortcut added in a profile is written into the profile", async () => {
    await renderGeneral(spec, { kind: "profile", id: WORK });

    await press(screen.getByRole("button", { name: "Add Start Shortcut" }));
    await pressKeys("ControlLeft", "KeyR");

    const calls = callsOf("setProfileValue");
    expect(calls).toHaveLength(1);
    const [scope, field, value] = calls[0];
    expect(scope).toEqual({ kind: "profile", id: WORK });
    expect(field).toBe("start_keys");
    expect((value as { shortcut: string }[]).map((s) => s.shortcut)).toEqual([
      "ctrl+alt+e",
      "ctrl+r",
    ]);
  });

  test("under the defaults, the deviating profile lists its start shortcuts by their keys", async () => {
    await renderGeneral(spec);

    const text = visibleText(startCard());
    expect(text).toContain("Work own: ⌥⌃E · off: ⌃ Space");
    expect(rawTextIn()).toEqual([]);
  });
});

describe("Start Shortcuts in German", () => {
  test("the card, its hint and its button read in German", async () => {
    await switchTestLanguage("de");
    await renderGeneral();

    const text = visibleText(card("Starttasten"));
    expect(text).toContain(
      "Antippen startet eine Aufnahme. Sie läuft, bis eine Endtaste sie beendet.",
    );
    expect(text).toContain("Auch Haltetaste");
    expect(
      screen.getByRole("button", { name: "Starttaste hinzufügen" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Starttaste ⌃ Leertaste löschen" }),
    ).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
  });

  test("Add Start Shortcut asks in German", async () => {
    await switchTestLanguage("de");
    await renderGeneral();

    await press(screen.getByRole("button", { name: "Starttaste hinzufügen" }));
    expect(
      screen.getByText("Drück die Tastenkombination für die neue Starttaste"),
    ).toBeTruthy();
  });
});
