import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { notify } from "@/stores/noticeStore";

/**
 * DanyWhere: the user manual's pages the app links to, by their slug.
 * `open_manual` (src-tauri/src/manual.rs) puts the language in front.
 */
export const MANUAL_PAGES = {
  startAndEndShortcuts: "how-it-works/start-and-end-shortcuts/",
} as const;

/**
 * DanyWhere: opens the user manual in its own window, at `page` or at its
 * home page; a window already open goes there. A failure is a click's
 * result, so it pops up as a toast.
 */
export const useOpenManual = () => {
  const { t } = useTranslation();
  return useCallback(
    async (page: string | null = null) => {
      const fail = (error: unknown) =>
        notify({ title: t("manual.errors.open", { error: String(error) }) });
      try {
        const result = await commands.openManual(page);
        if (result.status === "error") fail(result.error);
      } catch (error) {
        fail(error);
      }
    },
    [t],
  );
};
