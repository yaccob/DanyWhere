// DanyWhere: "When Held" first under "End Shortcuts" (qa 2026-11-22_2202,
// "Noch nicht geprüft"): shown only where a start shortcut can be held, one
// line per start shortcut named by its combination, written as one value,
// and in a profile its state line in words.
import { describe, expect, test } from "bun:test";
import { screen, within } from "@testing-library/react";
import { GeneralSettings } from "@/components/settings/general/GeneralSettings";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { callsOf } from "@/test/bindings-mock";
import {
  binding,
  defaultValues,
  holdEnd,
  settingsWith,
  type SettingsSpec,
} from "@/test/fixtures";
import { press, renderPage } from "@/test/render";
import { rawTextIn, visibleText, visibleTexts } from "@/test/visible";

const renderGeneral = (spec: SettingsSpec, scope: Scope = DEFAULTS_SCOPE) =>
  renderPage(<GeneralSettings />, {
    settings: settingsWith(spec),
    scope,
    acceptWrites: true,
  });

/** The line of one start shortcut under "When Held". */
const heldLine = (title: string): HTMLElement => {
  const line = screen.getByText(title).parentElement;
  if (!line) throw new Error(`no line for ${title}`);
  return line;
};

describe("When Held", () => {
  test("doesn't show while the shortcuts toggle", async () => {
    await renderGeneral({ defaults: { shortcut_activation: "toggle" } });
    expect(visibleText()).not.toContain("When Held");
  });

  for (const activation of ["push_to_talk", "hold_or_toggle"] as const) {
    test(`shows first under End Shortcuts with ${activation}`, async () => {
      await renderGeneral({ defaults: { shortcut_activation: activation } });

      // Since 2026-11-24 (user): releasing a held start shortcut is an end,
      // so it opens the end shortcuts, right after the start shortcuts.
      const texts = visibleTexts();
      const start = texts.indexOf("Start Shortcut");
      const ends = texts.indexOf("End Shortcuts");
      const held = texts.indexOf("When Held");
      const list = texts.indexOf("Add End Shortcut");
      expect(start).toBeGreaterThan(-1);
      expect(ends).toBeGreaterThan(start);
      expect(held).toBeGreaterThan(ends);
      expect(list).toBeGreaterThan(held);
    });
  }

  // The second start shortcut's line shows only while DanyWhere takes that
  // shortcut, i.e. while some dictation can post-process (`secondStartTaken`).
  const withSecondStart = {
    shortcut_activation: "push_to_talk",
    post_process_enabled: true,
  } as const;

  test("names each start shortcut's line by its combination", async () => {
    await renderGeneral({ defaults: withSecondStart });

    expect(screen.getByText("⌃ Space held")).toBeTruthy();
    expect(screen.getByText("⌃⇧ Space held")).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
  });

  test("the second start shortcut's line shows only while DanyWhere takes it", async () => {
    await renderGeneral({ defaults: { shortcut_activation: "push_to_talk" } });

    expect(screen.getByText("⌃ Space held")).toBeTruthy();
    expect(screen.queryByText("⌃⇧ Space held")).toBeNull();
  });

  test("a start shortcut without a combination is named instead", async () => {
    const bindings = {
      ...(defaultValues().bindings as Record<string, unknown>),
      transcribe_with_post_process: binding("transcribe_with_post_process", ""),
    };
    await renderGeneral({ defaults: { ...withSecondStart, bindings } });

    expect(screen.getByText("Second Start Shortcut held")).toBeTruthy();
  });

  test("a change stores the ends of both start shortcuts as one value", async () => {
    await renderGeneral({ defaults: { shortcut_activation: "push_to_talk" } });

    const line = heldLine("⌃ Space held");
    await press(within(line).getByRole("checkbox", { name: "Post-process" }));

    expect(callsOf("setProfileValue")).toEqual([
      [
        DEFAULTS_SCOPE,
        "hold_ends",
        {
          transcribe: holdEnd({ post_process: true }),
          transcribe_with_post_process: holdEnd({ post_process: true }),
        },
      ],
    ]);
  });

  test("Send Context can be checked only together with Post-process", async () => {
    await renderGeneral({ defaults: withSecondStart });

    const context = (title: string) =>
      within(heldLine(title)).getByRole("checkbox", {
        name: "Send Context",
      }) as HTMLInputElement;
    expect(context("⌃ Space held").disabled).toBe(true);
    expect(context("⌃⇧ Space held").disabled).toBe(false);
  });
});

// With post-processing off no hold end post-processes either (user,
// 2026-11-23, option a): a line whose "Post-process" is checked says so, for
// the scope shown; its box stays usable.
describe("When Held with post-processing off", () => {
  const HINT =
    "Post-processing is off: a dictation ended this way isn't post-processed. Turn on “Post Processing” under Advanced.";
  const hintIn = (title: string) =>
    within(heldLine(title)).queryByText(HINT) !== null;
  const posting = {
    shortcut_activation: "push_to_talk",
    hold_ends: {
      transcribe: holdEnd({ post_process: true }),
      transcribe_with_post_process: holdEnd({ post_process: true }),
    },
  } as const;

  test("a held end with Post-process checked says that it doesn't post-process", async () => {
    await renderGeneral({ defaults: posting });

    // Only the first start shortcut's line shows without post-processing.
    // (The page's End Shortcuts say it too, for the built-in end with
    // post-processing.)
    expect(hintIn("⌃ Space held")).toBe(true);
    expect(screen.queryByText("⌃⇧ Space held")).toBeNull();
    const box = within(heldLine("⌃ Space held")).getByRole("checkbox", {
      name: "Post-process",
    }) as HTMLInputElement;
    expect([box.checked, box.disabled]).toEqual([true, false]);
    expect(rawTextIn()).toEqual([]);
  });

  test("a held end without Post-process says nothing", async () => {
    await renderGeneral({ defaults: { shortcut_activation: "push_to_talk" } });

    expect(hintIn("⌃ Space held")).toBe(false);
  });

  test("with post-processing on, no line says so", async () => {
    await renderGeneral({
      defaults: { ...posting, post_process_enabled: true },
    });

    expect(screen.getByText("⌃⇧ Space held")).toBeTruthy();
    expect(visibleText()).not.toContain(HINT);
  });

  test("in a profile its own switch counts, not the defaults'", async () => {
    const WORK = "profile_1795300000001";
    const work = (postProcess: boolean): SettingsSpec => ({
      defaults: { ...posting, post_process_enabled: !postProcess },
      profiles: [
        {
          id: WORK,
          name: "Work",
          own: ["post_process_enabled"],
          values: { post_process_enabled: postProcess },
        },
      ],
    });

    await renderGeneral(work(false), { kind: "profile", id: WORK });
    expect(hintIn("⌃ Space held")).toBe(true);
    expect(rawTextIn()).toEqual([]);
  });

  test("in a profile with post-processing on, no line says so", async () => {
    const WORK = "profile_1795300000001";
    await renderGeneral(
      {
        defaults: { ...posting, post_process_enabled: false },
        profiles: [
          {
            id: WORK,
            name: "Work",
            own: ["post_process_enabled"],
            values: { post_process_enabled: true },
          },
        ],
      },
      { kind: "profile", id: WORK },
    );
    expect(visibleText()).not.toContain(HINT);
  });
});

describe("When Held in a profile", () => {
  const WORK = "profile_1795300000001";
  const spec: SettingsSpec = {
    defaults: { shortcut_activation: "push_to_talk" },
    profiles: [
      {
        id: WORK,
        name: "Work",
        own: ["hold_ends"],
        values: {
          hold_ends: {
            transcribe: holdEnd({ post_process: true, send_context: true }),
            transcribe_with_post_process: holdEnd({ post_process: true }),
          },
        },
      },
    ],
  };

  test("its state line names the defaults' ends in words", async () => {
    await renderGeneral(spec, { kind: "profile", id: WORK });

    expect(visibleText()).toContain(
      "Own value · Default: ⌃ Space held: nothing · ⌃⇧ Space held: Post-process",
    );
    expect(rawTextIn()).toEqual([]);
  });

  test("under the defaults, the profile's own ends read in words", async () => {
    await renderGeneral(spec);

    expect(visibleText()).toContain(
      "Work ⌃ Space held: Post-process, Send Context · ⌃⇧ Space held: Post-process",
    );
    expect(rawTextIn()).toEqual([]);
  });
});
