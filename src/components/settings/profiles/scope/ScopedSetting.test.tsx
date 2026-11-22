// DanyWhere: the state line under a setting in a profile and the profiles
// under a default (qa 2026-11-22_0107 B2, B3, B5, B7, B9 and errors 1 to
// 3): coupled, own, own and equal, each with the default's value in words;
// a list's own entries by name; nothing under a setting this device doesn't
// show; under the defaults the deviating profiles by name with their own
// control, the others in one line, "+ Profile" offering them by name.
import { describe, expect, test } from "bun:test";
import { screen, within } from "@testing-library/react";
import { DEFAULTS_SCOPE, type Scope } from "@/lib/profileScope";
import { ChannelSelector } from "@/components/settings/ChannelSelector";
import { DoneSound } from "@/components/settings/DoneSound";
import { ShortcutInput } from "@/components/settings/ShortcutInput";
import { TypingToolSetting } from "@/components/settings/TypingTool";
import { ScopedSetting } from "./ScopedSetting";
import { callsOf, mockCommand, ok } from "@/test/bindings-mock";
import {
  binding,
  defaultValues,
  settingsWith,
  wordList,
  type ProfileSpec,
} from "@/test/fixtures";
import { press, renderPage } from "@/test/render";
import { rawTextIn, visibleText } from "@/test/visible";

const WORK = "profile_1795300000001";
const PRIVATE = "profile_1795300000002";
const NEUTEST = "profile_1795300000003";
const IN_WORK: Scope = { kind: "profile", id: WORK };

const CHAIN_HOLD =
  "Follows the defaults. Click to keep this value even when the default changes.";
const CHAIN_COUPLE = "Own value. Click to follow the defaults again.";

const bindingsWith = (keys: string) => ({
  ...(defaultValues().bindings as Record<string, unknown>),
  transcribe: binding("transcribe", "ctrl+space", { current_binding: keys }),
});

const render = (
  ui: React.ReactElement,
  profiles: ProfileSpec[],
  scope: Scope,
  defaults: Record<string, unknown> = {},
) => renderPage(ui, { settings: settingsWith({ defaults, profiles }), scope });

describe("the state line in a profile", () => {
  test("a coupled value says Default, and its chain keeps it as the profile's own", async () => {
    await render(
      <ScopedSetting field="done_sound">
        <DoneSound />
      </ScopedSetting>,
      [{ id: WORK, name: "Work" }],
      IN_WORK,
    );

    expect(visibleText()).toContain("Sound When Done Default");
    await press(screen.getByRole("button", { name: CHAIN_HOLD }));
    expect(callsOf("holdProfileValue")).toEqual([[WORK, "done_sound"]]);
  });

  test("an own shortcut names the default's combination, not a count", async () => {
    await render(
      <ScopedSetting field="bindings" entry="transcribe">
        <ShortcutInput shortcutId="transcribe" />
      </ScopedSetting>,
      [
        {
          id: WORK,
          name: "Work",
          values: { bindings: bindingsWith("ctrl+alt+e") },
          ownEntries: { bindings: { own: ["transcribe"] } },
        },
      ],
      IN_WORK,
    );

    expect(visibleText()).toContain("Own value · Default: ⌃ Space Reset");
    expect(rawTextIn()).toEqual([]);

    await press(screen.getByRole("button", { name: CHAIN_COUPLE }));
    await press(screen.getByRole("button", { name: "Reset" }));
    expect(callsOf("resetProfileEntry")).toEqual([
      [WORK, "bindings", "transcribe"],
      [WORK, "bindings", "transcribe"],
    ]);
  });

  test("an own value equal to the default says so", async () => {
    await render(
      <ScopedSetting field="done_sound">
        <DoneSound />
      </ScopedSetting>,
      [{ id: WORK, name: "Work", own: ["done_sound"] }],
      IN_WORK,
    );

    expect(visibleText()).toContain("Own value · same as the default");
  });

  test("a list shows its own and switched-off entries by name", async () => {
    const gListe = wordList("word_list_1795306966443", "GListe");
    const pListe = wordList("word_list_1795307006713", "PListe");
    await render(
      <ScopedSetting field="word_lists">
        <div data-testid="word-lists" />
      </ScopedSetting>,
      [
        {
          id: WORK,
          name: "Work",
          values: { word_lists: [pListe] },
          ownEntries: { word_lists: { own: [pListe.id], off: [gListe.id] } },
        },
      ],
      IN_WORK,
      { word_lists: [gListe] },
    );

    const text = visibleText();
    expect(text).toContain("PListe own");
    expect(text).toContain("GListe off");
    expect(screen.getByRole("button", { name: "Reset GListe" })).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
  });
});

describe("a setting this device doesn't show", () => {
  const profiles: ProfileSpec[] = [
    { id: WORK, name: "Work", own: ["selected_channel", "typing_tool"] },
    { id: PRIVATE, name: "Private" },
  ];
  const page = (
    <>
      <ScopedSetting field="selected_channel">
        <ChannelSelector />
      </ScopedSetting>
      <ScopedSetting field="typing_tool">
        <TypingToolSetting />
      </ScopedSetting>
    </>
  );

  test("gets no state line in a profile", async () => {
    await render(page, profiles, IN_WORK);

    expect(visibleText()).toBe("");
    expect(screen.queryByRole("button", { name: CHAIN_COUPLE })).toBeNull();
  });

  test("gets no profiles under the default", async () => {
    await render(page, profiles, DEFAULTS_SCOPE);

    expect(visibleText()).toBe("");
  });

  test("gets its line once it shows: a microphone with two channels", async () => {
    mockCommand("getMicrophoneChannels", async () => ok(2));
    await render(page, profiles, IN_WORK);

    const text = visibleText();
    expect(text).toContain("Own value");
    // The typing tool still draws nothing on macOS.
    expect(text.match(/Own value/g)?.length).toBe(1);
  });
});

describe("the profiles under a default", () => {
  const profiles: ProfileSpec[] = [
    {
      id: WORK,
      name: "Work",
      own: ["done_sound"],
      values: { done_sound: false },
    },
    { id: PRIVATE, name: "Private" },
    { id: NEUTEST, name: "Neutest" },
  ];
  const page = (
    <ScopedSetting field="done_sound">
      <DoneSound />
    </ScopedSetting>
  );

  test("a deviating profile shows by name with its own control and its reset", async () => {
    await render(page, profiles, DEFAULTS_SCOPE);

    const switches = screen.getAllByRole("checkbox") as HTMLInputElement[];
    expect(switches.map((box) => box.checked)).toEqual([true, false]);
    const reset = screen.getByRole("button", {
      name: "Couple “Work” to the default again",
    });
    await press(reset);
    expect(callsOf("resetProfileValues")).toEqual([[WORK, ["done_sound"]]]);
  });

  test("the profiles that follow are named in one line", async () => {
    await render(page, profiles, DEFAULTS_SCOPE);

    const text = visibleText();
    expect(text).toContain("Work");
    expect(text).toContain("Private, Neutest follow");
    expect(rawTextIn()).toEqual([]);
  });

  test("one that follows alone says so in the singular", async () => {
    await render(page, profiles.slice(0, 2), DEFAULTS_SCOPE);
    expect(visibleText()).toContain("Private follows");
  });

  test("without deviations, all profiles follow", async () => {
    await render(page, profiles.slice(1), DEFAULTS_SCOPE);
    expect(visibleText()).toContain("All profiles follow");
  });

  test("+ Profile offers the following profiles by name and gives one its own value", async () => {
    await render(page, profiles, DEFAULTS_SCOPE);

    const trigger = screen.getByRole("button", { name: "+ Profile" });
    await press(trigger);
    // The menu's entries are the buttons beside the trigger.
    const options = within(trigger.parentElement as HTMLElement)
      .getAllByRole("button")
      .filter((button) => button !== trigger);
    expect(options.map((option) => option.textContent?.trim())).toEqual([
      "Private",
      "Neutest",
    ]);
    await press(options[1]);

    expect(callsOf("holdProfileValue")).toEqual([[NEUTEST, "done_sound"]]);
  });
});
