import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface DictationKeyInsertsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * DanyWhere: without Accessibility, the dictation shortcut's second press
 * ends the dictation and inserts it once it's ready, through a macOS service
 * that carries the dictation shortcut as its own.
 */
export const DictationKeyInserts: React.FC<DictationKeyInsertsProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("dictation_key_inserts") ?? true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("dictation_key_inserts", enabled)}
        isUpdating={isUpdating("dictation_key_inserts")}
        label={t("settings.general.dictationKeyInserts.label")}
        description={t("settings.general.dictationKeyInserts.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });
