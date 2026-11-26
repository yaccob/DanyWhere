import React from "react";
import { useTranslation } from "react-i18next";
import { commands, type EndKey } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
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
 * shortcut it starts a recording that keeps running, else nothing; with the
 * keys of an instruction shortcut, that one switches to the instruction
 * during a recording. No hold shortcut says whether it post-processes since
 * Phase 2 ("Schnittstelle Phase 2"), so every one is always taken.
 */
export const HoldKeysSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const startKeys = getSetting("start_keys") ?? [];
  const holdKeys = getSetting("hold_keys") ?? [];
  const instructionKeys = getSetting("instruction_keys") ?? [];
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
            ...(hasCombination(instructionKeys, hold.shortcut)
              ? [t("settings.holdKeys.alsoInstruction")]
              : []),
          ]}
          disabled={busy}
          onChange={save}
        />
      </ScopedSetting>
    </SettingsGroup>
  );
};
