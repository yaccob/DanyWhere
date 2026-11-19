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
        <ShowOverlay descriptionMode="tooltip" grouped={true} />
        <PasteMethodSetting descriptionMode="tooltip" grouped={true} />
        <TypingToolSetting descriptionMode="tooltip" grouped={true} />
        <ClipboardHandlingSetting descriptionMode="tooltip" grouped={true} />
        <ClipboardNoticeSetting descriptionMode="tooltip" grouped={true} />
        {/* DanyWhere: "Append" ("Auto Submit" and "Append Trailing Space"
            before it) belongs to each end shortcut now (end_keys.rs), on the
            shortcut settings. */}
      </SettingsGroup>

      <SettingsGroup title={t("settings.advanced.groups.transcription")}>
        <VoiceActivityDetection descriptionMode="tooltip" grouped={true} />
        <LongFormChunking descriptionMode="tooltip" grouped={true} />
        <FillerWordRemoval descriptionMode="tooltip" grouped={true} />
        {/* DanyWhere: post-processing is used daily, so its switch is not
            hidden behind the experimental features. */}
        <PostProcessingToggle descriptionMode="tooltip" grouped={true} />
        <PostProcessTimeouts descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      {/* DanyWhere: the word lists in their three tiers: for all profiles,
          the profile's own, and the profile's additional lists. */}
      <SettingsGroup
        title={t("settings.advanced.wordLists.groupTitle")}
        description={t("settings.advanced.wordLists.groupDescription")}
      >
        <CustomWords descriptionMode="tooltip" grouped />
        <ProfileWords descriptionMode="tooltip" grouped />
        <ActiveWordList descriptionMode="tooltip" grouped />
        <WordLists descriptionMode="tooltip" grouped />
      </SettingsGroup>

      {/* DanyWhere: experimental settings of a dictation, formerly on the
          Debug page and, like it, shown in debug mode only. The detection
          window belongs to the language detection. */}
      {debugMode && (
        <SettingsGroup title={t("settings.advanced.groups.experimental")}>
          <LanguageDetection descriptionMode="tooltip" grouped={true} />
          <LanguageDetectionWindow descriptionMode="tooltip" grouped={true} />
          <SoundPicker
            label={t("settings.debug.soundTheme.label")}
            description={t("settings.debug.soundTheme.description")}
          />
          <AlwaysOnMicrophone descriptionMode="tooltip" grouped={true} />
          <ClamshellMicrophoneSelector
            descriptionMode="tooltip"
            grouped={true}
          />
          <AirPodsMode descriptionMode="tooltip" grouped={true} />
        </SettingsGroup>
      )}
    </div>
  );
};
