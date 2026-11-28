// DanyWhere: the order of the page General (user, 2026-11-24; sidecar qa
// 2026-11-24_0141, and since 2026-11-26 qa 2026-11-26_1713, decision):
// four lists, start, hold, instruction and end shortcuts, each a card of its
// own (the instruction shortcuts since qa 2026-11-26_1841, decision), the
// end shortcuts closing with Cancel, then the other shortcuts. "Shortcut
// Behavior", "Second Start Shortcut", "When Held" and "When Tapped" are
// gone: what a combination does follows from the lists it stands in.
import { describe, expect, test } from "bun:test";
import { screen, within } from "@testing-library/react";
import { GeneralSettings } from "./GeneralSettings";
import { DEFAULTS_SCOPE } from "@/lib/profileScope";
import { settingsWith, type SettingsSpec } from "@/test/fixtures";
import { switchTestLanguage } from "@/test/i18n";
import { renderPage } from "@/test/render";
import { setOsType } from "@/test/tauri-mock";
import {
  rawTextIn,
  readableTexts,
  visibleText,
  visibleTexts,
} from "@/test/visible";

const renderGeneral = (spec: SettingsSpec = {}) =>
  renderPage(<GeneralSettings />, {
    settings: settingsWith(spec),
    scope: DEFAULTS_SCOPE,
    acceptWrites: true,
  });

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

/** The values of the name fields in the card `title`. */
const namesIn = (title: string): string[] =>
  (within(card(title)).queryAllByLabelText("Name") as HTMLInputElement[]).map(
    (field) => field.value,
  );

/** The rows to look for in a card; the start shortcuts' combinations are
 * looked for only there, since the hold and end shortcuts show theirs too. */
const START_ROWS = ["⌃ Space", "Also a hold shortcut", "Add Start Shortcut"];
const ROWS = [
  "Also a hold shortcut",
  "Add Start Shortcut",
  "Add Hold Shortcut",
  "Add End Shortcut",
  "Cancel Shortcut",
  "Paste Last Transcript",
  "Copy Last Transcript",
  "Switch Word List",
  "Choose Profile by Application",
];

const ALSO_START =
  "Also a start shortcut: a short tap starts a recording that keeps running.";
const HOLD_ONLY = "Hold shortcut only: a short tap does nothing.";

describe("General: start, hold and end shortcuts", () => {
  test("the cards: start, hold, instruction, end and other shortcuts, then the rest", async () => {
    await renderGeneral();

    expect(cardTitles().slice(0, 6)).toEqual([
      "Start Shortcuts",
      "Hold Shortcuts",
      "Instruction Shortcuts",
      "End Shortcuts",
      "Other Shortcuts",
      "Languages",
    ]);
    expect(rawTextIn()).toEqual([]);
  });

  test("nothing of the old model is left: no behavior, second start shortcut, When Held or When Tapped", async () => {
    await renderGeneral({ defaults: { post_process_mode: "always" } });

    const readable = readableTexts().join(" ");
    for (const gone of [
      "Shortcut Behavior",
      "Second Start Shortcut",
      "When Held",
      "When Tapped",
      // No key says any more whether it post-processes.
      "Post-process",
      "Taken only while post-processing is on",
    ]) {
      expect(readable).not.toContain(gone);
    }
  });

  test("Start Shortcuts: each combination with its hint, then Add Start Shortcut", async () => {
    await renderGeneral();

    const start = card("Start Shortcuts");
    expect(visibleText(start)).toContain(
      "Tap to start a recording. It runs until an end shortcut ends it.",
    );
    expect(shownIn("Start Shortcuts", START_ROWS)).toEqual([
      "⌃ Space",
      "Also a hold shortcut",
      "Add Start Shortcut",
    ]);
    // A start shortcut has no name of its own.
    expect(namesIn("Start Shortcuts")).toEqual([]);
  });

  test("Hold Shortcuts: each named after its keys, with whether it is a start shortcut too", async () => {
    await renderGeneral();

    expect(namesIn("Hold Shortcuts")).toEqual([
      "⌃ Space held",
      "⌃⇧ Space held",
    ]);
    const text = visibleText(card("Hold Shortcuts"));
    expect(text).toContain(
      "Hold to record. Releasing ends the recording this way",
    );
    expect(text.split(ALSO_START).length - 1).toBe(1);
    expect(text.split(HOLD_ONLY).length - 1).toBe(1);
    expect(text.indexOf(ALSO_START)).toBeLessThan(text.indexOf(HOLD_ONLY));
    expect(shownIn("Hold Shortcuts", ROWS)).toEqual(["Add Hold Shortcut"]);
  });

  test("End Shortcuts: the ends, then Cancel last", async () => {
    await renderGeneral();

    expect(shownIn("End Shortcuts", ROWS)).toEqual([
      "Add End Shortcut",
      "Cancel Shortcut",
    ]);
  });

  test("Other Shortcuts: paste, copy, word list, profile by application", async () => {
    await renderGeneral();

    expect(shownIn("Other Shortcuts", ROWS)).toEqual([
      "Paste Last Transcript",
      "Copy Last Transcript",
      "Switch Word List",
      "Choose Profile by Application",
    ]);
  });

  test("on Linux: no Cancel, the instruction shortcuts, and the Linux hint opens End Shortcuts", async () => {
    setOsType("linux");
    await renderGeneral();

    // Instruction shortcuts on a start or hold shortcut's combination
    // (user, 2026-11-28).
    expect(cardTitles().slice(0, 4)).toEqual([
      "Start Shortcuts",
      "Hold Shortcuts",
      "Instruction Shortcuts",
      "End Shortcuts",
    ]);
    const texts = visibleTexts(card("End Shortcuts"));
    expect(texts.indexOf("End Shortcuts")).toBe(0);
    const hint = texts.findIndex((text) => text.startsWith("On Linux,"));
    expect(hint).toBeGreaterThan(-1);
    expect(hint).toBeLessThan(texts.indexOf("Add End Shortcut"));
    expect(visibleText()).not.toContain("Cancel Shortcut");
  });

  test("in German: Starttasten, Haltetasten, Endtasten, with their hints, no raw keys", async () => {
    await switchTestLanguage("de");
    await renderGeneral();

    expect(cardTitles().slice(0, 4)).toEqual([
      "Starttasten",
      "Haltetasten",
      "Anweisungstasten",
      "Endtasten",
    ]);
    expect(visibleText(card("Starttasten"))).toContain("Auch Haltetaste");
    expect(namesIn("Haltetasten")).toEqual([
      "⌃ Leertaste gehalten",
      "⌃⇧ Leertaste gehalten",
    ]);
    const hold = visibleText(card("Haltetasten"));
    expect(hold).toContain(
      "Auch Starttaste: kurzes Antippen startet eine Aufnahme, die weiterläuft.",
    );
    expect(hold).toContain("Nur Haltetaste: kurzes Antippen tut nichts.");
    const readable = readableTexts().join(" ");
    expect(readable).not.toContain("Verhalten der Tastenkombination");
    expect(readable).not.toContain("Zweite Starttaste");
    expect(readable).not.toContain("Beim Halten");
    expect(rawTextIn()).toEqual([]);
  });
});
