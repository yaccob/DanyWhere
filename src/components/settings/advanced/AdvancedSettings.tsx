import React from "react";
import { useTranslation } from "react-i18next";
import { ShowOverlay } from "../ShowOverlay";
import { ModelUnloadTimeoutSetting } from "../ModelUnloadTimeout";
import { CustomWords, ProfileWords } from "../CustomWords";
import { ActiveWordList, WordLists } from "../WordLists";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { StartHidden } from "../StartHidden";
import { AutostartToggle } from "../AutostartToggle";
import { ShowTrayIcon } from "../ShowTrayIcon";
import { PasteMethodSetting } from "../PasteMethod";
import { TypingToolSetting } from "../TypingTool";
import { ClipboardHandlingSetting } from "../ClipboardHandling";
import { ClipboardNoticeSetting } from "../ClipboardNotice";
import { PostProcessingToggle } from "../PostProcessingToggle";
import { PostProcessTimeouts } from "../PostProcessTimeouts";
import { RecordingStorageLimitSetting } from "../RecordingStorageLimit";
import { VoiceActivityDetection } from "../VoiceActivityDetection";
import { LongFormChunking } from "../LongFormChunking";
import { FillerWordRemoval } from "../FillerWordRemoval";

export const AdvancedSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.advanced.groups.app")}>
        <StartHidden descriptionMode="tooltip" grouped={true} />
        <AutostartToggle descriptionMode="tooltip" grouped={true} />
        <ShowTrayIcon descriptionMode="tooltip" grouped={true} />
        <ShowOverlay descriptionMode="tooltip" grouped={true} />
        <ModelUnloadTimeoutSetting descriptionMode="tooltip" grouped={true} />
        {/* DanyWhere: no "Experimental Features" switch; its settings are
            always on the Debug page (DebugSettings). */}
      </SettingsGroup>

      <SettingsGroup title={t("settings.advanced.groups.output")}>
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

      <SettingsGroup title={t("settings.advanced.groups.history")}>
        <RecordingStorageLimitSetting
          descriptionMode="tooltip"
          grouped={true}
        />
      </SettingsGroup>
    </div>
  );
};
