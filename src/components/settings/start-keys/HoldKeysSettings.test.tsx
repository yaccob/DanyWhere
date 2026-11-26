// DanyWhere: the card Hold Shortcuts on General (sidecar qa 2026-11-26_1713,
// "Konzeptvorschlag" and question 4 a; 2026-11-26_1810): the editor of the
// end shortcuts, with a name that follows the keys ("⌃ Space held") until
// one is typed, and under each entry whether a short tap starts a recording
// too. A hold shortcut that post-processes is taken only while some dictation
// can post-process; the page says so at it.
import { describe, expect, test } from "bun:test";
import { fireEvent, screen, within } from "@testing-library/react";
import type { EndKey } from "@/bindings";
import { GeneralSettings } from "@/components/settings/general/GeneralSettings";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { callsOf } from "@/test/bindings-mock";
import {
  holdKey,
  settingsWith,
  startKey,
  type SettingsSpec,
} from "@/test/fixtures";
import { switchTestLanguage } from "@/test/i18n";
import { press, pressKeys, renderPage, settle } from "@/test/render";
import { rawTextIn, visibleText } from "@/test/visible";

const START = startKey("start", "ctrl+space");
const DIRECT = holdKey("hold_direct", "ctrl+space");
const POST = holdKey("hold_post", "ctrl+shift+space", { post_process: true });

const renderGeneral = (
  spec: SettingsSpec = {},
  scope: Scope = DEFAULTS_SCOPE,
) =>
  renderPage(<GeneralSettings />, {
    settings: settingsWith(spec),
    scope,
    acceptWrites: true,
  });

const holdCard = (title = "Hold Shortcuts"): HTMLElement => {
  const heading = screen.getByRole("heading", { level: 2, name: title });
  const group = heading.closest(".space-y-2");
  if (!(group instanceof HTMLElement)) throw new Error(`no card ${title}`);
  return group;
};

const nameFields = () =>
  within(holdCard()).getAllByLabelText("Name") as HTMLInputElement[];

/** The hold shortcuts the page last asked the backend to store. */
const storedHolds = (): EndKey[] => {
  const calls = callsOf("setProfileValue").filter(
    ([, field]) => field === "hold_keys",
  );
  expect(calls.length).toBeGreaterThan(0);
  const [scope, , value] = calls[calls.length - 1];
  expect(scope).toEqual(DEFAULTS_SCOPE);
  return value as unknown as EndKey[];
};

/** The row of the hold shortcut whose name field shows `name`. */
const rowOf = (name: string): HTMLElement => {
  const field = nameFields().find((input) => input.value === name);
  if (!field) throw new Error(`no hold shortcut ${name}`);
  let row: HTMLElement | null = field;
  while (row && !row.parentElement?.className.includes("divide-y")) {
    row = row.parentElement;
  }
  if (!row) throw new Error(`no row for ${name}`);
  return row;
};

const ALSO_START =
  "Also a start shortcut: a short tap starts a recording that keeps running.";
const HOLD_ONLY = "Hold shortcut only: a short tap does nothing.";
const NOT_TAKEN =
  "Taken only while post-processing is on: until then, its combination stays with the app in front.";
const POST_OFF =
  "Post-processing is off: a dictation ended this way isn't post-processed. Turn on “Post Processing” under Advanced.";

describe("Hold Shortcuts: names", () => {
  test("a hold shortcut without a name of its own is called after its keys, held", async () => {
    await renderGeneral();

    expect(nameFields().map((field) => field.value)).toEqual([
      "⌃ Space held",
      "⌃⇧ Space held",
    ]);
    expect(rawTextIn()).toEqual([]);
  });

  test("a typed name is stored when the field loses focus", async () => {
    await renderGeneral();

    const field = nameFields()[1];
    fireEvent.change(field, { target: { value: "Polieren" } });
    fireEvent.blur(field);
    await settle();

    expect(storedHolds()).toEqual([DIRECT, { ...POST, name: "Polieren" }]);
  });

  test("a blank name stores no name, and the field follows the keys again", async () => {
    await renderGeneral({
      defaults: { hold_keys: [DIRECT, { ...POST, name: "Polieren" }] },
    });

    const field = nameFields()[1];
    fireEvent.change(field, { target: { value: "  " } });
    fireEvent.blur(field);
    await settle();

    expect(storedHolds()[1].name).toBe("");
    expect(nameFields()[1].value).toBe("⌃⇧ Space held");
  });

  test("typing the name it follows stores nothing", async () => {
    await renderGeneral();

    const field = nameFields()[0];
    fireEvent.change(field, { target: { value: "⌃ Space held" } });
    fireEvent.blur(field);
    await settle();

    expect(callsOf("setProfileValue")).toEqual([]);
  });
});

describe("Hold Shortcuts: what a short tap does", () => {
  test("a hold shortcut that is a start shortcut too says that a tap starts", async () => {
    await renderGeneral();

    expect(visibleText(rowOf("⌃ Space held"))).toContain(ALSO_START);
    expect(visibleText(rowOf("⌃ Space held"))).not.toContain(HOLD_ONLY);
    expect(visibleText(rowOf("⌃⇧ Space held"))).toContain(HOLD_ONLY);
  });

  test("a start shortcut on a side of the keys is another combination", async () => {
    await renderGeneral({
      defaults: { start_keys: [startKey("start", "ctrl_left+space")] },
    });

    expect(visibleText(rowOf("⌃ Space held"))).toContain(HOLD_ONLY);
  });
});

describe("Hold Shortcuts: values", () => {
  test("Post-process is stored with the list, Send Context only together with it", async () => {
    await renderGeneral();

    const context = within(holdCard()).getAllByRole("checkbox", {
      name: "Send Context",
    }) as HTMLInputElement[];
    expect(context.map((box) => box.disabled)).toEqual([true, false]);

    const post = within(holdCard()).getAllByRole("checkbox", {
      name: "Post-process",
    });
    await press(post[0]);
    expect(storedHolds()[0]).toEqual({ ...DIRECT, post_process: true });
  });

  test("deleting a hold shortcut stores the list without it", async () => {
    await renderGeneral();

    await press(screen.getByRole("button", { name: "Delete ⌃⇧ Space held" }));
    expect(storedHolds()).toEqual([DIRECT]);
  });
});

describe("Add Hold Shortcut", () => {
  test("records a combination first and adds the hold shortcut only with it", async () => {
    await renderGeneral();

    await press(screen.getByRole("button", { name: "Add Hold Shortcut" }));
    expect(
      screen.getByText("Press the combination for the new hold shortcut"),
    ).toBeTruthy();
    expect(callsOf("setProfileValue")).toEqual([]);

    await pressKeys("ControlLeft", "KeyE");

    const holds = storedHolds();
    expect(holds.slice(0, 2)).toEqual([DIRECT, POST]);
    expect(holds).toHaveLength(3);
    expect(holds[2].id).toMatch(/^hold_\d+$/);
    expect(holds[2]).toEqual({
      id: holds[2].id,
      name: "",
      shortcut: "ctrl+e",
      append_text: "",
      post_process: false,
      send_context: false,
    });
    expect(callsOf("resumeAllBindings")).toEqual([[]]);
  });

  test("Escape adds nothing", async () => {
    await renderGeneral();

    await press(screen.getByRole("button", { name: "Add Hold Shortcut" }));
    await pressKeys("Escape");

    expect(callsOf("setProfileValue")).toEqual([]);
  });
});

// Question 4 a: a hold shortcut that post-processes is taken only while
// some dictation can post-process (`app_profiles::post_processing_reachable`),
// else its keys stay with the app. With post-processing reachable but off
// for the scope shown, the end shortcuts' hint applies instead.
describe("Hold Shortcuts that post-process", () => {
  test("while nothing can post-process, it says that it isn't taken", async () => {
    await renderGeneral();

    const post = visibleText(rowOf("⌃⇧ Space held"));
    expect(post).toContain(NOT_TAKEN);
    expect(post).not.toContain(POST_OFF);
    expect(visibleText(rowOf("⌃ Space held"))).not.toContain(NOT_TAKEN);
    expect(rawTextIn()).toEqual([]);
  });

  test("with post-processing on, it says nothing", async () => {
    await renderGeneral({ defaults: { post_process_enabled: true } });

    const text = visibleText(holdCard());
    expect(text).not.toContain(NOT_TAKEN);
    expect(text).not.toContain(POST_OFF);
  });

  test("when another profile post-processes, it is taken, and the defaults' own switch says so", async () => {
    await renderGeneral({
      profiles: [
        {
          id: "profile_1795300000001",
          name: "Work",
          apps: ["com.microsoft.VSCode"],
          own: ["post_process_enabled"],
          values: { post_process_enabled: true },
        },
      ],
    });

    const post = visibleText(rowOf("⌃⇧ Space held"));
    expect(post).not.toContain(NOT_TAKEN);
    expect(post).toContain(POST_OFF);
  });

  test("a hold shortcut without Post-process says neither", async () => {
    await renderGeneral({ defaults: { hold_keys: [DIRECT] } });

    expect(visibleText(holdCard())).not.toContain(NOT_TAKEN);
  });
});

describe("Hold Shortcuts in a profile", () => {
  const WORK = "profile_1795300000001";
  const OWN = holdKey("hold_1795307930035", "ctrl+alt+h", {
    name: "Polieren",
    post_process: true,
  });
  const spec: SettingsSpec = {
    defaults: { post_process_enabled: true },
    profiles: [
      {
        id: WORK,
        name: "Work",
        values: { hold_keys: [DIRECT, OWN] },
        ownEntries: { hold_keys: { own: [OWN.id], off: [POST.id] } },
      },
    ],
  };

  test("the profile's own and switched-off hold shortcuts are named", async () => {
    await renderGeneral(spec, { kind: "profile", id: WORK });

    const text = visibleText(holdCard());
    expect(text).toContain("Polieren own");
    expect(text).toContain("⌃⇧ Space held off");
    expect(screen.getByRole("button", { name: "Reset Polieren" })).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
  });

  test("under the defaults, the deviating profile lists them by name", async () => {
    await renderGeneral(spec);

    expect(visibleText(holdCard())).toContain(
      "Work own: Polieren · off: ⌃⇧ Space held",
    );
    expect(rawTextIn()).toEqual([]);
  });
});

describe("Hold Shortcuts in German", () => {
  test("names, hints and buttons read in German", async () => {
    await switchTestLanguage("de");
    await renderGeneral();

    expect(
      (
        within(holdCard("Haltetasten")).getAllByLabelText(
          "Name",
        ) as HTMLInputElement[]
      ).map((field) => field.value),
    ).toEqual(["⌃ Leertaste gehalten", "⌃⇧ Leertaste gehalten"]);
    const text = visibleText(holdCard("Haltetasten"));
    expect(text).toContain(
      "Gedrückt halten nimmt auf. Das Loslassen beendet die Aufnahme so",
    );
    expect(text).toContain(
      "Nur belegt, solange die Nachbearbeitung eingeschaltet ist: Bis dahin bleibt ihre Tastenkombination bei der App im Vordergrund.",
    );
    expect(
      screen.getByRole("button", { name: "Haltetaste hinzufügen" }),
    ).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
  });
});
