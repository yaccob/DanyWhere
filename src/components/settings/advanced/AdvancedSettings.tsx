import React from "react";
import { useTranslation } from "react-i18next";
import { ShowOverlay } from "../ShowOverlay";
import { CustomWords, ProfileWords } from "../CustomWords";
import { ActiveWordList, WordLists } from "../WordLists";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { PasteMethodSetting } from "../PasteMethod";
import { TypingToolSetting } from "../TypingTool";
import { ClipboardHandlingSetting } from "../ClipboardHandling";
import { ClipboardNoticeSetting } from "../ClipboardNotice";
import { PostProcessingToggle } from "../PostProcessingToggle";
import { PostProcessTimeouts } from "../PostProcessTimeouts";
import { VoiceActivityDetection } from "../VoiceActivityDetection";
import { LongFormChunking } from "../LongFormChunking";
import { FillerWordRemoval } from "../FillerWordRemoval";
// DanyWhere: settings of a dictation that were on Debug (experimental).
import { useSettings } from "../../../hooks/useSettings";
import { LanguageDetection } from "../LanguageDetection";
import { LanguageDetectionWindow } from "../debug/LanguageDetectionWindow";
import { SoundPicker } from "../SoundPicker";
import { AlwaysOnMicrophone } from "../AlwaysOnMicrophone";
import { ClamshellMicrophoneSelector } from "../ClamshellMicrophoneSelector";
import { AirPodsMode } from "../AirPodsMode";
// DanyWhere (strand E2): each profile value with its chain and the
// profiles that deviate from it.
import { ScopedSetting } from "../profiles/scope/ScopedSetting";

export const AdvancedSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const debugMode = getSetting("debug_mode") ?? false;

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      {/* DanyWhere: the group "App" went to the application settings
          (start, tray, model unloading); the overlay concerns a dictation
          and stays, with the output. */}
      <SettingsGroup title={t("settings.advanced.groups.output")}>
        <ScopedSetting field={["overlay_style", "overlay_position"]}>
          <ShowOverlay descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field={["paste_method", "external_script_path"]}>
          <PasteMethodSetting descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="typing_tool">
          <TypingToolSetting descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="clipboard_handling">
          <ClipboardHandlingSetting descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="clipboard_notice_ms">
          <ClipboardNoticeSetting descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        {/* DanyWhere: "Append" ("Auto Submit" and "Append Trailing Space"
            before it) belongs to each end shortcut now (end_keys.rs), on the
            shortcut settings. */}
      </SettingsGroup>

      <SettingsGroup title={t("settings.advanced.groups.transcription")}>
        <ScopedSetting field="vad_enabled">
          <VoiceActivityDetection descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="long_form_chunking">
          <LongFormChunking descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting
          field={["filler_word_removal_enabled", "custom_filler_words"]}
        >
          <FillerWordRemoval descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        {/* DanyWhere: post-processing is used daily, so its switch is not
            hidden behind the experimental features. */}
        <ScopedSetting field="post_process_enabled">
          <PostProcessingToggle descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
        <ScopedSetting field="post_process_timeout_secs">
          <PostProcessTimeouts descriptionMode="tooltip" grouped={true} />
        </ScopedSetting>
      </SettingsGroup>

      {/* DanyWhere: the word lists in their three tiers: for all profiles,
          the profile's own, and the profile's additional lists. */}
      <SettingsGroup
        title={t("settings.advanced.wordLists.groupTitle")}
        description={t("settings.advanced.wordLists.groupDescription")}
      >
        <ScopedSetting field="custom_words">
          <CustomWords descriptionMode="tooltip" grouped />
        </ScopedSetting>
        <ScopedSetting field="profile_words" profileOnly>
          <ProfileWords descriptionMode="tooltip" grouped />
        </ScopedSetting>
        <ScopedSetting field="active_word_list_id">
          <ActiveWordList descriptionMode="tooltip" grouped />
        </ScopedSetting>
        <ScopedSetting field="word_lists">
          <WordLists descriptionMode="tooltip" grouped />
        </ScopedSetting>
      </SettingsGroup>

      {/* DanyWhere: experimental settings of a dictation, formerly on the
          Debug page and, like it, shown in debug mode only. The detection
          window belongs to the language detection. */}
      {debugMode && (
        <SettingsGroup title={t("settings.advanced.groups.experimental")}>
          <ScopedSetting field="language_detection">
            <LanguageDetection descriptionMode="tooltip" grouped={true} />
          </ScopedSetting>
          <ScopedSetting field="language_detection_window_secs">
            <LanguageDetectionWindow descriptionMode="tooltip" grouped={true} />
          </ScopedSetting>
          <ScopedSetting field="sound_theme">
            <SoundPicker
              label={t("settings.debug.soundTheme.label")}
              description={t("settings.debug.soundTheme.description")}
            />
          </ScopedSetting>
          <ScopedSetting field="always_on_microphone">
            <AlwaysOnMicrophone descriptionMode="tooltip" grouped={true} />
          </ScopedSetting>
          <ScopedSetting field="clamshell_microphone">
            <ClamshellMicrophoneSelector
              descriptionMode="tooltip"
              grouped={true}
            />
          </ScopedSetting>
          <ScopedSetting field="airpods_mode">
            <AirPodsMode descriptionMode="tooltip" grouped={true} />
          </ScopedSetting>
        </SettingsGroup>
      )}
    </div>
  );
};
