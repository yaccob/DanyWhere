import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { AutostartToggle } from "../AutostartToggle";
import { StartHidden } from "../StartHidden";
import { ShowTrayIcon } from "../ShowTrayIcon";
import { AppLanguageSelector } from "../AppLanguageSelector";
import { ThemeSelector } from "../ThemeSelector";

/** DanyWhere: Application settings › General (qa 2026-11-19_1711 P11): how
 * the app starts and looks. Formerly in Advanced › App and About. */
export const ApplicationGeneralSettings: React.FC = () => {
  const { t } = useTranslation();
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("sidebar.general")}>
        <AutostartToggle descriptionMode="tooltip" grouped={true} />
        <StartHidden descriptionMode="tooltip" grouped={true} />
        <ShowTrayIcon descriptionMode="tooltip" grouped={true} />
        <AppLanguageSelector descriptionMode="tooltip" grouped={true} />
        <ThemeSelector descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
