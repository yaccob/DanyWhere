// DanyWhere: the sidebar's entry for the user manual. It sits between
// History and About and opens the manual's home page in a window of its own
// (`open_manual`, src-tauri/src/manual.rs); the settings window stays on the
// page it shows.
import { describe, expect, test } from "bun:test";
import { screen, within } from "@testing-library/react";
import type { NavLocation } from "@/lib/navigation";
import { Sidebar } from "./Sidebar";
import { callsOf, err, mockCommand } from "@/test/bindings-mock";
import { press, raisedNotices, renderPage } from "@/test/render";
import { switchTestLanguage } from "@/test/i18n";

const navigated: NavLocation[] = [];

const renderSidebar = () => {
  navigated.length = 0;
  return renderPage(
    <Sidebar
      location={{ kind: "history" }}
      onNavigate={(next) => navigated.push(next)}
    />,
  );
};

/** The names of the sidebar's rows, top to bottom. */
const rowNames = (): string[] =>
  within(screen.getByRole("navigation"))
    .getAllByRole("button")
    .map((row) => row.textContent ?? "");

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

  test("opens the manual's home page and leaves the window where it is", async () => {
    await renderSidebar();

    await press(screen.getByRole("button", { name: "Manual" }));

    expect(callsOf("openManual")).toEqual([[null]]);
    expect(navigated).toEqual([]);
    expect(
      screen
        .getByRole("button", { name: "History" })
        .getAttribute("aria-current"),
    ).toBe("page");
  });

  test("is never the current page", async () => {
    await renderSidebar();

    await press(screen.getByRole("button", { name: "Manual" }));

    expect(
      screen
        .getByRole("button", { name: "Manual" })
        .hasAttribute("aria-current"),
    ).toBe(false);
  });

  test("reads Handbuch in German", async () => {
    await switchTestLanguage("de");
    await renderSidebar();

    await press(screen.getByRole("button", { name: "Handbuch" }));

    expect(callsOf("openManual")).toEqual([[null]]);
  });

  test("says so when the manual can't open", async () => {
    mockCommand("openManual", async () => err("no window"));
    await renderSidebar();

    await press(screen.getByRole("button", { name: "Manual" }));

    expect(raisedNotices()).toEqual(["Couldn't open the manual: no window"]);
  });
});
