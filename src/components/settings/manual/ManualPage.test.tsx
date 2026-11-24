// DanyWhere: the user manual as a page of the settings window, like History
// and About (not a window of its own, which on macOS opened on another
// Space). The sidebar's Manual and the "?" next to End Shortcuts, Profile
// Settings and a profile's page Profile show it;
// a frame fills the content area with the manual in the interface's
// language. Inside it, links to other pages of the manual stay there, links
// leaving the app open in the default browser, and the page on screen
// follows Application Theme. A page the build lacks gives way to its
// language's home page; a build without the manual says so.
import { describe, expect, test } from "bun:test";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import React, { useState } from "react";
import type { NavLocation } from "@/lib/navigation";
import { Sidebar } from "@/components/Sidebar";
import { SettingsPage } from "@/components/SettingsPage";
import { ThemeSelector } from "@/components/settings/ThemeSelector";
import { settingsWith } from "@/test/fixtures";
import { press, renderPage, settle } from "@/test/render";
import { switchTestLanguage } from "@/test/i18n";
import { ipcCalls } from "@/test/tauri-mock";
import { isVisible, rawTextIn, visibleText } from "@/test/visible";
import {
  APP_INDEX,
  manualPage,
  manualRequests,
  serveManualPages,
  withoutManual,
} from "@/test/manual-frame";

const CHAPTER = "how-it-works/start-and-end-shortcuts/";

/** The settings window's sidebar and page, with the navigation App keeps. */
const Window: React.FC<{ start: NavLocation }> = ({ start }) => {
  const [location, setLocation] = useState(start);
  return (
    <>
      <Sidebar location={location} onNavigate={setLocation} />
      <SettingsPage
        location={location}
        onNavigate={setLocation}
        onPreviewOnboarding={() => {}}
        onRunOnboarding={() => {}}
        onRunModelMigration={() => {}}
      />
    </>
  );
};

const renderWindow = (
  start: NavLocation = { kind: "about" },
  settings = settingsWith(),
) => renderPage(<Window start={start} />, { settings });

const frame = (): HTMLIFrameElement | null => document.querySelector("iframe");

/** The frame, once its page has loaded. */
const loadedFrame = async (): Promise<HTMLIFrameElement> => {
  await waitFor(() =>
    expect(
      frame()?.contentDocument?.querySelector('meta[name="generator"]'),
    ).toBeTruthy(),
  );
  await settle();
  return frame() as HTMLIFrameElement;
};

const isCurrent = (name: string): boolean =>
  within(screen.getByRole("navigation"))
    .getByRole("button", { name })
    .getAttribute("aria-current") === "page";

/** The addresses the default browser was asked to open. */
const openedInBrowser = (): string[] =>
  ipcCalls()
    .filter((call) => call.cmd === "plugin:opener|open_url")
    .map((call) => (call.args as { url: string }).url);

describe("the manual's page in the settings window", () => {
  test("the sidebar's Manual shows the English manual's home page", async () => {
    await renderWindow();
    expect(frame() === null).toBe(true);

    await press(screen.getByRole("button", { name: "Manual" }));

    expect(frame()?.getAttribute("src")).toBe("/manual/");
    expect(isCurrent("Manual")).toBe(true);
    expect(isCurrent("About")).toBe(false);
    expect(rawTextIn()).toEqual([]);
  });

  test("in German it shows the German manual", async () => {
    await switchTestLanguage("de");
    await renderWindow();

    await press(screen.getByRole("button", { name: "Handbuch" }));

    expect(frame()?.getAttribute("src")).toBe("/manual/de/");
  });

  test("every other interface language gets the English manual", async () => {
    await switchTestLanguage("ja");
    await renderWindow({ kind: "manual" });

    expect(frame()?.getAttribute("src")).toBe("/manual/");
  });

  test("the frame fills the content area, without a border", async () => {
    await renderWindow({ kind: "manual" });

    // happy-dom lays nothing out: the frame takes the column's whole width
    // and the height left, in a column as high as the content area.
    const classes = frame()?.getAttribute("class")?.split(/\s+/) ?? [];
    expect(classes).toContain("border-0");
    expect(classes).toContain("w-full");
    expect(classes).toContain("flex-1");
    expect(frame()?.getAttribute("title")).toBe("Manual");
  });

  test("the ? next to End Shortcuts opens the manual at its chapter", async () => {
    await renderWindow({ kind: "defaults", page: "general" });

    await press(
      screen.getByRole("button", {
        name: "Open the manual: Start and End Shortcuts",
      }),
    );

    expect(frame()?.getAttribute("src")).toBe(
      "/manual/how-it-works/start-and-end-shortcuts/",
    );
    expect(isCurrent("Manual")).toBe(true);
  });

  test("in German the ? opens the German chapter", async () => {
    await switchTestLanguage("de");
    await renderWindow({ kind: "defaults", page: "general" });

    await press(
      screen.getByRole("button", {
        name: "Handbuch öffnen: Start- und Endtasten",
      }),
    );

    expect(frame()?.getAttribute("src")).toBe(
      "/manual/de/how-it-works/start-and-end-shortcuts/",
    );
  });

  // The "?" on the profile pages: the chapter on the defaults and profiles.
  const WORK = "profile_1795300000001";
  const withWork = settingsWith({ profiles: [{ id: WORK, name: "Work" }] });

  test("the ? on Profile Settings opens the manual at its chapter", async () => {
    await renderWindow({ kind: "profiles" }, withWork);

    await press(
      screen.getByRole("button", {
        name: "Open the manual: Defaults and Profiles",
      }),
    );

    expect(frame()?.getAttribute("src")).toBe(
      "/manual/how-it-works/defaults-and-profiles/",
    );
    expect(isCurrent("Manual")).toBe(true);
    expect(isCurrent("Profile Settings")).toBe(false);
  });

  test("in German the ? on Profile Settings opens the German chapter", async () => {
    await switchTestLanguage("de");
    await renderWindow({ kind: "profiles" }, withWork);

    await press(
      screen.getByRole("button", {
        name: "Handbuch öffnen: Grundeinstellungen und Profile",
      }),
    );

    expect(frame()?.getAttribute("src")).toBe(
      "/manual/de/how-it-works/defaults-and-profiles/",
    );
  });

  test("the ? on a profile's page Profile opens the manual at its chapter", async () => {
    await renderWindow(
      { kind: "profile", profileId: WORK, page: "profile" },
      withWork,
    );

    await press(
      screen.getByRole("button", {
        name: "Open the manual: Defaults and Profiles",
      }),
    );

    expect(frame()?.getAttribute("src")).toBe(
      "/manual/how-it-works/defaults-and-profiles/",
    );
    expect(isCurrent("Manual")).toBe(true);
  });

  test("in German the ? on a profile's page Profile opens the German chapter", async () => {
    await switchTestLanguage("de");
    await renderWindow(
      { kind: "profile", profileId: WORK, page: "profile" },
      withWork,
    );

    await press(
      screen.getByRole("button", {
        name: "Handbuch öffnen: Grundeinstellungen und Profile",
      }),
    );

    expect(frame()?.getAttribute("src")).toBe(
      "/manual/de/how-it-works/defaults-and-profiles/",
    );
    expect(isCurrent("Handbuch")).toBe(true);
  });

  test("a manual on screen goes to the chapter asked for", async () => {
    const page = (location: NavLocation) => (
      <SettingsPage
        location={location}
        onNavigate={() => {}}
        onPreviewOnboarding={() => {}}
        onRunOnboarding={() => {}}
        onRunModelMigration={() => {}}
      />
    );
    const { rerender } = await renderPage(page({ kind: "manual" }));
    const shown = await loadedFrame();

    rerender(page({ kind: "manual", page: CHAPTER }));
    await settle();

    // The same frame, not a new one.
    expect(frame() === shown).toBe(true);
    expect(shown.getAttribute("src")).toBe(
      "/manual/how-it-works/start-and-end-shortcuts/",
    );
    await waitFor(() =>
      expect(manualRequests()).toContain(
        "/manual/how-it-works/start-and-end-shortcuts/",
      ),
    );
  });
});

describe("links inside the manual", () => {
  const LINKS =
    '<a id="web" href="https://github.com/cjpais/Handy">Handy</a>' +
    '<a id="mail" href="mailto:someone@example.com">Mail</a>' +
    '<a id="chapter" href="/manual/how-it-works/start-and-end-shortcuts/">Chapter</a>' +
    '<a id="relative" href="how-it-works/">Relative</a>' +
    '<a id="anchor" href="#intro">Anchor</a>' +
    '<a id="app" href="/">App</a>' +
    '<a id="script" href="javascript:void(0)">Script</a>' +
    '<a id="blank" href="/manual/de/" target="_blank">New window</a>' +
    '<a id="top" href="/manual/fr/" target="_top">Top</a>' +
    '<a id="nested" href="https://example.com/"><span id="inner">Inside</span></a>';

  /** Clicks `id` in the manual's page; true when the page may follow it. */
  const click = (shown: HTMLIFrameElement, id: string): boolean => {
    const link = shown.contentDocument?.getElementById(id);
    if (!link) throw new Error(`no ${id} on the page`);
    return fireEvent.click(link);
  };

  const showLinks = async () => {
    serveManualPages({ "/manual/": manualPage(LINKS) });
    await renderWindow({ kind: "manual" });
    return loadedFrame();
  };

  test("a website opens in the default browser, the manual stays", async () => {
    const shown = await showLinks();

    expect(click(shown, "web")).toBe(false);

    expect(openedInBrowser()).toEqual(["https://github.com/cjpais/Handy"]);
    expect(shown.contentWindow?.location.pathname).toBe("/manual/");
  });

  test("an e-mail address opens in the mail program", async () => {
    const shown = await showLinks();

    expect(click(shown, "mail")).toBe(false);

    expect(openedInBrowser()).toEqual(["mailto:someone@example.com"]);
  });

  test("a click on what a link holds counts as the link's", async () => {
    const shown = await showLinks();

    expect(click(shown, "inner")).toBe(false);

    expect(openedInBrowser()).toEqual(["https://example.com/"]);
  });

  test("another page of the manual opens in the frame", async () => {
    const shown = await showLinks();

    expect(click(shown, "chapter")).toBe(true);

    await waitFor(() =>
      expect(manualRequests()).toContain(
        "/manual/how-it-works/start-and-end-shortcuts/",
      ),
    );
    expect(openedInBrowser()).toEqual([]);
  });

  test("an anchor and a relative link stay in the manual", async () => {
    const shown = await showLinks();

    expect(click(shown, "anchor")).toBe(true);
    expect(click(shown, "relative")).toBe(true);

    await waitFor(() =>
      expect(manualRequests()).toContain("/manual/how-it-works/"),
    );
    expect(openedInBrowser()).toEqual([]);
  });

  // Each page the frame loads gets its links decided. (happy-dom keeps the
  // first page's window after a navigation inside the frame, so the next
  // page comes here by the page's own address, as the "?" brings it.)
  test("the links of the next page are decided too", async () => {
    serveManualPages({
      "/manual/": manualPage("<h1>Home</h1>"),
      "/manual/how-it-works/start-and-end-shortcuts/": manualPage(LINKS),
    });
    const page = (location: NavLocation) => (
      <SettingsPage
        location={location}
        onNavigate={() => {}}
        onPreviewOnboarding={() => {}}
        onRunOnboarding={() => {}}
        onRunModelMigration={() => {}}
      />
    );
    const { rerender } = await renderPage(page({ kind: "manual" }));
    const shown = await loadedFrame();

    rerender(page({ kind: "manual", page: CHAPTER }));
    await waitFor(() =>
      expect(shown.contentDocument?.getElementById("web")).toBeTruthy(),
    );
    await settle();

    expect(click(shown, "web")).toBe(false);
    expect(openedInBrowser()).toEqual(["https://github.com/cjpais/Handy"]);
  });

  test("the app's own page and scripts open nothing", async () => {
    const shown = await showLinks();

    expect(click(shown, "app")).toBe(false);
    expect(click(shown, "script")).toBe(false);

    expect(openedInBrowser()).toEqual([]);
    expect(shown.contentWindow?.location.pathname).toBe("/manual/");
  });

  test("a page of the manual that asks for another window opens in the frame", async () => {
    const shown = await showLinks();

    expect(click(shown, "blank")).toBe(false);
    await waitFor(() => expect(manualRequests()).toContain("/manual/de/"));

    expect(openedInBrowser()).toEqual([]);
  });

  test("a page of the manual can't take over the settings window", async () => {
    const shown = await showLinks();

    expect(click(shown, "top")).toBe(false);
    await waitFor(() => expect(manualRequests()).toContain("/manual/fr/"));

    expect(window.location.pathname).toBe("/");
  });
});

describe("the manual and Application Theme", () => {
  const choose = async (current: string, option: string) => {
    await press(screen.getByRole("button", { name: current }));
    await press(screen.getByRole("button", { name: option }));
  };

  const themeOf = (shown: HTMLIFrameElement) => ({
    theme: shown.contentDocument?.documentElement.dataset.theme,
    menu: (
      shown.contentDocument?.querySelector(
        "starlight-theme-select select",
      ) as HTMLSelectElement | null
    )?.value,
  });

  test("the page on screen follows a change of the theme", async () => {
    await renderPage(
      <>
        <Window start={{ kind: "manual" }} />
        <ThemeSelector />
      </>,
      { settings: settingsWith({ app: { theme: "dark" } }) },
    );
    const shown = await loadedFrame();

    await choose("Dark", "Light");
    expect(themeOf(shown)).toEqual({ theme: "light", menu: "light" });

    await choose("Light", "Dark");
    expect(themeOf(shown)).toEqual({ theme: "dark", menu: "dark" });

    // System: the system's appearance, light in the test's DOM.
    await choose("Dark", "System");
    expect(themeOf(shown)).toEqual({ theme: "light", menu: "auto" });
  });
});

describe("a build without the page", () => {
  test("a chapter the build lacks gives way to its language's home page", async () => {
    await switchTestLanguage("de");
    serveManualPages({
      "/manual/de/how-it-works/start-and-end-shortcuts/": APP_INDEX,
    });

    await renderWindow({ kind: "manual", page: CHAPTER });

    await waitFor(() =>
      expect(frame()?.getAttribute("src")).toBe("/manual/de/"),
    );
    await loadedFrame();
    expect(manualRequests()).toEqual([
      "/manual/de/how-it-works/start-and-end-shortcuts/",
      "/manual/de/",
    ]);
  });

  test("a language the build lacks gives way to the English home page", async () => {
    await switchTestLanguage("fr");
    serveManualPages({ "/manual/fr/": APP_INDEX });

    await renderWindow({ kind: "manual" });

    await waitFor(() => expect(frame()?.getAttribute("src")).toBe("/manual/"));
  });

  test("a build without the manual says so", async () => {
    withoutManual();

    await renderWindow({ kind: "manual" });

    await waitFor(() =>
      expect(visibleText()).toContain("This build of DanyWhere has no manual."),
    );
    const shown = frame();
    expect(shown === null || !isVisible(shown)).toBe(true);
    expect(rawTextIn()).toEqual([]);
  });
});
