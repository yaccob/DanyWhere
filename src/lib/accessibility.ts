import {
  checkAccessibilityPermission,
  requestAccessibilityPermission,
} from "tauri-plugin-macos-permissions-api";
import { commands, type AppSettings } from "@/bindings";

/**
 * DanyWhere: the one place in the frontend that asks about Accessibility,
 * as `accessibility.rs` is in the backend. Two things decide it:
 *
 * - the user's choice in onboarding, "Continue without Accessibility",
 *   stored as `accessibility_skipped` and never reset; it switches on the
 *   mode without the permission (macOS only);
 * - macOS's answer, through the permissions plugin.
 *
 * `App.tsx`, onboarding and the settings pages ask here (or through
 * `useAccessibility`) instead of reading the setting or the plugin.
 */

/** The part of the settings that holds the user's choice. */
export type StoredChoice = Pick<AppSettings, "accessibility_skipped">;

/** The user chose to continue without Accessibility. */
export const choseToGoWithout = (
  settings: StoredChoice | null | undefined,
): boolean => settings?.accessibility_skipped === true;

/** The mode without Accessibility is on: chosen, and on macOS, the only
 * system that has it. */
export const runsWithoutAccessibility = (
  os: string,
  settings: StoredChoice | null | undefined,
): boolean => os === "macos" && choseToGoWithout(settings);

/** The user's choice as the backend has it now. */
export const readChoseToGoWithout = async (): Promise<boolean> => {
  const result = await commands.getAppSettings();
  return result.status === "ok" && choseToGoWithout(result.data);
};

/** macOS's answer: whether DanyWhere has Accessibility. Never shows a
 * prompt. */
export const macosGrantsAccessibility = (): Promise<boolean> =>
  checkAccessibilityPermission();

/** Asks macOS for Accessibility: its prompt, or nothing once it was
 * answered (then only System Settings can change it). */
export const askMacosForAccessibility = async (): Promise<void> => {
  await requestAccessibilityPermission();
};
