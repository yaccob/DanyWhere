// DanyWhere: the jumps to a profile's setting (qa 2026-11-22_1603 B5,
// 2026-11-22_1638): "Open in …" under a default (the only place that says
// `settings.profileView.deviations.openIn`, `ScopedSetting`) and a row of
// the page "Profile" open the profile's page that has the setting, and that
// setting, not another, scrolls into view and lights up. A jump to a setting
// this device doesn't show just opens the page.
import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { screen, waitFor, within } from "@testing-library/react";
import React, { useState } from "react";
import type { NavLocation } from "@/lib/navigation";
import { SettingsPage } from "./SettingsPage";
import { useSettingJump } from "./settings/profiles/scope/settingJumpStore";
import {
  defaultValues,
  endKey,
  holdKey,
  settingsWith,
  type SettingsSpec,
} from "@/test/fixtures";
import { press, renderPage, settle } from "@/test/render";
import { isVisible, visibleText } from "@/test/visible";

const WORK = "profile_1795300000001";
const PENDE = endKey("end_1795307930035", "ctrl+alt+l", { name: "PEnde" });
const PHALTEN = holdKey("hold_1795307930036", "ctrl+alt+h", {
  name: "PHalten",
});

const spec: SettingsSpec = {
  profiles: [
    {
      id: WORK,
      name: "Work",
      own: ["selected_channel"],
      ownEntries: {
        hold_keys: { own: [PHALTEN.id] },
        end_keys: { own: [PENDE.id] },
      },
      values: {
        hold_keys: [...(defaultValues().hold_keys as unknown[]), PHALTEN],
        end_keys: [...(defaultValues().end_keys as unknown[]), PENDE],
        selected_channel: 1,
      },
    },
    { id: "profile_1795300000002", name: "Private" },
  ],
};

const navigated: NavLocation[] = [];

/** The settings window's navigation around the page, as App keeps it. */
const Window: React.FC<{ start: NavLocation }> = ({ start }) => {
  const [location, setLocation] = useState(start);
  return (
    <SettingsPage
      location={location}
      onNavigate={(next) => {
        navigated.push(next);
        setLocation(next);
      }}
      onPreviewOnboarding={() => {}}
      onRunOnboarding={() => {}}
      onRunModelMigration={() => {}}
    />
  );
};

const scrolledTo: Element[] = [];
const scrollIntoView = Element.prototype.scrollIntoView;

beforeEach(() => {
  navigated.length = 0;
  scrolledTo.length = 0;
  Element.prototype.scrollIntoView = function (this: Element) {
    scrolledTo.push(this);
  };
});

afterEach(() => {
  Element.prototype.scrollIntoView = scrollIntoView;
});

const openWindow = (start: NavLocation) =>
  renderPage(<Window start={start} />, { settings: settingsWith(spec) });

/** The "Open in Work" of the deviating profile's row that says `summary`. */
const openInWorkBeside = (summary: string): HTMLElement => {
  const row = screen.getByText(summary).parentElement as HTMLElement;
  return within(row).getByRole("button", { name: "Open in Work" });
};

/** The settings that are lit after a jump (`JUMP_LIT` in ScopedSetting). */
const lit = () =>
  Array.from(document.querySelectorAll('[class*="bg-logo-primary/15"]'));

/** Waits until exactly one setting is lit and gives it. */
const litSetting = async (): Promise<Element> => {
  await waitFor(() => expect(lit()).toHaveLength(1));
  return lit()[0];
};

describe("Open in … under a default", () => {
  test("is offered for each deviating profile of a large setting, by name", async () => {
    await openWindow({ kind: "defaults", page: "general" });

    const buttons = screen
      .getAllByRole("button", { name: "Open in Work" })
      .filter(isVisible);
    // Hold Shortcuts and End Shortcuts; the channel isn't shown on this
    // device, and Private follows everywhere.
    expect(buttons.map((button) => visibleText(button.parentElement!))).toEqual(
      ["Work own: PHalten Open in Work", "Work own: PEnde Open in Work"],
    );
    expect(
      screen.queryByRole("button", { name: "Open in Private" }),
    ).toBeNull();
  });

  test("opens the profile's page and lights up the end shortcuts there", async () => {
    await openWindow({ kind: "defaults", page: "general" });

    await press(openInWorkBeside("own: PEnde"));

    expect(navigated).toEqual([
      { kind: "profile", profileId: WORK, page: "general" },
    ]);
    const setting = await litSetting();
    expect(setting.textContent).toContain("Add End Shortcut");
    expect(setting.textContent).not.toContain("Add Hold Shortcut");
    expect(scrolledTo).toEqual([setting]);
    // The page is the profile's: its own end shortcut is there.
    expect(visibleText()).toContain("PEnde own");
  });

  test("lights up the hold shortcuts when that is the setting", async () => {
    await openWindow({ kind: "defaults", page: "general" });

    await press(openInWorkBeside("own: PHalten"));

    const setting = await litSetting();
    expect(setting.textContent).toContain("Add Hold Shortcut");
    expect(setting.textContent).not.toContain("Add End Shortcut");
    expect(visibleText()).toContain("PHalten own");
  });
});

describe("a row of the page Profile", () => {
  test("opens the value's page and lights up its setting", async () => {
    await openWindow({ kind: "profile", profileId: WORK, page: "profile" });

    await press(
      screen.getByRole("button", { name: "General End Shortcuts: PEnde" }),
    );

    expect(navigated).toEqual([
      { kind: "profile", profileId: WORK, page: "general" },
    ]);
    const setting = await litSetting();
    expect(setting.textContent).toContain("Add End Shortcut");
    expect(scrolledTo).toEqual([setting]);
  });

  test("a setting this device doesn't show just opens the page", async () => {
    await openWindow({ kind: "profile", profileId: WORK, page: "profile" });

    // A microphone with one channel shows no channel to choose.
    await press(
      screen.getByRole("button", { name: "General Input Channel: Channel 2" }),
    );
    await settle();
    await new Promise((resolve) => setTimeout(resolve, 50));
    await settle();

    expect(navigated).toEqual([
      { kind: "profile", profileId: WORK, page: "general" },
    ]);
    expect(lit()).toEqual([]);
    expect(scrolledTo).toEqual([]);
    // The jump still waits for a setting that never shows; it lapses after
    // JUMP_WAIT_MS, so a later visit of the page doesn't jump.
    expect(useSettingJump.getState().jump?.field).toBe("selected_channel");
  });
});
