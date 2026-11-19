import React from "react";
import { useTranslation } from "react-i18next";
import { useProfiles } from "@/hooks/useProfiles";
import type { ProfilePage } from "@/lib/navigation";

interface ProfilePagePlaceholderProps {
  profileId: string;
  page: ProfilePage;
}

/** DanyWhere: PLACEHOLDER for a profile's pages under the profile settings,
 * until the profile view is built (strand E2 of qa 2026-11-19_1910; sketches
 * 2 and 4 of qa 2026-11-19_1711). E2 replaces this component with the same
 * props: the page "Profile" (name, applications, the profile's own values),
 * and the other pages like the defaults', each value coupled or its own. */
export const ProfilePagePlaceholder: React.FC<ProfilePagePlaceholderProps> = ({
  profileId,
  page,
}) => {
  const { t } = useTranslation();
  const { profiles, displayName } = useProfiles();
  const profile = profiles.find((p) => p.id === profileId);
  if (!profile) return null;

  return (
    <div className="max-w-3xl w-full mx-auto">
      <div
        data-placeholder="profile-page"
        data-page={page}
        className="rounded-lg border border-dashed border-line-strong px-4 py-6 text-sm text-ink-2"
      >
        {t("settings.profiles.pagePlaceholder", { name: displayName(profile) })}
      </div>
    </div>
  );
};
