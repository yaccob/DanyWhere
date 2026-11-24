// DanyWhere: the "?" next to the title of the page "Profile Settings" opens
// the manual's chapter on the defaults and profiles, on the manual's page of
// the settings window (`OpenManualProvider`; what that page shows:
// ManualPage.test.tsx).
import { describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import { ProfilesSettings } from "./ProfilesSettings";
import { OpenManualProvider } from "@/hooks/useOpenManual";
import { callsOf } from "@/test/bindings-mock";
import { settingsWith } from "@/test/fixtures";
import { press, renderPage } from "@/test/render";
import { switchTestLanguage } from "@/test/i18n";
import { rawTextIn } from "@/test/visible";

const CHAPTER = "how-it-works/defaults-and-profiles/";
const NAME = "Open the manual: Defaults and Profiles";

const settings = settingsWith({
  profiles: [
    { id: "profile_1795300000001", name: "Work" },
    { id: "profile_1795300000002", name: "Private" },
  ],
});

/** The manual's chapters the page asked the settings window to show (none
 * for its home page). */
const openedChapters: (string | undefined)[] = [];

const renderProfiles = () => {
  openedChapters.length = 0;
  return renderPage(
    <OpenManualProvider value={(page) => openedChapters.push(page)}>
      <ProfilesSettings />
    </OpenManualProvider>,
    { settings },
  );
};

const helpButton = (name = NAME) => screen.getByRole("button", { name });

describe("the manual's chapter from Profile Settings", () => {
  test("a button next to the title opens the chapter", async () => {
    await renderProfiles();

    const title = screen.getByRole("heading", { name: "Profiles" });
    expect(title.parentElement?.contains(helpButton())).toBe(true);

    await press(helpButton());

    expect(openedChapters).toEqual([CHAPTER]);
    expect(callsOf("switchProfile")).toEqual([]);
  });

  test("shows a question mark icon and a tooltip", async () => {
    await renderProfiles();

    // Lucide's CircleHelp, the same "?" as next to End Shortcuts.
    const icon = helpButton().querySelector("svg");
    expect(icon?.getAttribute("class")).toContain(
      "lucide-circle-question-mark",
    );
    expect(icon?.getAttribute("aria-hidden")).toBe("true");
    expect(helpButton().textContent).toBe("");
    expect(helpButton().getAttribute("title")).toBe(NAME);
    expect(rawTextIn()).toEqual([]);
  });

  test("names the chapter in German", async () => {
    await switchTestLanguage("de");
    await renderProfiles();

    await press(helpButton("Handbuch öffnen: Grundeinstellungen und Profile"));

    expect(openedChapters).toEqual([CHAPTER]);
    expect(rawTextIn()).toEqual([]);
  });

  test("outside the settings window there is no button that does nothing", async () => {
    await renderPage(<ProfilesSettings />, { settings });

    expect(screen.getByRole("heading", { name: "Profiles" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: NAME }) === null).toBe(true);
  });
});
