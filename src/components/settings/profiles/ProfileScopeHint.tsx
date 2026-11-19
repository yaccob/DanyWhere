import React from "react";
import { useTranslation } from "react-i18next";
import { useProfiles } from "@/hooks/useProfiles";

interface ProfileScopeHintProps {
  /** Which page's settings it speaks of. */
  page: "models" | "postProcessing";
  className?: string;
}

/** DanyWhere: a quiet line saying where a change on this page goes while a
 * profile is in use: its own value, or else the defaults. Nothing while the
 * defaults are in use, where every change goes into them. */
export const ProfileScopeHint: React.FC<ProfileScopeHintProps> = ({
  page,
  className = "",
}) => {
  const { t } = useTranslation();
  const { profiles, active, displayName } = useProfiles();
  if (profiles.length === 0 || !active) return null;

  return (
    <p className={`text-xs text-ink-3 ${className}`}>
      {t(`settings.profiles.scope.${page}`, { name: displayName(active) })}
    </p>
  );
};
