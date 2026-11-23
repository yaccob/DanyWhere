// DanyWhere: "When Held" under "Shortcut Behavior" (qa 2026-11-22_2202,
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
    test(`shows under Shortcut Behavior with ${activation}`, async () => {
      await renderGeneral({ defaults: { shortcut_activation: activation } });

      const texts = visibleTexts();
      const behavior = texts.indexOf("Shortcut Behavior");
      const held = texts.indexOf("When Held");
      const next = texts.indexOf("Paste Last Transcript");
      expect(behavior).toBeGreaterThan(-1);
      expect(held).toBeGreaterThan(behavior);
      expect(next).toBeGreaterThan(held);
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
