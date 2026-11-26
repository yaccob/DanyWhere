import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { commands, type EndKey } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { postProcessingOn } from "@/lib/endKeys";
import { hasCombination } from "@/lib/startKeys";
import { SettingsGroup } from "../../ui/SettingsGroup";
// DanyWhere: the "?" to the manual's chapter on start and end shortcuts.
import { ManualHelpButton } from "../../ui/ManualHelpButton";
import { MANUAL_PAGES } from "@/lib/manual";
import { EndKeysEditor } from "./EndKeysEditor";
// DanyWhere (strand E2): the end shortcuts of the page's scope, the defaults
// or a profile, each with its chain.
import { ScopedSetting } from "../profiles/scope/ScopedSetting";
import { useKeyListSave } from "./useKeyListSave";

interface EndKeysSettingsProps {
  /** Rows below the list, e.g. the cancel shortcut. */
  after?: React.ReactNode;
}

/**
 * DanyWhere: the end shortcuts on the shortcut settings (end_keys.rs, qa
 * 2026-11-22_2134): the list of the page's scope (the defaults or a
 * profile; outside one, the values in use). The page General puts Cancel
 * below it (`after`; user, 2026-11-24), so the card holds every way a
 * recording ends but releasing a hold shortcut, which has its own card
 * (qa 2026-11-26_1810). An end with the keys of a start or hold shortcut
 * says so: tapping those during a recording ends this way.
 */
export const EndKeysSettings: React.FC<EndKeysSettingsProps> = ({ after }) => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const endKeys = getSetting("end_keys") ?? [];
  const startKeys = getSetting("start_keys") ?? [];
  const holdKeys = getSetting("hold_keys") ?? [];
  // The scope's own switch: the defaults' or the profile's value.
  const postProcessingOff = !postProcessingOn({
    post_process_enabled: getSetting("post_process_enabled"),
  });
  const isLinux = type() === "linux";
  const { busy, save } = useKeyListSave<EndKey>(
    "end_keys",
    commands.changeEndKeysSetting,
    "settings.endKeys.errors.save",
  );

  const hintsOf = (end: EndKey): string[] => {
    if (hasCombination(startKeys, end.shortcut)) {
      return [t("settings.endKeys.sameAsStart")];
    }
    if (hasCombination(holdKeys, end.shortcut)) {
      return [t("settings.endKeys.sameAsHold")];
    }
    return [];
  };

  return (
    <SettingsGroup
      title={t("settings.endKeys.title")}
      description={t("settings.endKeys.description")}
      titleAction={
        <ManualHelpButton
          page={MANUAL_PAGES.startAndEndShortcuts}
          chapter={t("manual.chapters.startAndEndShortcuts")}
        />
      }
    >
      {isLinux && (
        <p className="px-4 py-2 text-xs text-ink-3">
          {t("settings.endKeys.linuxNoShortcuts")}
        </p>
      )}
      <ScopedSetting field="end_keys">
        <EndKeysEditor
          value={endKeys}
          hintsOf={hintsOf}
          disabled={busy}
          postProcessingOff={postProcessingOff}
          onChange={save}
        />
      </ScopedSetting>
      {after}
    </SettingsGroup>
  );
};
