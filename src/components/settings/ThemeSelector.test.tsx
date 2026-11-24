// DanyWhere: the manual's window follows the app's theme. The manual is
// served from the app's own origin, so it shares the main window's
// localStorage; Starlight reads its theme from `starlight-theme` there
// ("light", "dark", or empty for the system's) on every page it loads.
import { beforeEach, describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import { ThemeSelector } from "./ThemeSelector";
import { callsOf } from "@/test/bindings-mock";
import { settingsWith } from "@/test/fixtures";
import { press, renderPage } from "@/test/render";

const MANUAL_THEME = "starlight-theme";

const choose = async (current: string, option: string) => {
  await press(screen.getByRole("button", { name: current }));
  await press(screen.getByRole("button", { name: option }));
};

beforeEach(() => {
  localStorage.clear();
});

describe("Application Theme and the manual", () => {
  test("Dark makes the manual dark", async () => {
    await renderPage(<ThemeSelector />);

    await choose("System", "Dark");

    expect(callsOf("changeThemeSetting")).toEqual([["dark"]]);
    expect(localStorage.getItem(MANUAL_THEME)).toBe("dark");
  });

  test("Light makes the manual light", async () => {
    await renderPage(<ThemeSelector />);

    await choose("System", "Light");

    expect(localStorage.getItem(MANUAL_THEME)).toBe("light");
  });

  test("System leaves the manual to the system's appearance", async () => {
    localStorage.setItem(MANUAL_THEME, "dark");
    await renderPage(<ThemeSelector />, {
      settings: settingsWith({ app: { theme: "dark" } }),
    });

    await choose("Dark", "System");

    expect(callsOf("changeThemeSetting")).toEqual([["system"]]);
    expect(localStorage.getItem(MANUAL_THEME)).toBe("");
  });
});
