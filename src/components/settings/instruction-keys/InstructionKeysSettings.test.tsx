// DanyWhere: the card Instruction Shortcuts on General (sidecar qa
// 2026-11-26_1841, decision: a free list like the start, hold and end
// shortcuts; 2026-11-26_1810, "Schnittstelle Phase 2"; 2026-11-26_1720,
// "Einstellungen auf ‚Allgemein‘"). During a recording an instruction
// shortcut switches from the text to a spoken instruction for
// post-processing. The card sits between Hold Shortcuts and End Shortcuts,
// uses the end shortcuts' editor with names that follow the keys ("⇥ held"),
// may be emptied, says that its shortcuts are taken only during a recording,
// and when they aren't taken at all (post-processing "never", no provider).
// Linux has them only on a start or hold shortcut's combination (below).
import { describe, expect, test } from "bun:test";
import { cleanup, fireEvent, screen, within } from "@testing-library/react";
import type { EndKey } from "@/bindings";
import { GeneralSettings } from "@/components/settings/general/GeneralSettings";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { callsOf, err, mockCommand } from "@/test/bindings-mock";
import {
  endKey,
  instructionKey,
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
import { rawTextIn, readableTexts, visibleText } from "@/test/visible";
import { answerStatus, notReady } from "@/test/postProcessSetupFixtures";

const TAB = instructionKey("instruction_tab", "tab");
const WORK = "profile_1795300000001";

/** A provider with a model and its key: post-processing can run. */
const READY = {
  post_process_provider_id: "openai",
  post_process_providers: [
    { id: "openai", label: "OpenAI", base_url: "https://api.openai.com/v1" },
    {
      id: "chatgpt",
      label: "ChatGPT (subscription)",
      base_url: "https://api.openai.com/v1",
    },
  ],
  post_process_models: { openai: "gpt-5-mini" },
  post_process_api_keys: { openai: "sk-test" },
};
/** The same provider and model without the key: not ready. */
const WITHOUT_KEY = { ...READY, post_process_api_keys: {} };

const TITLE = "Instruction Shortcuts";
const DESCRIPTION =
  "During a recording, an instruction shortcut switches from the text to an instruction: what you say next isn't inserted but tells post-processing what to do with the text";
const ONLY_WHILE = "Taken only during a recording.";
const NEVER =
  "Post-processing is set to “Never”: instruction shortcuts aren't taken. Choose another setting for Post Processing under Advanced.";
const NOT_SET_UP =
  "Nothing is set up for post-processing yet: after an instruction shortcut the text is inserted as heard, without the instruction, and a notice offers to set it up.";
const SAME_AS_START_OR_HOLD =
  "Same combination as a start or hold shortcut: without a recording it starts one, during a recording it switches to the instruction.";

const renderGeneral = (
  spec: SettingsSpec = {},
  scope: Scope = DEFAULTS_SCOPE,
  acceptWrites = true,
) =>
  renderPage(<GeneralSettings />, {
    settings: settingsWith(spec),
    scope,
    acceptWrites,
  });

/** The headings of the page's cards, top to bottom. */
const cardTitles = (): string[] =>
  Array.from(document.querySelectorAll("h2")).map(
    (heading) => heading.textContent?.trim() ?? "",
  );

const card = (title = TITLE): HTMLElement => {
  const heading = screen.getByRole("heading", { level: 2, name: title });
  const group = heading.closest(".space-y-2");
  if (!(group instanceof HTMLElement)) throw new Error(`no card ${title}`);
  return group;
};

const nameFields = (title = TITLE) =>
  within(card(title)).queryAllByLabelText("Name") as HTMLInputElement[];

/** The instruction shortcuts the page last asked the backend to store. */
const storedInstructions = (): EndKey[] => {
  const calls = callsOf("setProfileValue").filter(
    ([, field]) => field === "instruction_keys",
  );
  expect(calls.length).toBeGreaterThan(0);
  const [scope, , value] = calls[calls.length - 1];
  expect(scope).toEqual(DEFAULTS_SCOPE);
  return value as unknown as EndKey[];
};

describe("Instruction Shortcuts: where and what", () => {
  test("the card sits between Hold Shortcuts and End Shortcuts", async () => {
    await renderGeneral();

    expect(cardTitles().slice(0, 4)).toEqual([
      "Start Shortcuts",
      "Hold Shortcuts",
      TITLE,
      "End Shortcuts",
    ]);
    expect(visibleText(card())).toContain(DESCRIPTION);
    expect(rawTextIn()).toEqual([]);
  });

  test("out of the box: ⇥, named “⇥ held”, taken only during a recording", async () => {
    await renderGeneral({ defaults: READY });

    expect(nameFields().map((field) => field.value)).toEqual(["⇥ held"]);
    expect(within(card()).getByRole("button", { name: "⇥" })).toBeTruthy();
    expect(visibleText(card())).toContain(ONLY_WHILE);
    expect(rawTextIn()).toEqual([]);
  });

  test("each entry has Append and Send Context, but no Post-process", async () => {
    await renderGeneral({ defaults: READY });

    expect(within(card()).getAllByRole("checkbox")).toHaveLength(1);
    expect(
      within(card()).queryAllByRole("checkbox", { name: "Post-process" }),
    ).toEqual([]);
    const context = within(card()).getByRole("checkbox", {
      name: "Send Context",
    }) as HTMLInputElement;
    expect(context.disabled).toBe(false);
    expect(visibleText(card())).toContain("Append");

    await press(context);
    expect(storedInstructions()).toEqual([{ ...TAB, send_context: true }]);
  });

  test("a typed name is stored when the field loses focus", async () => {
    await renderGeneral({ defaults: READY });

    const field = nameFields()[0];
    fireEvent.change(field, { target: { value: "Übersetzen" } });
    fireEvent.blur(field);
    await settle();

    expect(storedInstructions()).toEqual([{ ...TAB, name: "Übersetzen" }]);
  });

  test("an instruction shortcut with a start or hold shortcut's keys says so", async () => {
    await renderGeneral({
      defaults: {
        ...READY,
        instruction_keys: [
          TAB,
          instructionKey("instruction_space", "ctrl+space"),
        ],
      },
    });

    const text = visibleText(card());
    expect(text.split(SAME_AS_START_OR_HOLD).length - 1).toBe(1);
    expect(text.split(ONLY_WHILE).length - 1).toBe(2);
    // And the start and hold shortcuts with those keys say it too.
    expect(visibleText(card("Start Shortcuts"))).toContain(
      "Also an instruction shortcut",
    );
    expect(visibleText(card("Hold Shortcuts"))).toContain(
      "Also an instruction shortcut: during a recording it switches to the instruction.",
    );
  });

  test("without shared keys the start and hold shortcuts say nothing of it", async () => {
    await renderGeneral({ defaults: READY });

    expect(readableTexts(card("Start Shortcuts")).join(" ")).not.toContain(
      "instruction",
    );
    expect(readableTexts(card("Hold Shortcuts")).join(" ")).not.toContain(
      "instruction shortcut",
    );
  });
});

describe("Instruction Shortcuts: adding and deleting", () => {
  test("Add Instruction Shortcut records a combination first, then adds it", async () => {
    await renderGeneral({ defaults: READY });

    await press(
      screen.getByRole("button", { name: "Add Instruction Shortcut" }),
    );
    expect(
      screen.getByText(
        "Press the combination for the new instruction shortcut",
      ),
    ).toBeTruthy();
    expect(callsOf("setProfileValue")).toEqual([]);

    await pressKeys("ControlLeft", "KeyE");

    const stored = storedInstructions();
    expect(stored).toHaveLength(2);
    expect(stored[0]).toEqual(TAB);
    expect(stored[1].id).toMatch(/^instruction_\d+$/);
    expect(stored[1]).toEqual({
      id: stored[1].id,
      name: "",
      shortcut: "ctrl+e",
      append_text: "",
      send_context: false,
    });
    expect(callsOf("resumeAllBindings")).toEqual([[]]);
  });

  test("the list may become empty: the last one can be deleted, and added again", async () => {
    await renderGeneral({ defaults: READY });

    const remove = screen.getByRole("button", { name: "Delete ⇥ held" });
    expect((remove as HTMLButtonElement).disabled).toBe(false);
    await press(remove);

    expect(storedInstructions()).toEqual([]);
    expect(nameFields()).toEqual([]);
    expect(
      screen.getByRole("button", { name: "Add Instruction Shortcut" }),
    ).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
  });

  test("an empty list shows only the button to add one", async () => {
    await renderGeneral({ defaults: { ...READY, instruction_keys: [] } });

    expect(nameFields()).toEqual([]);
    expect(visibleText(card())).not.toContain(ONLY_WHILE);
    expect(
      screen.getByRole("button", { name: "Add Instruction Shortcut" }),
    ).toBeTruthy();
  });
});

// Instruction shortcuts are taken while the scope may post-process, set up
// or not (qa 2026-11-26_2250: the setup's notice offers to set it up); the
// card says why they aren't, or what happens without a provider, for the
// scope shown.
describe("Instruction Shortcuts that aren't taken", () => {
  test("with post-processing set to Never, the card says so", async () => {
    await renderGeneral({
      defaults: { ...READY, post_process_mode: "never" },
    });

    const text = visibleText(card());
    expect(text).toContain(NEVER);
    expect(text).not.toContain(NOT_SET_UP);
    expect(rawTextIn()).toEqual([]);
  });

  test("without a provider with a model they are taken, and the card says what happens", async () => {
    await renderGeneral();

    const text = visibleText(card());
    expect(text).toContain(NOT_SET_UP);
    expect(text).not.toContain(NEVER);
    expect(text).not.toContain("aren't taken");
    expect(rawTextIn()).toEqual([]);
  });

  test("with a provider, with an instruction shortcut or always, nothing of it", async () => {
    for (const mode of ["with_instruction_key", "always"] as const) {
      await renderGeneral({ defaults: { ...READY, post_process_mode: mode } });

      const text = visibleText(card());
      expect(text).not.toContain(NEVER);
      expect(text).not.toContain(NOT_SET_UP);
      cleanup();
    }
  });

  // Ready is what the backend checks before it sends (`post_process_ready`):
  // a model alone isn't, e.g. after Disconnect (sidecar qa
  // 2026-11-26_2250, D, N10).
  test("a provider with a model but no key: the card says what happens", async () => {
    answerStatus(notReady({ readiness: "key", model: "gpt-5-mini" }));
    await renderGeneral({ defaults: WITHOUT_KEY });
    expect(visibleText(card())).toContain(NOT_SET_UP);
  });

  test("ChatGPT after Disconnect: the card says what happens", async () => {
    answerStatus(
      notReady({
        readiness: "sign_in",
        provider_id: "chatgpt",
        model: "gpt-6.2-sol",
      }),
    );
    await renderGeneral({
      defaults: {
        ...READY,
        post_process_provider_id: "chatgpt",
        post_process_models: { chatgpt: "gpt-6.2-sol" },
      },
      app: { chatgpt_signed_in: false },
    });
    expect(visibleText(card())).toContain(NOT_SET_UP);
  });

  test("in a profile, its provider without a key counts too", async () => {
    await renderGeneral(
      {
        defaults: WITHOUT_KEY,
        profiles: [{ id: WORK, name: "Work" }],
      },
      { kind: "profile", id: WORK },
    );
    expect(visibleText(card())).toContain(NOT_SET_UP);
  });

  test("in a profile its own mode counts, not the defaults'", async () => {
    const spec: SettingsSpec = {
      defaults: READY,
      profiles: [
        {
          id: WORK,
          name: "Work",
          own: ["post_process_mode"],
          values: { post_process_mode: "never" },
        },
      ],
    };

    await renderGeneral(spec, { kind: "profile", id: WORK });
    expect(visibleText(card())).toContain(NEVER);
    expect(rawTextIn()).toEqual([]);
  });
});

// Linux (user, 2026-11-28; sidecar qa 2026-11-28_1405): nothing is taken
// there only during a recording, so an instruction shortcut works only with
// a start or hold shortcut's combination, out of the box Ctrl+Alt+Shift+Space.
// The card shows and says so; an entry without such a combination says that
// Linux doesn't take it, and the backend refuses a new one.
describe("Instruction Shortcuts on Linux", () => {
  const LINUX_NOTE =
    "On Linux, DanyWhere can't take a combination only during a recording, so an instruction shortcut needs the combination of a start or hold shortcut: pressed during a recording, that one switches to the instruction.";
  const NOT_TAKEN =
    "Not taken on Linux: no start or hold shortcut has this combination.";
  const SECOND = instructionKey("instruction_second", "ctrl+alt+shift+space");
  const linux: SettingsSpec = {
    defaults: {
      ...READY,
      start_keys: [startKey("start", "ctrl+alt+space")],
      hold_keys: [
        endKey("hold_space", "ctrl+alt+space"),
        endKey("hold_no_space", "ctrl+alt+shift+space"),
      ],
      instruction_keys: [SECOND],
      end_keys: [endKey("plain", "ctrl+alt+space")],
    },
  };

  test("the card shows between Hold and End Shortcuts and says what Linux takes", async () => {
    setOsType("linux");
    await renderGeneral(linux);

    expect(cardTitles().slice(0, 4)).toEqual([
      "Start Shortcuts",
      "Hold Shortcuts",
      TITLE,
      "End Shortcuts",
    ]);
    const text = visibleText(card());
    expect(text).toContain(LINUX_NOTE);
    expect(text).toContain(SAME_AS_START_OR_HOLD);
    expect(text).not.toContain(NOT_TAKEN);
    expect(rawTextIn()).toEqual([]);
  });

  test("an entry no start or hold shortcut has says that Linux doesn't take it", async () => {
    setOsType("linux");
    await renderGeneral({
      defaults: { ...linux.defaults, instruction_keys: [TAB] },
    });

    expect(visibleText(card())).toContain(NOT_TAKEN);
    expect(rawTextIn()).toEqual([]);
  });

  test("elsewhere neither the Linux note nor its entry hint shows", async () => {
    await renderGeneral({ defaults: READY });

    const text = visibleText(card());
    expect(text).not.toContain(LINUX_NOTE);
    expect(text).not.toContain(NOT_TAKEN);
  });

  test("a combination of neither list is refused with its reason", async () => {
    setOsType("linux");
    mockCommand("setProfileValue", async () =>
      err("instruction-key-needs-start-or-hold:instruction_tab"),
    );
    await renderGeneral(
      { defaults: { ...linux.defaults, instruction_keys: [SECOND, TAB] } },
      DEFAULTS_SCOPE,
      false,
    );
    await press(screen.getByRole("button", { name: "Delete Tab held" }));

    expect(raisedNotices()).toEqual([
      "On Linux an instruction shortcut needs the combination of a start or hold shortcut.",
    ]);
  });
});

describe("Instruction Shortcuts in a profile", () => {
  const OWN = instructionKey("instruction_1795307930036", "ctrl+alt+i", {
    name: "Übersetzen",
  });
  const spec: SettingsSpec = {
    defaults: READY,
    profiles: [
      {
        id: WORK,
        name: "Work",
        values: { instruction_keys: [OWN] },
        ownEntries: { instruction_keys: { own: [OWN.id], off: [TAB.id] } },
      },
    ],
  };

  test("the profile's own and switched-off instruction shortcuts are named", async () => {
    await renderGeneral(spec, { kind: "profile", id: WORK });

    const text = visibleText(card());
    expect(text).toContain("Übersetzen own");
    expect(text).toContain("⇥ held off");
    expect(rawTextIn()).toEqual([]);
  });

  test("under the defaults, the deviating profile lists them by name", async () => {
    await renderGeneral(spec);

    expect(visibleText(card())).toContain("Work own: Übersetzen · off: ⇥ held");
    expect(rawTextIn()).toEqual([]);
  });
});

describe("refusals of the backend for instruction shortcuts", () => {
  const refuse = async (error: string) => {
    mockCommand("setProfileValue", async () => err(error));
    await renderGeneral({ defaults: READY }, DEFAULTS_SCOPE, false);
    await press(screen.getByRole("button", { name: "Delete ⇥ held" }));
    return raisedNotices();
  };

  test("an entry without a combination", async () => {
    expect(
      await refuse("instruction-key-needs-shortcut:instruction_tab"),
    ).toEqual(["Every instruction shortcut needs a key combination."]);
  });

  test("two entries with one combination", async () => {
    expect(await refuse("instruction-key-shortcut-twice:tab")).toEqual([
      "Two instruction shortcuts can't share a combination.",
    ]);
  });

  test("a combination an instruction shortcut has names it", async () => {
    expect(await refuse("conflict:instruction_key:instruction_tab")).toEqual([
      "Instruction shortcut ⇥ held already uses this shortcut",
    ]);
  });

  test("an end shortcut's combination names the end shortcut", async () => {
    expect(await refuse("conflict:end_key:plain")).toEqual([
      "End shortcut ⌃ Space already uses this shortcut",
    ]);
  });

  test("any other reason says that the instruction shortcuts weren't saved", async () => {
    expect(await refuse("disk full")).toEqual([
      "Couldn't save the instruction shortcuts: disk full",
    ]);
  });
});

describe("Instruction Shortcuts in German", () => {
  test("title, names, hints and buttons read in German", async () => {
    await switchTestLanguage("de");
    await renderGeneral({
      defaults: {
        ...READY,
        instruction_keys: [
          TAB,
          instructionKey("instruction_space", "ctrl+space"),
        ],
      },
    });

    expect(cardTitles().slice(0, 4)).toEqual([
      "Starttasten",
      "Haltetasten",
      "Anweisungstasten",
      "Endtasten",
    ]);
    expect(nameFields("Anweisungstasten").map((field) => field.value)).toEqual([
      "⇥ gehalten",
      "⌃ Leertaste gehalten",
    ]);
    const text = visibleText(card("Anweisungstasten"));
    expect(text).toContain(
      "Während einer Aufnahme schaltet eine Anweisungstaste vom Text zur Anweisung",
    );
    expect(text).toContain("Nur während einer Aufnahme belegt.");
    expect(text).toContain(
      "Dieselbe Tastenkombination wie eine Start- oder Haltetaste: Ohne Aufnahme startet sie eine, während einer Aufnahme schaltet sie zur Anweisung.",
    );
    expect(
      screen.getByRole("button", { name: "Anweisungstaste hinzufügen" }),
    ).toBeTruthy();
    expect(visibleText(card("Starttasten"))).toContain("Auch Anweisungstaste");
    expect(rawTextIn()).toEqual([]);
  });

  test("the hints why they aren't taken read in German", async () => {
    await switchTestLanguage("de");
    await renderGeneral({ defaults: { post_process_mode: "never" } });

    expect(visibleText(card("Anweisungstasten"))).toContain(
      "Die Nachbearbeitung steht auf „Nie“: Anweisungstasten sind nicht belegt. Wähle unter Erweitert bei „Nachbearbeitung“ eine andere Einstellung.",
    );
    expect(rawTextIn()).toEqual([]);

    cleanup();
    await renderGeneral();
    expect(visibleText(card("Anweisungstasten"))).toContain(
      "Die Nachbearbeitung ist noch nicht eingerichtet: Nach einer Anweisungstaste wird der Text so eingefügt, wie er gehört wurde, ohne die Anweisung, und ein Hinweis bietet an, sie einzurichten.",
    );
  });

  test("refusals read in German", async () => {
    await switchTestLanguage("de");
    mockCommand("setProfileValue", async () =>
      err("conflict:instruction_key:instruction_tab"),
    );
    await renderGeneral({ defaults: READY }, DEFAULTS_SCOPE, false);
    await press(screen.getByRole("button", { name: "⇥ gehalten löschen" }));

    expect(raisedNotices()).toEqual([
      "„Anweisungstaste ⇥ gehalten“ verwendet dieses Kürzel schon",
    ]);
  });
});
