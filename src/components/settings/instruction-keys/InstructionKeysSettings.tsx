import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { commands, type EndKey, type Readiness } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { hasCombination } from "@/lib/startKeys";
import { postProcessReadiness } from "@/lib/postProcessSetup/readiness";
import { usePostProcessSetupStore } from "@/stores/postProcessSetupStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { ScopedSetting } from "../profiles/scope/ScopedSetting";
import { useSettingsScope } from "../profiles/scope/SettingsScope";
import { EndKeysEditor } from "../end-keys/EndKeysEditor";
import { useKeyListSave } from "../end-keys/useKeyListSave";

/** What the card says above the list of a scope: its post-processing mode
 * is "never", so the instruction shortcuts aren't taken; or post-processing
 * isn't ready there, so they are taken, but a dictation with an
 * instruction is inserted as heard and its notice offers the setup (qa
 * 2026-11-26_1810, "Regeln"; 2026-11-26_2250: taken before the setup too). */
export type InstructionKeysNote = "never" | "notSetUp" | null;

/** The note of a scope with this mode and readiness, if it has one. Ready
 * is what the backend checks before it sends: a provider, its model and
 * its key or sign-in (qa 2026-11-26_2250, D, N10). */
export const instructionKeysNote = (
  settings: { post_process_mode?: string | null } | null | undefined,
  readiness: Readiness,
): InstructionKeysNote => {
  if (settings?.post_process_mode === "never") return "never";
  return readiness === "ready" ? null : "notSetUp";
};

/** The readiness of the page's scope: the backend's for the defaults
 * (`get_post_process_setup_status`, asked again whenever the settings
 * change), a profile's from its values. */
const useScopeReadiness = (): Readiness => {
  const scope = useSettingsScope();
  const { settings } = useSettings();
  const stored = useSettingsStore((s) => s.settings);
  const status = usePostProcessSetupStore((s) => s.status);
  const loadStatus = usePostProcessSetupStore((s) => s.loadStatus);
  const defaults = scope === null || scope.kind === "defaults";
  useEffect(() => {
    if (defaults) void loadStatus();
  }, [defaults, loadStatus, stored]);
  return defaults && status ? status.readiness : postProcessReadiness(settings);
};

/**
 * DanyWhere: the card Instruction Shortcuts on General, between Hold
 * Shortcuts and End Shortcuts (qa 2026-11-26_1841, decision: a free list
 * like the start, hold and end shortcuts; 2026-11-26_1810, "Schnittstelle
 * Phase 2"; 2026-11-26_1720, "Einstellungen auf ‚Allgemein‘"). During a
 * recording an instruction shortcut switches from the text to a spoken
 * instruction for post-processing; tapped, an end shortcut then sends the
 * dictation, held, releasing it sends it with this entry's values. Such a
 * dictation is always post-processed. The end shortcuts' editor, with names
 * that follow the keys ("⇥ held", since the name stands for the end when it
 * is released); the list may be empty. Under each entry: taken only during
 * a recording, and with a start or hold shortcut's keys, what those do
 * outside one. Above the list, when the scope's mode is "never", that the
 * shortcuts aren't taken, and without a provider, what happens to a
 * dictation with an instruction until one is set up. General hides the card on
 * Linux, where instruction shortcuts aren't taken (`profile_sync.rs`).
 */
export const InstructionKeysSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, getSetting } = useSettings();
  const startKeys = getSetting("start_keys") ?? [];
  const holdKeys = getSetting("hold_keys") ?? [];
  const instructionKeys = getSetting("instruction_keys") ?? [];
  const note = instructionKeysNote(settings, useScopeReadiness());
  const { busy, save } = useKeyListSave<EndKey>(
    "instruction_keys",
    commands.changeInstructionKeysSetting,
    "settings.instructionKeys.errors.save",
  );

  return (
    <SettingsGroup
      title={t("settings.instructionKeys.title")}
      description={t("settings.instructionKeys.description")}
    >
      {note === "never" && (
        <p className="px-4 py-2 text-xs text-ink-3">
          {t("settings.instructionKeys.notTakenNever", {
            mode: t("settings.postProcessing.mode.options.never"),
            setting: t("settings.postProcessing.mode.title"),
          })}
        </p>
      )}
      {note === "notSetUp" && (
        <p className="px-4 py-2 text-xs text-ink-3">
          {t("settings.instructionKeys.notSetUp")}
        </p>
      )}
      <ScopedSetting field="instruction_keys">
        <EndKeysEditor
          kind="instruction"
          value={instructionKeys}
          removable={true}
          hintsOf={(instruction) => [
            t("settings.instructionKeys.onlyWhileRecording"),
            ...(hasCombination(startKeys, instruction.shortcut) ||
            hasCombination(holdKeys, instruction.shortcut)
              ? [t("settings.instructionKeys.sameAsStartOrHold")]
              : []),
          ]}
          disabled={busy}
          onChange={save}
        />
      </ScopedSetting>
    </SettingsGroup>
  );
};
