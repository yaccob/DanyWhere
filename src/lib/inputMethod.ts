import { commands, type AppSettings, type InputMethodStatus } from "@/bindings";
import { runsWithoutAccessibility } from "./accessibility";

/**
 * DanyWhere: the frontend's questions about DanyWhere's own input method
 * (`src-tauri/src/input_method/`, `src-tauri/macos/input-method/
 * PROTOCOL.md`). Once the user has added its input source in System
 * Settings, dictations go straight into the text field, without the
 * clipboard. Only macOS builds made by the release and test-build scripts
 * carry it; everywhere else the backend answers "not available".
 */

/** The backend's answer without an input method. */
export const NOT_AVAILABLE: InputMethodStatus = {
  available: false,
  installed: false,
  added: false,
  connected: false,
};

/** The part of the settings the one-time guide reads. */
export type GuideSettings = Pick<
  AppSettings,
  "accessibility_skipped" | "input_method_guide_seen"
>;

/** The input method can be offered: on macOS, in a build that carries it. */
export const offersInputMethod = (
  os: string,
  status: InputMethodStatus | null | undefined,
): boolean => os === "macos" && status?.available === true;

/**
 * The one-time guide is due at this start: an installation that runs without
 * Accessibility, whose build carries the input method, hasn't added the
 * source and hasn't seen the guide (in onboarding or at an earlier start).
 */
export const inputMethodGuideDue = (
  os: string,
  settings: GuideSettings | null | undefined,
  status: InputMethodStatus | null | undefined,
): boolean =>
  offersInputMethod(os, status) &&
  runsWithoutAccessibility(os, settings) &&
  status?.added !== true &&
  settings?.input_method_guide_seen !== true;

/** The backend's answer; "not available" when it can't tell. */
export const readInputMethodStatus = async (): Promise<InputMethodStatus> => {
  try {
    return (await commands.getInputMethodStatus()) ?? NOT_AVAILABLE;
  } catch (error) {
    console.warn("Failed to read the input method's status:", error);
    return NOT_AVAILABLE;
  }
};
