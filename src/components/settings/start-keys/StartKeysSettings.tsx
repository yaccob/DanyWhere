import React from "react";
import { useTranslation } from "react-i18next";
import { commands, type StartKey } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { canRemoveStartOrHold, hasCombination } from "@/lib/startKeys";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { ScopedSetting } from "../profiles/scope/ScopedSetting";
import { useKeyListSave } from "../end-keys/useKeyListSave";
import { StartKeysEditor } from "./StartKeysEditor";

/**
 * DanyWhere: the card Start Shortcuts on General (qa 2026-11-26_1713,
 * decision; 2026-11-26_1810): tapping one starts a recording that runs
 * until an end shortcut ends it. The list of the page's scope (the
 * defaults or a profile, entry by entry; outside one, the values in use).
 * An entry whose keys a hold shortcut has too says so: there the length of
 * the press tells tapping from holding. So does one whose keys an
 * instruction shortcut has: during a recording those switch to the
 * instruction (qa 2026-11-26_1810, "Schnittstelle Phase 2").
 */
export const StartKeysSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const startKeys = getSetting("start_keys") ?? [];
  const holdKeys = getSetting("hold_keys") ?? [];
  const instructionKeys = getSetting("instruction_keys") ?? [];
  const { busy, save } = useKeyListSave<StartKey>(
    "start_keys",
    commands.changeStartKeysSetting,
    "settings.startKeys.errors.save",
  );

  return (
    <SettingsGroup
      title={t("settings.general.startShortcuts")}
      description={t("settings.startKeys.description")}
    >
      <ScopedSetting field="start_keys">
        <StartKeysEditor
          value={startKeys}
          removable={canRemoveStartOrHold(startKeys, holdKeys)}
          hintOf={(start) => {
            const hints = [
              ...(hasCombination(holdKeys, start.shortcut)
                ? [t("settings.startKeys.alsoHold")]
                : []),
              ...(hasCombination(instructionKeys, start.shortcut)
                ? [t("settings.startKeys.alsoInstruction")]
                : []),
            ];
            return hints.length > 0 ? hints.join(" · ") : null;
          }}
          disabled={busy}
          onChange={save}
        />
      </ScopedSetting>
    </SettingsGroup>
  );
};
