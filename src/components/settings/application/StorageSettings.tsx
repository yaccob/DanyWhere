import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { RecordingStorageLimitSetting } from "../RecordingStorageLimit";
import { ModelUnloadTimeoutSetting } from "../ModelUnloadTimeout";
import { AppDataDirectory } from "../AppDataDirectory";
import { LogDirectory } from "../debug";

/** DanyWhere: Application settings › Storage (P11): what the app keeps on
 * disk and in memory. The recordings' limit was in Advanced › History, the
 * model unloading in Advanced › App, the folders on About.
 *
 * TODO(P11): list the downloaded models with their size and a delete here.
 * Today they are deleted from their row in the Models page's table ("Yours"),
 * which also chooses the model; that table has no list of its own to move. */
export const StorageSettings: React.FC = () => {
  const { t } = useTranslation();
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.advanced.groups.history")}>
        <RecordingStorageLimitSetting
          descriptionMode="tooltip"
          grouped={true}
        />
      </SettingsGroup>
      <SettingsGroup title={t("sidebar.models")}>
        <ModelUnloadTimeoutSetting descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
      <SettingsGroup>
        <AppDataDirectory descriptionMode="tooltip" grouped={true} />
        <LogDirectory grouped={true} />
      </SettingsGroup>
    </div>
  );
};
