import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface DoneSoundProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  disabled?: boolean;
}

/**
 * DanyWhere: a sound once a dictation is completely done, post-processing
 * included, whether it was pasted or waits to be pasted or inserted. Plays
 * only while Audio Feedback is on (`delivery::sound_after_dictation`).
 */
export const DoneSound: React.FC<DoneSoundProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false, disabled = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("done_sound") ?? true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("done_sound", enabled)}
        isUpdating={isUpdating("done_sound")}
        disabled={disabled}
        label={t("settings.sound.doneSound.label")}
        description={t("settings.sound.doneSound.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
