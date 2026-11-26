import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { shortcutRefusalText } from "@/lib/shortcutRefusal";
import { storeKeyRefLabel } from "./keyRefLabel";

/**
 * DanyWhere: a refusal of the backend for start, hold, instruction or end
 * shortcuts (qa 2026-11-26_1810, "Prüfung und Garantien", "Schnittstelle
 * Phase 2"), as the notice says it: its own text for the codes the backend
 * gives, the shortcut a combination belongs to for a conflict
 * (`shortcutRefusalText`, a start, hold, instruction or end shortcut by its
 * keys or name), else `saveKey` ("Couldn't save the end shortcuts", the
 * start, hold or instruction shortcuts) with the reason.
 */
export const useDescribeEndKeyError = (
  saveKey = "settings.endKeys.errors.save",
) => {
  const { t } = useTranslation();
  return useCallback(
    (error: string): string =>
      shortcutRefusalText(error, t, storeKeyRefLabel(t)) ??
      t(saveKey, { error }),
    [t, saveKey],
  );
};
