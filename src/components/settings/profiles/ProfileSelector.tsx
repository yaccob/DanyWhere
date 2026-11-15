import React from "react";
import { useTranslation } from "react-i18next";
import { useProfiles } from "@/hooks/useProfiles";
import { Dropdown } from "../../ui/Dropdown";
import { SendContextCompact } from "../dictation-context/SendContextSwitch";
import { ProfileAutoByAppCompact } from "./ProfileAutoByApp";

/** DanyWhere: the profile in use, at the top of the sidebar on every page.
 * Hidden until the settings carry profiles. */
export const ProfileSelector: React.FC = () => {
  const { t } = useTranslation();
  const { profiles, active, busy, displayName, switchTo } = useProfiles();
  if (profiles.length === 0) return null;

  return (
    <div className="w-full pb-3">
      <p className="px-1 pb-1 text-xs font-medium text-ink-3 uppercase tracking-wide">
        {t("sidebar.profile")}
      </p>
      <Dropdown
        compact
        selectedValue={active?.id ?? null}
        options={profiles.map((profile) => ({
          value: profile.id,
          label: displayName(profile),
        }))}
        onSelect={switchTo}
        disabled={busy}
        className="w-full"
      />
      <ProfileAutoByAppCompact />
      <SendContextCompact />
    </div>
  );
};
