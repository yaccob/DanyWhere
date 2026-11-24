// DanyWhere: the order of the page General (user, 2026-11-24; sidecar qa
// 2026-11-24_0141): start and end shortcuts follow each other. First the
// start shortcuts with the behavior on top, then the end shortcuts, which
// begin with "When Held" (releasing a held start shortcut is an end too) and
// close with Cancel, then the other shortcuts.
import { describe, expect, test } from "bun:test";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { GeneralSettings } from "./GeneralSettings";
import { DEFAULTS_SCOPE } from "@/lib/profileScope";
import { settingsWith, type SettingsSpec } from "@/test/fixtures";
import { renderPage, settle } from "@/test/render";
import { setOsType } from "@/test/tauri-mock";
import { rawTextIn, visibleText, visibleTexts } from "@/test/visible";

const renderGeneral = (spec: SettingsSpec = {}) =>
  renderPage(<GeneralSettings />, {
    settings: settingsWith(spec),
    scope: DEFAULTS_SCOPE,
    acceptWrites: true,
  });

// Both start shortcuts show, and both can be held.
const holdBoth: SettingsSpec = {
  defaults: { shortcut_activation: "push_to_talk", post_process_enabled: true },
};

/** The headings of the page's cards, top to bottom. */
const cardTitles = (): string[] =>
  Array.from(document.querySelectorAll("h2")).map(
    (heading) => heading.textContent?.trim() ?? "",
  );

/** The card under the heading `title`: its heading and its rows. */
const card = (title: string): HTMLElement => {
  const heading = screen.getByRole("heading", { level: 2, name: title });
  const group = heading.closest(".space-y-2");
  if (!(group instanceof HTMLElement)) throw new Error(`no card ${title}`);
  return group;
};

/** Which of `names` the card `title` shows, in the order it shows them. */
const shownIn = (title: string, names: string[]): string[] => {
  const texts = visibleTexts(card(title));
  return names
    .filter((name) => texts.includes(name))
    .sort((a, b) => texts.indexOf(a) - texts.indexOf(b));
};

const ROWS = [
  "Shortcut Behavior",
  "Start Shortcut",
  "Second Start Shortcut",
  "When Held",
  "⌃ Space held",
  "⌃⇧ Space held",
  "Add End Shortcut",
  "Cancel Shortcut",
  "Paste Last Transcript",
  "Copy Last Transcript",
  "Switch Word List",
  "Choose Profile by Application",
];

describe("General: start and end shortcuts together", () => {
  test("the cards: start shortcuts, end shortcuts, other shortcuts, then the rest", async () => {
    await renderGeneral(holdBoth);

    expect(cardTitles().slice(0, 4)).toEqual([
      "Start Shortcuts",
      "End Shortcuts",
      "Other Shortcuts",
      "Languages",
    ]);
    expect(rawTextIn()).toEqual([]);
  });

  test("Start Shortcuts: the behavior first, then both start shortcuts, nothing else", async () => {
    await renderGeneral(holdBoth);

    expect(shownIn("Start Shortcuts", ROWS)).toEqual([
      "Shortcut Behavior",
      "Start Shortcut",
      "Second Start Shortcut",
    ]);
  });

  test("End Shortcuts: When Held first, then the ends, Cancel last", async () => {
    await renderGeneral(holdBoth);

    expect(shownIn("End Shortcuts", ROWS)).toEqual([
      "When Held",
      "⌃ Space held",
      "⌃⇧ Space held",
      "Add End Shortcut",
      "Cancel Shortcut",
    ]);
  });

  test("End Shortcuts while the shortcuts toggle: the ends, Cancel last", async () => {
    await renderGeneral({ defaults: { shortcut_activation: "toggle" } });

    expect(shownIn("End Shortcuts", ROWS)).toEqual([
      "Add End Shortcut",
      "Cancel Shortcut",
    ]);
  });

  test("Other Shortcuts: paste, copy, word list, profile by application", async () => {
    await renderGeneral(holdBoth);

    expect(shownIn("Other Shortcuts", ROWS)).toEqual([
      "Paste Last Transcript",
      "Copy Last Transcript",
      "Switch Word List",
      "Choose Profile by Application",
    ]);
  });

  test("on Linux: no Cancel, and the Linux hint opens End Shortcuts", async () => {
    setOsType("linux");
    await renderGeneral(holdBoth);

    const texts = visibleTexts(card("End Shortcuts"));
    expect(texts.indexOf("End Shortcuts")).toBe(0);
    const hint = texts.findIndex((text) => text.startsWith("On Linux,"));
    expect(hint).toBeGreaterThan(-1);
    expect(hint).toBeLessThan(texts.indexOf("When Held"));
    expect(visibleText()).not.toContain("Cancel Shortcut");
  });

  test("Start Shortcut's information names the end shortcuts for holding too", async () => {
    await renderGeneral(holdBoth);

    let row: HTMLElement | null = screen.getByText("Start Shortcut");
    const info = () =>
      row
        ? within(row).queryByRole("button", { name: "More information" })
        : null;
    while (row && !info()) row = row.parentElement;
    const button = info();
    if (!button) throw new Error("no information for Start Shortcut");
    // The information shows on a press (`showOnPress`).
    await act(async () => {
      fireEvent.pointerDown(button);
    });
    await settle();

    expect(screen.getByRole("tooltip").textContent).toBe(
      "Starts a recording. How it ends is up to the end shortcuts, also when held.",
    );
  });
});
