import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { useSettings } from "../../../hooks/useSettings";
import { useAccessibility } from "../../../hooks/useAccessibility";
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
  const { runsWithoutAccessibility } = useAccessibility();
  const dictationKeyInserts = getSetting("dictation_key_inserts") ?? true;
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("onboarding.permissions.accessibility.name")}>
        <KeyboardImplementationSelector
          descriptionMode="tooltip"
          grouped={true}
        />
        {/* DanyWhere: the dictation shortcut's second press inserts too. */}
        {runsWithoutAccessibility && (
          <DictationKeyInserts descriptionMode="tooltip" grouped={true} />
        )}
        {runsWithoutAccessibility && dictationKeyInserts && (
          <DictationServiceWhenIdleSetting
            descriptionMode="tooltip"
            grouped={true}
          />
        )}
      </SettingsGroup>
    </div>
  );
};
