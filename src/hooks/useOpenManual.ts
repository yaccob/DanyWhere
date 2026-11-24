import { createContext, useContext } from "react";

/** Shows the user manual at `page` (a page's slug, `MANUAL_PAGES` in
 * `src/lib/manual.ts`), or at its home page. */
export type OpenManual = (page?: string) => void;

const OpenManualContext = createContext<OpenManual | null>(null);

/**
 * DanyWhere: the settings window shows the user manual on a page of its own
 * (`ManualPage`); this lets a control deep inside a settings page, such as
 * the "?" next to End Shortcuts (`ManualHelpButton`), go there.
 * `SettingsPage` provides it.
 */
export const OpenManualProvider = OpenManualContext.Provider;

/** How to show the manual from here; none outside the settings window. */
export const useOpenManual = (): OpenManual | null =>
  useContext(OpenManualContext);
