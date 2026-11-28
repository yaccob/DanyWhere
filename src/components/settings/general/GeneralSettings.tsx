import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { MicrophoneSelector } from "../MicrophoneSelector";
import { ChannelSelector } from "../ChannelSelector";
import { ShortcutInput } from "../ShortcutInput";
import { EndKeysSettings } from "../end-keys/EndKeysSettings";
import { StartKeysSettings } from "../start-keys/StartKeysSettings";
import { HoldKeysSettings } from "../start-keys/HoldKeysSettings";
import { InstructionKeysSettings } from "../instruction-keys/InstructionKeysSettings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { OutputDeviceSelector } from "../OutputDeviceSelector";
import { AudioFeedback } from "../AudioFeedback";
import { DoneSound } from "../DoneSound";
import { useSettings } from "../../../hooks/useSettings";
import { VolumeSlider } from "../VolumeSlider";
import { MuteWhileRecording } from "../MuteWhileRecording";
import { ModelSettingsCard } from "./ModelSettingsCard";
import { PreferredLanguages } from "../PreferredLanguages";
import { ChineseScriptSetting } from "../ChineseScript";
import { isChinese } from "@/lib/utils/languageSuggestions";
// DanyWhere (strand E2): each profile value with its chain and the
// profiles that deviate from it.
import { ScopedSetting } from "../profiles/scope/ScopedSetting";

export const GeneralSettings: React.FC = () => {
  const { t } = useTranslation();
  const { audioFeedbackEnabled, getSetting } = useSettings();
  const isLinux = type() === "linux";
  // Only for Chinese speakers, or anyone who already picked a script.
  const speaksChinese =
    (getSetting("spoken_languages") ?? []).some(isChinese) ||
    (getSetting("chinese_script") ?? "as_transcribed") !== "as_transcribed";
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      {/* DanyWhere: four lists, start, hold, instruction and end shortcuts,
          follow each other, the other shortcuts come after them (user,
          2026-11-24; qa 2026-11-26_1713 and 2026-11-26_1841, decisions).
          What a combination does follows from the lists it stands in. Linux
          takes instruction shortcuts only on a start or hold shortcut's
          combination (user, 2026-11-28); the card says so there. */}
      <StartKeysSettings />
      <HoldKeysSettings />
      <InstructionKeysSettings />
      {/* DanyWhere: how a recording ends and what follows (end_keys.rs);
          Cancel, which ends without a text, closes the end shortcuts. */}
      <EndKeysSettings
        after={
          // Cancel shortcut remains hidden on Linux because of dynamic shortcut instability.
          !isLinux && (
            <ScopedSetting field="bindings" entry="cancel">
              <ShortcutInput shortcutId="cancel" grouped={true} />
            </ScopedSetting>
          )
        }
      />
      <SettingsGroup title={t("settings.general.otherShortcuts")}>
        {/* DanyWhere: the two switches of the mode without Accessibility
            are in Application settings › Accessibility. */}
        {/* DanyWhere: without Accessibility, on macOS, the paste shortcut is
            the "Insert Last Dictation" service's (insert_service.rs). */}
        <ScopedSetting field="bindings" entry="paste_last_transcript">
          <ShortcutInput shortcutId="paste_last_transcript" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="bindings" entry="copy_last_transcript">
          <ShortcutInput shortcutId="copy_last_transcript" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="bindings" entry="cycle_word_list">
          <ShortcutInput shortcutId="cycle_word_list" grouped={true} />
        </ScopedSetting>
        {/* DanyWhere: turns choosing the profile by application on and off. */}
        <ScopedSetting field="bindings" entry="toggle_profile_auto">
          <ShortcutInput shortcutId="toggle_profile_auto" grouped={true} />
        </ScopedSetting>
      </SettingsGroup>
      <SettingsGroup title={t("settings.languages.title")}>
        <ScopedSetting field="spoken_languages">
          <PreferredLanguages descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        {speaksChinese && (
          <ScopedSetting field="chinese_script">
            <ChineseScriptSetting descriptionMode="tooltip" grouped={true} />
          </ScopedSetting>
        )}
      </SettingsGroup>
      <ModelSettingsCard />
      <SettingsGroup title={t("settings.sound.title")}>
        <ScopedSetting field="selected_microphone">
          <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="selected_channel">
          <ChannelSelector descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="mute_while_recording">
          <MuteWhileRecording descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="audio_feedback">
          <AudioFeedback descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        {/* DanyWhere: a sound once a dictation is completely done. */}
        <ScopedSetting field="done_sound">
          <DoneSound
            descriptionMode="tooltip"
            grouped={true}
            disabled={!audioFeedbackEnabled}
          />
        </ScopedSetting>
        <ScopedSetting field="selected_output_device">
          <OutputDeviceSelector
            descriptionMode="tooltip"
            grouped={true}
            disabled={!audioFeedbackEnabled}
          />
        </ScopedSetting>
        <ScopedSetting field="audio_feedback_volume">
          <VolumeSlider disabled={!audioFeedbackEnabled} />
        </ScopedSetting>
      </SettingsGroup>
    </div>
  );
};
