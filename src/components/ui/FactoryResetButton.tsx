// DanyWhere: Handy's reset buttons, which set a setting to its factory value
// (qa 2026-11-22_1632, point 4). They say so ("Werkseinstellung", the user's
// word) under the defaults and in the app's own settings, and are hidden in
// a profile, where the state line's Reset goes back to the defaults.
import React from "react";
import { useTranslation } from "react-i18next";
import { showsFactoryReset } from "@/lib/displayNames";
import { useSettingsScope } from "@/components/settings/profiles/scope/SettingsScope";
import { ResetButton } from "./ResetButton";

interface FactoryResetButtonProps {
  /** The setting's name, for the label "Reset … to the factory setting". */
  setting: string;
  onClick: () => void;
  disabled?: boolean;
  className?: string;
}

export const FactoryResetButton: React.FC<FactoryResetButtonProps> = ({
  setting,
  onClick,
  disabled,
  className,
}) => {
  const { t } = useTranslation();
  const scope = useSettingsScope();
  if (!showsFactoryReset(scope)) return null;
  return (
    <ResetButton
      onClick={onClick}
      disabled={disabled}
      className={className}
      ariaLabel={t("common.resetSetting", { setting })}
    />
  );
};
