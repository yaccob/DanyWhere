import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { shortcutRefusalText } from "@/lib/shortcutRefusal";

/**
 * DanyWhere: a refusal of the backend for end shortcuts or the ends for
 * holding (end_keys.rs), as the notice says it: its own text for the codes
 * the backend gives, the shortcut a combination belongs to for a conflict
 * (`shortcutRefusalText`), else "Couldn't save the end shortcuts" with the
 * reason.
 */
export const useDescribeEndKeyError = () => {
  const { t } = useTranslation();
  return useCallback(
    (error: string): string =>
      shortcutRefusalText(error, t) ??
      t("settings.endKeys.errors.save", { error }),
    [t],
  );
};
