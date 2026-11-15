import React from "react";
import { useTranslation } from "react-i18next";
import { useProfiles } from "@/hooks/useProfiles";
import { ToggleSwitch } from "../../ui/ToggleSwitch";

/** DanyWhere: "Choose Profile by Application" under the profile list on the
 * page "Profiles" (`app_profiles.rs`); the tray and its shortcut switch the
 * same setting. Shown from two profiles on. */
export const ProfileAutoByApp: React.FC<{ grouped?: boolean }> = ({
  grouped = true,
}) => {
  const { t } = useTranslation();
  const { profiles, autoByApp, assigned, setAutoByApp, busy } = useProfiles();
  if (profiles.length < 2) return null;
  return (
    <ToggleSwitch
      checked={autoByApp}
      onChange={(enabled) => void setAutoByApp(enabled)}
      isUpdating={busy}
      label={t("settings.profiles.autoByApp.label")}
      description={
        assigned
          ? t("settings.profiles.autoByApp.description")
          : t("settings.profiles.autoByApp.noApps")
      }
      descriptionMode="tooltip"
      grouped={grouped}
    />
  );
};

/** DanyWhere: the same switch, small, under the profile selector in the
 * sidebar while applications are assigned. */
export const ProfileAutoByAppCompact: React.FC = () => {
  const { t } = useTranslation();
  const { profiles, autoByApp, assigned, setAutoByApp, busy } = useProfiles();
  if (profiles.length < 2 || !assigned) return null;
  return (
    <label
      className="flex items-center gap-2 px-1 pt-2 text-xs text-ink-2 cursor-pointer"
      title={t("settings.profiles.autoByApp.description")}
    >
      <input
        type="checkbox"
        checked={autoByApp}
        disabled={busy}
        onChange={(event) => void setAutoByApp(event.target.checked)}
        className="accent-logo-primary"
      />
      {t("settings.profiles.autoByApp.short")}
    </label>
  );
};
