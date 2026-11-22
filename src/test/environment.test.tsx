// DanyWhere: the component test environment itself (AGENTS.md, "Component
// tests"): a real component renders with the real English texts, a click
// reaches the replaced `bindings.ts`, a placeholder the code doesn't fill
// shows, and nothing of one test leaks into the next.
import { describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import { type } from "@tauri-apps/plugin-os";
import i18n from "i18next";
import { commands } from "@/bindings";
import { DoneSound } from "@/components/settings/DoneSound";
import { useSettingsStore } from "@/stores/settingsStore";
import { callsOf, emitEvent, err, mockCommand } from "./bindings-mock";
import { settingsWith } from "./fixtures";
import { press, renderPage } from "./render";
import { setOsType } from "./tauri-mock";
import { rawTextIn, visibleText } from "./visible";

describe("component test environment", () => {
  test("a component renders with the real texts and its click reaches the replaced command", async () => {
    await renderPage(<DoneSound />, {
      settings: settingsWith({ defaults: { done_sound: false } }),
    });

    expect(screen.getByText("Sound When Done")).toBeTruthy();
    const toggle = screen.getByRole("checkbox", { name: "Sound When Done" });
    await press(toggle);

    expect(callsOf("changeDoneSoundSetting")).toEqual([[true]]);
  });

  test("a test answers a command its own way", async () => {
    mockCommand("changeDoneSoundSetting", async () => err("busy"));
    expect(await commands.changeDoneSoundSetting(true)).toEqual({
      status: "error",
      error: "busy",
    });
  });

  test("every other command answers by its return type", async () => {
    expect(await commands.listApplications()).toEqual([]);
    expect(await commands.getAvailableMicrophones()).toEqual({
      status: "ok",
      data: [],
    });
    expect(await commands.isUpdateChecksLocked()).toBe(false);
  });

  test("answers and calls are gone in the next test", () => {
    expect(callsOf("changeDoneSoundSetting")).toEqual([]);
    expect(useSettingsStore.getState().settings).toBeNull();
  });

  test("the texts are DanyWhere's, and a missing placeholder shows", () => {
    expect(i18n.t("settings.about.title")).not.toContain("Handy");
    const unfilled = i18n.t("settings.profileView.value.items", { count: 3 });
    expect(unfilled).toContain("{{number}}");
    document.body.innerHTML = `<p>${unfilled}</p>`;
    expect(rawTextIn()).toEqual([`unfilled placeholder: ${unfilled}`]);
  });

  test("hidden markup is not visible text", () => {
    document.body.innerHTML =
      '<div><span>shown</span><span class="hidden">gone</span><span aria-hidden="true">icon</span></div>';
    expect(visibleText()).toBe("shown");
  });

  test("the system is macOS unless a test says otherwise", () => {
    expect(type()).toBe("macos");
    setOsType("linux");
    expect(type()).toBe("linux");
  });

  test("events reach what listens to them", async () => {
    const seen: unknown[] = [];
    const { events } = await import("@/bindings");
    await events.noticesChanged.listen((event) => seen.push(event.payload));
    emitEvent("noticesChanged", { notices: [] } as never);
    expect(seen).toEqual([{ notices: [] }]);
  });
});
