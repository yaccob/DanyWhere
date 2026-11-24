// DanyWhere: the sidebar's entry for the user manual. It sits between
// History and About and, like them, shows a page of its own in the settings
// window: the manual (`ManualPage`), marked as the current page while shown.
import { describe, expect, test } from "bun:test";
import { screen, within } from "@testing-library/react";
import type { NavLocation } from "@/lib/navigation";
import { Sidebar } from "./Sidebar";
import { press, renderPage } from "@/test/render";
import { switchTestLanguage } from "@/test/i18n";

const CHAPTER = "how-it-works/start-and-end-shortcuts/";
const navigated: NavLocation[] = [];

const renderSidebar = (location: NavLocation = { kind: "history" }) => {
  navigated.length = 0;
  return renderPage(
    <Sidebar location={location} onNavigate={(next) => navigated.push(next)} />,
  );
};

/** The names of the sidebar's rows, top to bottom. */
const rowNames = (): string[] =>
  within(screen.getByRole("navigation"))
    .getAllByRole("button")
    .map((row) => row.textContent ?? "");

const isCurrent = (name: string): boolean =>
  screen.getByRole("button", { name }).getAttribute("aria-current") === "page";

describe("the sidebar's Manual entry", () => {
  test("sits between History and About", async () => {
    await renderSidebar();

    const rows = rowNames();
    const history = rows.indexOf("History");
    expect(history).toBeGreaterThanOrEqual(0);
    expect(rows.slice(history, history + 3)).toEqual([
      "History",
      "Manual",
      "About",
    ]);
  });

  test("shows the manual's page, at its home page", async () => {
    await renderSidebar();

    await press(screen.getByRole("button", { name: "Manual" }));

    expect(navigated).toEqual([{ kind: "manual" }]);
  });

  test("is the current page while the manual shows", async () => {
    await renderSidebar({ kind: "manual" });

    expect(isCurrent("Manual")).toBe(true);
    expect(isCurrent("History")).toBe(false);
    expect(isCurrent("About")).toBe(false);
  });

  test("is current at any chapter of the manual too", async () => {
    await renderSidebar({ kind: "manual", page: CHAPTER });

    expect(isCurrent("Manual")).toBe(true);
  });

  test("isn't current on another page", async () => {
    await renderSidebar({ kind: "history" });

    expect(isCurrent("Manual")).toBe(false);
  });

  test("reads Handbuch in German", async () => {
    await switchTestLanguage("de");
    await renderSidebar();

    await press(screen.getByRole("button", { name: "Handbuch" }));

    expect(navigated).toEqual([{ kind: "manual" }]);
  });
});
