import React from "react";
import { useTranslation } from "react-i18next";
import { commands, type EndKey } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useSettingsStore } from "@/stores/settingsStore";
import {
  postProcessingOn,
  postProcessingReachable,
  type ReachableFrom,
} from "@/lib/endKeys";
import { canRemoveStartOrHold, hasCombination } from "@/lib/startKeys";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { ScopedSetting } from "../profiles/scope/ScopedSetting";
import { EndKeysEditor } from "../end-keys/EndKeysEditor";
import { useKeyListSave } from "../end-keys/useKeyListSave";

/**
 * DanyWhere: the card Hold Shortcuts on General (qa 2026-11-26_1713,
 * decision; 2026-11-26_1810): pressing one starts a recording, releasing it
 * ends the recording with its own values, as an end shortcut would. The
 * end shortcuts' editor, with names that follow the keys ("⌃ Space held")
 * and under each entry what a short tap does: with the keys of a start
 * shortcut it starts a recording that keeps running, else nothing. One that
 * post-processes is taken only while some dictation can post-process
 * (question 4 a), which the settings in use decide, not the page's scope.
 */
export const HoldKeysSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const startKeys = getSetting("start_keys") ?? [];
  const holdKeys = getSetting("hold_keys") ?? [];
  const inUse = useSettingsStore((state) => state.settings);
  const unreachable = !postProcessingReachable((inUse ?? {}) as ReachableFrom);
  // The scope's own switch: the defaults' or the profile's value.
  const postProcessingOff = !postProcessingOn({
    post_process_enabled: getSetting("post_process_enabled"),
  });
  const { busy, save } = useKeyListSave<EndKey>(
    "hold_keys",
    commands.changeHoldKeysSetting,
    "settings.holdKeys.errors.save",
  );

  return (
    <SettingsGroup
      title={t("settings.holdKeys.title")}
      description={t("settings.holdKeys.description")}
    >
      <ScopedSetting field="hold_keys">
        <EndKeysEditor
          kind="hold"
          value={holdKeys}
          removable={canRemoveStartOrHold(startKeys, holdKeys)}
          hintsOf={(hold) => [
            hasCombination(startKeys, hold.shortcut)
              ? t("settings.holdKeys.alsoStart")
              : t("settings.holdKeys.holdOnly"),
          ]}
          disabled={busy}
          postProcessingOff={postProcessingOff}
          postProcessingUnreachable={unreachable}
          onChange={save}
        />
      </ScopedSetting>
    </SettingsGroup>
  );
};
