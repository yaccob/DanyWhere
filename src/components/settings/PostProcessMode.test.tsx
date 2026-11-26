// DanyWhere: the switch "Post Processing" became a choice per profile
// (sidecar qa 2026-11-26_1841, decision: variant b; 2026-11-26_1810,
// "Schnittstelle Phase 2"): Never (nothing goes to a provider, instruction
// shortcuts aren't taken), With Instruction Shortcut (out of the box: only
// a dictation in which one was pressed) and Always (every dictation; an
// instruction shortcut adds an instruction). It sits where the switch was,
// on Advanced › Transcription, and is a profile value like any other.
import { describe, expect, test } from "bun:test";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { AdvancedSettings } from "./advanced/AdvancedSettings";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { callsOf } from "@/test/bindings-mock";
import { settingsWith, type SettingsSpec } from "@/test/fixtures";
import { switchTestLanguage } from "@/test/i18n";
import { press, renderPage, settle } from "@/test/render";
import { rawTextIn, readableTexts, visibleText } from "@/test/visible";

const WORK = "profile_1795300000001";

const renderAdvanced = (
  spec: SettingsSpec = {},
  scope: Scope = DEFAULTS_SCOPE,
) =>
  renderPage(<AdvancedSettings />, {
    settings: settingsWith(spec),
    scope,
    acceptWrites: true,
  });

/** The writes of `post_process_mode`, as (scope, value). */
const modeWrites = () =>
  callsOf("setProfileValue")
    .filter(([, field]) => field === "post_process_mode")
    .map(([scope, , value]) => [scope, value]);

/** Opens the tooltip of the setting called `title`, as a press on its
 * info button does. */
const openInfo = async (title: string) => {
  const heading = screen.getByRole("heading", { level: 3, name: title });
  const info = within(heading.parentElement!).getByRole("button");
  await act(async () => {
    fireEvent.pointerDown(info);
  });
  await settle();
};

const choose = async (shown: string, option: string) => {
  await press(screen.getByRole("button", { name: shown }));
  const options = screen.getAllByRole("button", { name: option });
  await press(options[options.length - 1]);
};

describe("Post Processing: a choice instead of a switch", () => {
  test("out of the box: With Instruction Shortcut, and no switch any more", async () => {
    await renderAdvanced();

    expect(visibleText()).toContain("Post Processing");
    expect(
      screen.getByRole("button", { name: "With Instruction Shortcut" }),
    ).toBeTruthy();
    expect(
      screen.queryAllByRole("checkbox", { name: "Post Processing" }),
    ).toEqual([]);
    expect(readableTexts().join(" ")).not.toContain(
      "Enable AI-powered text refinement",
    );
    expect(rawTextIn()).toEqual([]);
  });

  test("its three choices, in order", async () => {
    await renderAdvanced();

    await press(
      screen.getByRole("button", { name: "With Instruction Shortcut" }),
    );
    const texts = visibleText();
    const at = (label: string) => texts.lastIndexOf(label);
    expect(at("Never")).toBeGreaterThan(-1);
    expect(at("Always")).toBeGreaterThan(-1);
    expect(at("Never")).toBeLessThan(at("Always"));
    expect(rawTextIn()).toEqual([]);
  });

  test("its description names what each choice does", async () => {
    await renderAdvanced();
    await openInfo("Post Processing");

    const readable = readableTexts().join(" ");
    expect(readable).toContain(
      "Never: nothing goes to a provider, and instruction shortcuts aren't taken.",
    );
    expect(readable).toContain(
      "With Instruction Shortcut: only a dictation in which an instruction shortcut was pressed.",
    );
    expect(readable).toContain(
      "Always: every dictation; an instruction shortcut adds an instruction.",
    );
  });

  test("choosing Never writes it into the defaults", async () => {
    await renderAdvanced();

    await choose("With Instruction Shortcut", "Never");

    expect(modeWrites()).toEqual([[DEFAULTS_SCOPE, "never"]]);
    expect(screen.getByRole("button", { name: "Never" })).toBeTruthy();
  });

  test("choosing Always in a profile writes it into the profile", async () => {
    const scope: Scope = { kind: "profile", id: WORK };
    await renderAdvanced({ profiles: [{ id: WORK, name: "Work" }] }, scope);

    await choose("With Instruction Shortcut", "Always");

    expect(modeWrites()).toEqual([[scope, "always"]]);
  });

  test("a profile's own mode shows, with the default's value under it", async () => {
    await renderAdvanced(
      {
        profiles: [
          {
            id: WORK,
            name: "Work",
            own: ["post_process_mode"],
            values: { post_process_mode: "never" },
          },
        ],
      },
      { kind: "profile", id: WORK },
    );

    expect(screen.getByRole("button", { name: "Never" })).toBeTruthy();
    expect(visibleText()).toContain("With Instruction Shortcut");
    expect(rawTextIn()).toEqual([]);
  });
});

// The time limit only counts while something can be post-processed.
describe("Post-Processing Time Limit", () => {
  test("hidden while post-processing is Never", async () => {
    await renderAdvanced({ defaults: { post_process_mode: "never" } });
    expect(visibleText()).not.toContain("Post-Processing Time Limit");
  });

  test("shown with an instruction shortcut and always", async () => {
    await renderAdvanced({ defaults: { post_process_mode: "always" } });
    expect(visibleText()).toContain("Post-Processing Time Limit");
  });
});

describe("Post Processing in German", () => {
  test("Nachbearbeitung with Nie, Mit Anweisungstaste and Immer", async () => {
    await switchTestLanguage("de");
    await renderAdvanced();

    expect(visibleText()).toContain("Nachbearbeitung");
    await press(screen.getByRole("button", { name: "Mit Anweisungstaste" }));
    const texts = visibleText();
    expect(texts).toContain("Nie");
    expect(texts).toContain("Immer");
    await openInfo("Nachbearbeitung");
    expect(readableTexts().join(" ")).toContain(
      "Nie: Nichts geht an einen Anbieter, Anweisungstasten sind nicht belegt.",
    );
    expect(rawTextIn()).toEqual([]);
  });
});
