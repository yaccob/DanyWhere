// DanyWhere: the About page credits Handy under its own name. DanyWhere
// grew out of Handy, and the row says so and thanks Handy's makers; the
// branding that renames "Handy" to "DanyWhere" everywhere else must leave
// this row alone, and its button opens Handy's repository.
import { describe, expect, test } from "bun:test";
import { fireEvent, screen } from "@testing-library/react";
import React from "react";
import { AboutSettings } from "./AboutSettings";
import { UPSTREAM_URL } from "@/lib/constants/branding";
import { switchTestLanguage } from "@/test/i18n";
import { press, renderPage, settle } from "@/test/render";
import { ipcCalls } from "@/test/tauri-mock";
import { rawTextIn, visibleText } from "@/test/visible";

/** The addresses the default browser was asked to open. */
const openedInBrowser = (): string[] =>
  ipcCalls()
    .filter((call) => call.cmd === "plugin:opener|open_url")
    .map((call) => (call.args as { url: string }).url);

/** The row whose title is `title`. */
const row = (title: string): HTMLElement => {
  const heading = screen.getByRole("heading", { name: title });
  const container = heading.closest("div.flex.items-center.justify-between");
  if (!(container instanceof HTMLElement)) throw new Error(`no row ${title}`);
  return container;
};

/** The text of the row's info tooltip, after pressing its icon. */
const tooltipOf = async (title: string): Promise<string> => {
  const info = row(title).querySelector("button[aria-label]");
  if (!info) throw new Error(`no info icon in ${title}`);
  fireEvent.pointerDown(info, { pointerType: "mouse" });
  await settle();
  return screen.getByRole("tooltip").textContent ?? "";
};

describe("About: where DanyWhere comes from", () => {
  test("the row says DanyWhere grew out of Handy, not out of DanyWhere", async () => {
    await renderPage(<AboutSettings />);

    expect(visibleText()).toContain("Grown out of Handy");
    expect(visibleText()).not.toContain("Grown out of DanyWhere");
    expect(visibleText()).not.toContain("Based on");
    expect(rawTextIn()).toEqual([]);
  });

  test("its description thanks Handy's makers", async () => {
    await renderPage(<AboutSettings />);

    expect(await tooltipOf("Grown out of Handy")).toBe(
      "DanyWhere grew out of Handy by CJ Pais and contributors. Thanks to them for the groundwork it still stands on.",
    );
  });

  test("its button opens Handy's repository", async () => {
    await renderPage(<AboutSettings />);

    await press(
      screen.getByRole("button", { name: "Visit Handy" }) as HTMLElement,
    );

    expect(openedInBrowser()).toEqual([UPSTREAM_URL]);
  });

  test("in German too it names Handy", async () => {
    await switchTestLanguage("de");
    await renderPage(<AboutSettings />);

    expect(visibleText()).toContain("Aus Handy entstanden");
    expect(await tooltipOf("Aus Handy entstanden")).toBe(
      "DanyWhere ist aus Handy von CJ Pais und Mitwirkenden entstanden. Danke für das Fundament, auf dem es bis heute steht.",
    );
    expect(screen.getByRole("button", { name: "Handy besuchen" })).toBeTruthy();
  });
});
