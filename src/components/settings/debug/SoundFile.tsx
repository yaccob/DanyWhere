import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../../ui/Button";
import { SettingContainer } from "../../ui/SettingContainer";

interface SoundFileProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * DanyWhere: opens the user's sound file (sounds.toml next to the settings,
 * the file and pitch of each feedback sound per sound theme and event) in the
 * default app, writing it first if it is missing.
 */
export const SoundFile: React.FC<SoundFileProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [opening, setOpening] = useState(false);

  const open = async () => {
    setOpening(true);
    try {
      const result = await commands.openSoundFile();
      if (result.status === "error") {
        console.error("Failed to open the sound file:", result.error);
      }
    } catch (error) {
      console.error("Failed to open the sound file:", error);
    } finally {
      setOpening(false);
    }
  };

  return (
    <SettingContainer
      title={t("settings.debug.soundFile.title")}
      description={t("settings.debug.soundFile.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Button variant="secondary" size="md" onClick={open} disabled={opening}>
        {t("settings.debug.soundFile.button")}
      </Button>
    </SettingContainer>
  );
};
