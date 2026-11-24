import { commands, type Theme } from "@/bindings";

/**
 * Appearance theme handling.
 *
 * Handy already ships a full light palette and a full dark palette (see
 * `App.css`). This module lets the user pick which one is used instead of
 * always following the OS:
 *  - `system` removes the override so the `prefers-color-scheme` media query
 *    governs (the historical behaviour).
 *  - `light` / `dark` set `data-theme` on the document root, whose
 *    higher-specificity CSS selectors win over the media query.
 *
 * The choice is persisted in `AppSettings` (source of truth) and mirrored to
 * localStorage so it can be applied synchronously on boot, before React mounts,
 * avoiding a flash of the wrong palette.
 */

export const THEME_STORAGE_KEY = "handy.theme";

/**
 * DanyWhere: where the user manual (Astro Starlight, in a frame on its page
 * of the settings window, `ManualPage`) reads its theme on every page it
 * loads: "light", "dark", or empty for the system's appearance. The manual
 * is served from the app's own origin, so it shares this window's
 * localStorage; writing it here, at start and on every change, makes the
 * manual follow Application Theme. The page on screen read it before, so a
 * change reaches it through `applyManualTheme`.
 */
export const MANUAL_THEME_STORAGE_KEY = "starlight-theme";

/** Starlight's stored theme for `theme`: "light", "dark", or empty for the
 * system's appearance. */
const manualThemeValue = (theme: Theme): string =>
  theme === "system" ? "" : theme;

export const THEME_OPTIONS: Theme[] = ["system", "light", "dark"];

const isTheme = (value: unknown): value is Theme =>
  value === "system" || value === "light" || value === "dark";

/** Apply a theme to the document root and remember it for the next launch
 * and for the manual. */
export const applyTheme = (theme: Theme): void => {
  const root = document.documentElement;
  if (theme === "system") {
    delete root.dataset.theme;
  } else {
    root.dataset.theme = theme;
  }
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
    localStorage.setItem(MANUAL_THEME_STORAGE_KEY, manualThemeValue(theme));
  } catch {
    // localStorage may be unavailable (e.g. private mode); the setting still
    // persists in AppSettings, so this only costs a one-frame flash on boot.
  }
};

/**
 * DanyWhere: brings the manual's page `doc` on screen into `theme` at once,
 * as Starlight's own theme menu does: the `data-theme` its styles follow
 * (for System the appearance the page's window has) and the menu's choice.
 */
export const applyManualTheme = (doc: Document, theme: Theme): void => {
  const stored = manualThemeValue(theme);
  const light =
    doc.defaultView?.matchMedia?.("(prefers-color-scheme: light)").matches ===
    true;
  doc.documentElement.dataset.theme = stored || (light ? "light" : "dark");
  doc
    .querySelectorAll<HTMLSelectElement>("starlight-theme-select select")
    .forEach((select) => {
      select.value = stored || "auto";
    });
};

/** Read the last-applied theme for synchronous boot-time application. */
export const getStoredTheme = (): Theme => {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (isTheme(stored)) return stored;
  } catch {
    // ignore
  }
  return "system";
};

/** Apply the persisted theme from AppSettings (the source of truth). */
export const syncThemeFromSettings = async (): Promise<void> => {
  try {
    const result = await commands.getAppSettings();
    if (result.status === "ok") {
      applyTheme(result.data.theme ?? "system");
    }
  } catch (e) {
    console.warn("Failed to sync theme from settings:", e);
  }
};
