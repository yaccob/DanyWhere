// DanyWhere: a profile's page "Profile" (qa 2026-11-22_0107 B5, errors 1
// and 2; qa 2026-11-22_2202 "die Profilseite mit ⌃⌥D gehalten"): the own
// values by name with their values, never "{{number}}" or an internal id;
// a row opens its page and jumps to the setting; each row and the whole
// profile reset to the defaults; the "?" opens the manual's chapter on the
// defaults and profiles.
import { describe, expect, test } from "bun:test";
import { screen, within } from "@testing-library/react";
import type { JumpTarget } from "@/lib/settingJump";
import { ProfileOverview } from "./ProfileOverview";
import { OpenProfilePageProvider } from "./scope/SettingsScope";
import { callsOf } from "@/test/bindings-mock";
import {
  binding,
  defaultValues,
  endKey,
  holdEnd,
  prompt,
  settingsWith,
  wordList,
} from "@/test/fixtures";
import { press, renderPage } from "@/test/render";
import { switchTestLanguage } from "@/test/i18n";
import { rawTextIn, visibleText } from "@/test/visible";
import { OpenManualProvider } from "@/hooks/useOpenManual";

const WORK = "profile_1795300000001";
const PENDE = endKey("end_1795307930035", "ctrl+alt+l", { name: "PEnde" });
const GLISTE = wordList("word_list_1795306966443", "GListe");
const PLISTE = wordList("word_list_1795307006713", "PListe");

const bindings = {
  ...(defaultValues().bindings as Record<string, unknown>),
  transcribe: binding("transcribe", "ctrl+space", {
    current_binding: "ctrl+alt+e",
  }),
};

const settings = settingsWith({
  defaults: { word_lists: [GLISTE] },
  app: {
    post_process_prompts: [
      prompt("prompt_1", "Diktat (DE/EN)"),
      prompt("prompt_2", "Improve Transcriptions"),
    ],
  },
  profiles: [
    {
      id: WORK,
      name: "Pruefkopie",
      own: ["post_process_prompt_order", "hold_ends", "selected_microphone"],
      ownEntries: {
        bindings: { own: ["transcribe"] },
        end_keys: { own: [PENDE.id] },
        word_lists: { own: [PLISTE.id], off: [GLISTE.id] },
      },
      values: {
        post_process_prompt_order: [
          { id: "prompt_1", active: true },
          { id: "prompt_2", active: false },
        ],
        hold_ends: {
          transcribe: holdEnd({ post_process: true, send_context: true }),
          transcribe_with_post_process: holdEnd({ post_process: true }),
        },
        selected_microphone: "MacBook Pro Microphone",
        bindings,
        end_keys: [...(defaultValues().end_keys as unknown[]), PENDE],
        word_lists: [PLISTE],
      },
    },
  ],
});

const opened: [string, string, JumpTarget | undefined][] = [];

const renderOverview = () => {
  opened.length = 0;
  return renderPage(
    <OpenProfilePageProvider
      value={(profileId, page, jump) => opened.push([profileId, page, jump])}
    >
      <ProfileOverview profileId={WORK} />
    </OpenProfilePageProvider>,
    { settings },
  );
};

/** The row of an own value, by the text it shows. */
const row = (text: string) => screen.getByRole("button", { name: text });

describe("the page Profile", () => {
  test("counts the own values in its title", async () => {
    await renderOverview();
    expect(screen.getByText("Own Values (7)")).toBeTruthy();
  });

  test("names each own value and shows its value in words", async () => {
    await renderOverview();

    const text = visibleText();
    for (const expected of [
      "Post Process Active Prompts: Diktat (DE/EN)",
      "General When Held: ⌥⌃E held: Post-process, Send Context · ⌃⇧ Space held: Post-process",
      "General Microphone: MacBook Pro Microphone",
      "General Shortcuts: Start Shortcut: ⌥⌃E",
      "General End Shortcuts: PEnde",
      "Advanced Additional Lists: PListe",
      "Advanced Additional Lists: GListe (off)",
    ]) {
      expect(text).toContain(expected);
    }
  });

  test("shows no placeholder, key or internal id, also not in tooltips and labels", async () => {
    await renderOverview();
    expect(rawTextIn()).toEqual([]);
  });

  test("a row opens the value's page and jumps to its setting", async () => {
    await renderOverview();

    await press(row("General End Shortcuts: PEnde"));
    await press(row("Advanced Additional Lists: GListe (off)"));
    await press(row("Post Process Active Prompts: Diktat (DE/EN)"));

    expect(opened).toEqual([
      [
        WORK,
        "general",
        { profileId: WORK, field: "end_keys", entry: PENDE.id },
      ],
      [
        WORK,
        "advanced",
        { profileId: WORK, field: "word_lists", entry: GLISTE.id },
      ],
      [
        WORK,
        "postprocessing",
        { profileId: WORK, field: "post_process_prompt_order" },
      ],
    ]);
  });

  test("a row's reset couples that value or entry again", async () => {
    await renderOverview();

    await press(
      screen.getByRole("button", { name: "Reset End Shortcuts: PEnde" }),
    );
    await press(screen.getByRole("button", { name: "Reset When Held" }));

    expect(callsOf("resetProfileEntry")).toEqual([
      [WORK, "end_keys", PENDE.id],
    ]);
    expect(callsOf("resetProfileValues")).toEqual([[WORK, ["hold_ends"]]]);
  });

  test("the whole profile's reset asks first, naming the values", async () => {
    await renderOverview();

    await press(
      screen.getByRole("button", { name: "Whole Profile to Defaults" }),
    );
    const dialog = screen.getByRole("dialog");
    const text = visibleText(dialog);
    expect(text).toContain("Reset “Pruefkopie” completely to the defaults?");
    for (const name of [
      "Active Prompts",
      "When Held",
      "Microphone",
      "Shortcuts: Start Shortcut",
      "End Shortcuts: PEnde",
      "Additional Lists: PListe",
      "Additional Lists: GListe",
    ]) {
      expect(text).toContain(name);
    }
    expect(text).toContain("The profile's name and applications stay.");
    expect(rawTextIn(dialog)).toEqual([]);
    expect(callsOf("resetWholeProfile")).toEqual([]);

    await press(within(dialog).getByRole("button", { name: "Reset" }));
    expect(callsOf("resetWholeProfile")).toEqual([[WORK]]);
  });

  test("a profile that follows the defaults says so", async () => {
    await renderPage(<ProfileOverview profileId="profile_1795300000009" />, {
      settings: settingsWith({
        profiles: [{ id: "profile_1795300000009", name: "Neutest" }],
      }),
    });

    expect(screen.getByText("Own Values (0)")).toBeTruthy();
    expect(
      screen.getByText("This profile follows the defaults in everything."),
    ).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
  });
});

// DanyWhere: the "?" next to the page's title opens the manual's chapter on
// the defaults and profiles, on the manual's page of the settings window
// (`OpenManualProvider`; what that page shows: ManualPage.test.tsx).
describe("the manual's chapter from the page Profile", () => {
  const CHAPTER = "how-it-works/defaults-and-profiles/";
  const NAME = "Open the manual: Defaults and Profiles";
  const chapters: (string | undefined)[] = [];
  const renderWithManual = () => {
    chapters.length = 0;
    return renderPage(
      <OpenManualProvider value={(page) => chapters.push(page)}>
        <ProfileOverview profileId={WORK} />
      </OpenManualProvider>,
      { settings },
    );
  };
  const helpButton = (name = NAME) => screen.getByRole("button", { name });

  test("a button next to the title opens the chapter", async () => {
    await renderWithManual();

    const title = screen.getByRole("heading", { name: "Own Values (7)" });
    expect(title.parentElement?.contains(helpButton())).toBe(true);

    await press(helpButton());

    expect(chapters).toEqual([CHAPTER]);
    expect(callsOf("resetWholeProfile")).toEqual([]);
    expect(callsOf("resetProfileValues")).toEqual([]);
  });

  test("shows a question mark icon and a tooltip", async () => {
    await renderWithManual();

    const icon = helpButton().querySelector("svg");
    expect(icon?.getAttribute("class")).toContain(
      "lucide-circle-question-mark",
    );
    expect(icon?.getAttribute("aria-hidden")).toBe("true");
    expect(helpButton().textContent).toBe("");
    expect(helpButton().getAttribute("title")).toBe(NAME);
    expect(rawTextIn()).toEqual([]);
  });

  test("names the chapter in French", async () => {
    await switchTestLanguage("fr");
    await renderWithManual();

    await press(helpButton("Ouvrir le manuel : Paramètres de base et profils"));

    expect(chapters).toEqual([CHAPTER]);
    expect(rawTextIn()).toEqual([]);
  });

  test("outside the settings window there is no button that does nothing", async () => {
    await renderOverview();

    expect(screen.getByText("Own Values (7)")).toBeTruthy();
    expect(screen.queryByRole("button", { name: NAME }) === null).toBe(true);
  });
});
