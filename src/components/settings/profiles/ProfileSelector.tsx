import React from "react";
import { useTranslation } from "react-i18next";
import { useProfiles } from "@/hooks/useProfiles";
import { DEFAULT_PROFILE_ID } from "@/lib/utils/profiles";
import { Dropdown } from "../../ui/Dropdown";
import { ProfileAutoByAppCompact } from "./ProfileAutoByApp";

/** DanyWhere: the box "In Use" at the top of the sidebar on every page: what
 * dictations use, the defaults as the first entry, then the profiles, and
 * choosing the profile by application (qa 2026-11-19_1711 P2). Only for
 * using; editing goes through the navigation below. */
export const ProfileSelector: React.FC = () => {
  const { t } = useTranslation();
  const { profiles, activeId, busy, defaultName, displayName, switchTo } =
    useProfiles();

  return (
    <div className="w-full mb-3 rounded-lg border border-line p-2">
      <p className="px-1 pb-1 text-xs font-medium text-ink-3 uppercase tracking-wide">
        {t("sidebar.inUse")}
      </p>
      <Dropdown
        compact
        selectedValue={activeId}
        options={[
          { value: DEFAULT_PROFILE_ID, label: defaultName },
          ...profiles.map((profile) => ({
            value: profile.id,
            label: displayName(profile),
          })),
        ]}
        onSelect={switchTo}
        disabled={busy || profiles.length === 0}
        className="w-full"
      />
      <ProfileAutoByAppCompact />
    </div>
  );
};
