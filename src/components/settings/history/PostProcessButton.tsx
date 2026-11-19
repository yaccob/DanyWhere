// DanyWhere: History's ✨ button. A click post-processes the current version,
// the one the entry shows; the result goes on top as a new version and every
// version stays, so nothing asks first (user, 2026-11-19, H1 and H5: the
// menu's "replace" and "redo from the previous version" went). A menu that
// applies one prompt of the shared library by name (H6) comes with that
// library.
import React from "react";
import { useTranslation } from "react-i18next";
import { WandSparkles } from "lucide-react";
import type { PostProcessAvailability } from "./postProcessEntry";

const KEYS = "settings.history.versions";

interface PostProcessButtonProps {
  /** Whether post-processing can run with the current settings; when it
   * can't, the button says why. */
  availability: PostProcessAvailability;
  /** Post-processes the current version. */
  onRun: () => void;
  /** While the user edits the text: nothing may change the stack. */
  disabled?: boolean;
}

export const PostProcessButton: React.FC<PostProcessButtonProps> = ({
  availability,
  onRun,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const ready = availability === "ready";
  const usable = ready && !disabled;

  const title = ready
    ? `${t(`${KEYS}.postProcessFurther`)}\n${t(`${KEYS}.postProcessFurtherHint`)}`
    : availability === "off"
      ? t("settings.history.postProcessOff")
      : t("settings.history.postProcessNotSetUp");

  return (
    <button
      type="button"
      onClick={usable ? onRun : undefined}
      aria-disabled={!usable || undefined}
      title={title}
      className={`p-1.5 rounded-md flex items-center justify-center transition-colors ${
        usable
          ? "cursor-pointer text-ink-3 hover:text-logo-primary"
          : "cursor-not-allowed text-text/20"
      }`}
    >
      <WandSparkles width={16} height={16} />
    </button>
  );
};
