import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { endKeyErrorKey } from "@/lib/endKeys";

/**
 * DanyWhere: a refusal of the backend for end shortcuts or the ends for
 * holding (end_keys.rs), as the notice says it: its own text for the codes
 * the backend gives, the shortcut a combination belongs to for a conflict,
 * else "Couldn't save the end shortcuts" with the reason.
 */
export const useDescribeEndKeyError = () => {
  const { t } = useTranslation();
  return useCallback(
    (error: string): string => {
      const key = endKeyErrorKey(error);
      if (key) return t(key);
      const conflict = /conflict:([a-z_]+)/.exec(error);
      if (conflict) {
        return t("settings.general.insertService.conflict", {
          name: t(
            `settings.general.shortcut.bindings.${conflict[1]}.name`,
            conflict[1],
          ),
        });
      }
      return t("settings.endKeys.errors.save", { error });
    },
    [t],
  );
};
