import React from "react";
import { useTranslation } from "react-i18next";
import { TextLink } from "../ui/TextLink";
import { DoneMark } from "../shared/DoneMark";
import type { Permissions } from "./usePermissions";

/**
 * DanyWhere: continue without Accessibility, e.g. on a managed Mac that
 * blocks it. Shortcuts then use the Tauri backend and transcripts are left on
 * the clipboard. Shown under the permission rows while Accessibility isn't
 * allowed; allowing it later still works.
 */
export const AccessibilitySkip: React.FC<{ permissions: Permissions }> = ({
  permissions,
}) => {
  const { t } = useTranslation();
  if (permissions.accessibilitySkipped)
    return (
      <p className="mt-[18px] mb-0 inline-flex items-center gap-[8px] text-small text-ink-2">
        <DoneMark />
        {t("onboarding.permissions.accessibility.skipped")}
      </p>
    );
  return (
    <div className="mt-[18px] text-small text-ink-3">
      <TextLink onClick={permissions.skipAccessibility}>
        {t("onboarding.permissions.accessibility.skip")}
      </TextLink>
      <p className="mt-[4px] mb-0 text-pretty">
        {t("onboarding.permissions.accessibility.skipHint")}
      </p>
    </div>
  );
};
