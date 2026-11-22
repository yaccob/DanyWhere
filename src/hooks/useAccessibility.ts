import { runsWithoutAccessibility } from "@/lib/accessibility";
import { useOsType } from "./useOsType";
import { useSettings } from "./useSettings";

/** What a settings page needs to know about Accessibility. */
export interface AccessibilityView {
  /** The mode without Accessibility is on (macOS, chosen in onboarding). */
  runsWithoutAccessibility: boolean;
}

/**
 * DanyWhere: Accessibility for the settings pages, from the loaded settings
 * and the system, through `lib/accessibility` (the one place that decides
 * it).
 */
export const useAccessibility = (): AccessibilityView => {
  const os = useOsType();
  const { getSetting } = useSettings();
  return {
    runsWithoutAccessibility: runsWithoutAccessibility(os, {
      accessibility_skipped: getSetting("accessibility_skipped"),
    }),
  };
};
