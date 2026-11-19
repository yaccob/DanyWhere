import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { UpdateChecksToggle } from "../UpdateChecksToggle";
import { ShowWhatsNewOnUpdate } from "../ShowWhatsNewOnUpdate";

/** DanyWhere: Application settings › Updates (P11). The update checks were
 * on Debug, "What's New" on About. */
export const UpdatesSettings: React.FC = () => {
  const { t } = useTranslation();
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("sidebar.updates")}>
        <UpdateChecksToggle descriptionMode="tooltip" grouped={true} />
        <ShowWhatsNewOnUpdate descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
