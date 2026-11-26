// DanyWhere: the quiet bar after a coupled value was changed in a profile
// (qa 2026-11-22_0107 B2, B9 and error 2): it names the setting, and an
// entry by its name, never by its id, also one the profile switched off;
// "Undo" couples it again, "Change the Default Instead" moves the value.
import { describe, expect, test } from "bun:test";
import { act, screen } from "@testing-library/react";
import { DoneSound } from "@/components/settings/DoneSound";
import { ProfileScopedPage } from "../ProfileScopedPage";
import { DecoupleBar } from "./DecoupleBar";
import { ScopedSetting } from "./ScopedSetting";
import { useDecoupleBar } from "./decoupleBarStore";
import { callsOf } from "@/test/bindings-mock";
import {
  defaultValues,
  endKey,
  settingsWith,
  wordList,
  type ProfileSpec,
} from "@/test/fixtures";
import { press, renderPage } from "@/test/render";
import { rawTextIn, visibleText } from "@/test/visible";

const NEUTEST = "profile_1795300000003";
const GLISTE = wordList("word_list_1795306966443", "GListe");
const PLISTE = wordList("word_list_1795307006713", "PListe");
const PENDE = endKey("end_1795307930035", "ctrl+alt+l", { name: "PEnde" });

const neutest: ProfileSpec = {
  id: NEUTEST,
  name: "Neutest",
  values: {
    word_lists: [PLISTE],
    end_keys: [...(defaultValues().end_keys as unknown[]), PENDE],
  },
  ownEntries: {
    word_lists: { own: [PLISTE.id], off: [GLISTE.id] },
    end_keys: { own: [PENDE.id] },
  },
};

const renderBar = (bar: { field: string; entry?: string }) =>
  renderPage(<DecoupleBar profileId={NEUTEST} />, {
    settings: settingsWith({
      defaults: { word_lists: [GLISTE] },
      profiles: [neutest],
    }),
  }).then(async (result) => {
    await act(async () => {
      useDecoupleBar.getState().show({ profileId: NEUTEST, ...bar });
    });
    return result;
  });

describe("the bar after decoupling", () => {
  test("names a list entry the profile switched off by its name", async () => {
    await renderBar({ field: "word_lists", entry: GLISTE.id });

    expect(screen.getByRole("status").textContent).toContain(
      "“Additional Lists: GListe” now applies only to “Neutest”.",
    );
    expect(rawTextIn()).toEqual([]);
  });

  test("names the profile's own entries by their names", async () => {
    await renderBar({ field: "word_lists", entry: PLISTE.id });
    expect(visibleText()).toContain("“Additional Lists: PListe” now applies");

    await act(async () => {
      useDecoupleBar
        .getState()
        .show({ profileId: NEUTEST, field: "end_keys", entry: PENDE.id });
    });
    expect(visibleText()).toContain("“End Shortcuts: PEnde” now applies");
  });

  test("names a shortcut by its name", async () => {
    await renderBar({ field: "bindings", entry: "paste_last_transcript" });
    expect(visibleText()).toContain(
      "“Shortcuts: Paste Last Transcript” now applies only to “Neutest”.",
    );
  });

  test("names a start shortcut by its keys and a hold shortcut after them", async () => {
    await renderBar({ field: "start_keys", entry: "start" });
    expect(visibleText()).toContain(
      "“Start Shortcuts: ⌃ Space” now applies only to “Neutest”.",
    );

    await act(async () => {
      useDecoupleBar
        .getState()
        .show({ profileId: NEUTEST, field: "hold_keys", entry: "hold_post" });
    });
    expect(visibleText()).toContain(
      "“Hold Shortcuts: ⌃⇧ Space held” now applies only to “Neutest”.",
    );
    expect(rawTextIn()).toEqual([]);
  });

  test("Change the Default Instead moves an entry to the defaults", async () => {
    await renderBar({ field: "end_keys", entry: PENDE.id });

    await press(
      screen.getByRole("button", { name: "Change the Default Instead" }),
    );

    expect(callsOf("setProfileEntry")).toEqual([
      [{ kind: "defaults" }, "end_keys", PENDE.id, PENDE],
    ]);
    expect(callsOf("resetProfileEntry")).toEqual([
      [NEUTEST, "end_keys", PENDE.id],
    ]);
    expect(screen.queryByRole("status")).toBeNull();
  });

  test("doesn't show on another profile's page", async () => {
    await renderBar({ field: "word_lists", entry: GLISTE.id });
    await act(async () => {
      useDecoupleBar.getState().show({
        profileId: "profile_1795300000001",
        field: "word_lists",
        entry: GLISTE.id,
      });
    });
    expect(screen.queryByRole("status")).toBeNull();
  });
});

describe("a change of a coupled value on a profile's page", () => {
  const WORK = "profile_1795300000001";

  const renderWorkPage = () =>
    renderPage(
      <ProfileScopedPage profileId={WORK} page="general">
        <ScopedSetting field="done_sound">
          <DoneSound />
        </ScopedSetting>
      </ProfileScopedPage>,
      {
        settings: settingsWith({ profiles: [{ id: WORK, name: "Work" }] }),
        acceptWrites: true,
      },
    );

  test("shows the bar naming the setting and the profile", async () => {
    await renderWorkPage();
    await press(screen.getByRole("checkbox", { name: "Sound When Done" }));

    expect(callsOf("setProfileValue")).toEqual([
      [{ kind: "profile", id: WORK }, "done_sound", false],
    ]);
    expect(screen.getByRole("status").textContent).toContain(
      "“Sound When Done” now applies only to “Work”.",
    );
  });

  test("Undo couples the value again", async () => {
    await renderWorkPage();
    await press(screen.getByRole("checkbox", { name: "Sound When Done" }));
    await press(screen.getByRole("button", { name: "Undo" }));

    expect(callsOf("resetProfileValues")).toEqual([[WORK, ["done_sound"]]]);
    expect(screen.queryByRole("status")).toBeNull();
  });

  test("Change the Default Instead gives the default the profile's value", async () => {
    await renderWorkPage();
    await press(screen.getByRole("checkbox", { name: "Sound When Done" }));
    await press(
      screen.getByRole("button", { name: "Change the Default Instead" }),
    );

    expect(callsOf("useProfileValueAsDefault")).toEqual([[WORK, "done_sound"]]);
  });
});
