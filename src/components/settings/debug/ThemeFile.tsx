import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../../ui/Button";
import { SettingContainer } from "../../ui/SettingContainer";

interface ThemeFileProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * DanyWhere: opens the user's theme file (theme.css next to the settings,
 * src-tauri/src/theme_file.rs) in the default app, writing it first if it
 * was deleted.
 */
export const ThemeFile: React.FC<ThemeFileProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [opening, setOpening] = useState(false);

  const open = async () => {
    setOpening(true);
    try {
      const result = await commands.openThemeFile();
      if (result.status === "error") {
        console.error("Failed to open the theme file:", result.error);
      }
    } catch (error) {
      console.error("Failed to open the theme file:", error);
    } finally {
      setOpening(false);
    }
  };

  return (
    <SettingContainer
      title={t("settings.debug.themeFile.title")}
      description={t("settings.debug.themeFile.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Button variant="secondary" size="md" onClick={open} disabled={opening}>
        {t("settings.debug.themeFile.button")}
      </Button>
    </SettingContainer>
  );
};
