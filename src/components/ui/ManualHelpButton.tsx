import React from "react";
import { useTranslation } from "react-i18next";
import { CircleHelp } from "lucide-react";
import { useOpenManual } from "@/hooks/useOpenManual";

interface ManualHelpButtonProps {
  /** The manual's page, by its slug (`MANUAL_PAGES`). */
  page: string;
  /** The page's name, for the button's name and tooltip. */
  chapter: string;
}

/**
 * DanyWhere: a "?" that shows a chapter of the user manual on the settings
 * window's manual page, e.g. next to a settings group's title
 * (`SettingsGroup`'s `titleAction`). The same circled question mark as the
 * Models legend. Outside the settings window, where nothing could show the
 * manual, there is none.
 */
export const ManualHelpButton: React.FC<ManualHelpButtonProps> = ({
  page,
  chapter,
}) => {
  const { t } = useTranslation();
  const openManual = useOpenManual();
  if (!openManual) return null;
  const label = t("manual.openChapter", { chapter });
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={() => openManual(page)}
      className="grid size-[18px] shrink-0 cursor-pointer place-items-center rounded-full text-ink-3 transition-colors hover:bg-wash hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
    >
      <CircleHelp className="size-[14px]" aria-hidden="true" />
    </button>
  );
};
