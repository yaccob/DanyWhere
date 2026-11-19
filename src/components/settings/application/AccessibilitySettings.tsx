import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { useSettings } from "../../../hooks/useSettings";
import { KeyboardImplementationSelector } from "../debug/KeyboardImplementationSelector";
import { DictationKeyInserts } from "../DictationKeyInserts";
import { DictationServiceWhenIdleSetting } from "../DictationServiceWhenIdle";

/** DanyWhere: Application settings › Accessibility (P11): how shortcuts are
 * read, and, in the mode without Accessibility (chosen in onboarding), what
 * the dictation shortcut's second press does. The keyboard implementation was
 * on Debug, the two switches of the mode in General. */
export const AccessibilitySettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const isMacOS = type() === "macos";
  const accessibilitySkipped = getSetting("accessibility_skipped") === true;
  const dictationKeyInserts = getSetting("dictation_key_inserts") ?? true;
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("onboarding.permissions.accessibility.name")}>
        <KeyboardImplementationSelector
          descriptionMode="tooltip"
          grouped={true}
        />
        {/* DanyWhere: the dictation shortcut's second press inserts too. */}
        {isMacOS && accessibilitySkipped && (
          <DictationKeyInserts descriptionMode="tooltip" grouped={true} />
        )}
        {isMacOS && accessibilitySkipped && dictationKeyInserts && (
          <DictationServiceWhenIdleSetting
            descriptionMode="tooltip"
            grouped={true}
          />
        )}
      </SettingsGroup>
    </div>
  );
};
