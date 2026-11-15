import React from "react";
import { useTranslation } from "react-i18next";
import { useProfiles } from "@/hooks/useProfiles";

interface ProfileScopeHintProps {
  /** Which page's settings it speaks of. */
  page: "models" | "postProcessing";
  className?: string;
}

/** DanyWhere: a quiet line saying that a page's settings belong to the
 * active profile. Only once there is more than one profile to tell apart. */
export const ProfileScopeHint: React.FC<ProfileScopeHintProps> = ({
  page,
  className = "",
}) => {
  const { t } = useTranslation();
  const { profiles, active, displayName } = useProfiles();
  if (profiles.length < 2 || !active) return null;

  return (
    <p className={`text-xs text-ink-3 ${className}`}>
      {t(`settings.profiles.scope.${page}`, { name: displayName(active) })}
    </p>
  );
};
