import React from "react";
import { useTranslation } from "react-i18next";
import { useProfiles } from "@/hooks/useProfiles";
import { DEFAULT_PROFILE_ID } from "@/lib/utils/profiles";
import { Dropdown } from "../../ui/Dropdown";
import { SendContextCompact } from "../dictation-context/SendContextSwitch";
import { ProfileAutoByAppCompact } from "./ProfileAutoByApp";

/** DanyWhere: what is in use, at the top of the sidebar on every page: the
 * defaults first, then the profiles (P2). The choice shows once there is a
 * profile to choose. */
export const ProfileSelector: React.FC = () => {
  const { t } = useTranslation();
  const { profiles, activeId, busy, defaultName, displayName, switchTo } =
    useProfiles();

  return (
    <div className="w-full pb-3">
      {profiles.length > 0 && (
        <>
          <p className="px-1 pb-1 text-xs font-medium text-ink-3 uppercase tracking-wide">
            {t("sidebar.profile")}
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
            disabled={busy}
            className="w-full"
          />
        </>
      )}
      <ProfileAutoByAppCompact />
      <SendContextCompact />
    </div>
  );
};
