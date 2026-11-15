import React from "react";
import { useTranslation } from "react-i18next";
import { WordCorrectionThreshold } from "./WordCorrectionThreshold";
import { LogLevelSelector } from "./LogLevelSelector";
import { LiveLogViewer } from "./LiveLogViewer";
import { PasteDelay } from "./PasteDelay";
import { HoldThreshold } from "./HoldThreshold";
import { ReliablePasteToggle } from "./ReliablePaste";
import { RecordingBuffer } from "./RecordingBuffer";
import { LanguageDetectionWindow } from "./LanguageDetectionWindow";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { AlwaysOnMicrophone } from "../AlwaysOnMicrophone";
import { SoundPicker } from "../SoundPicker";
import { ClamshellMicrophoneSelector } from "../ClamshellMicrophoneSelector";
import { UpdateChecksToggle } from "../UpdateChecksToggle";
import { WhatsNewPreview } from "./WhatsNewPreview";
import { ThemeFile } from "./ThemeFile";
import { KeyboardDiagnostic } from "./KeyboardDiagnostic";
import { DebugRun, OnboardingPreview } from "./OnboardingPreview";
import type { OnboardingStep } from "../../onboarding/onboardingStore";
// DanyWhere: the experimental settings, formerly behind a switch in Advanced.
import { KeyboardImplementationSelector } from "./KeyboardImplementationSelector";
import { AccelerationSelector } from "../AccelerationSelector";
import { LazyStreamClose } from "../LazyStreamClose";
import { AirPodsMode } from "../AirPodsMode";
import { VadBackendSelector } from "../VadBackendSelector";
import { LanguageDetection } from "../LanguageDetection";

interface DebugSettingsProps {
  onPreviewOnboarding: (step: OnboardingStep) => void;
  onRunOnboarding: () => void;
  onRunModelMigration: () => void;
}

export const DebugSettings: React.FC<DebugSettingsProps> = ({
  onPreviewOnboarding,
  onRunOnboarding,
  onRunModelMigration,
}) => {
  const { t } = useTranslation();

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.debug.title")}>
        <LogLevelSelector grouped={true} />
        <WhatsNewPreview descriptionMode="tooltip" grouped={true} />
        <ThemeFile descriptionMode="tooltip" grouped={true} />
        <OnboardingPreview
          onPreview={onPreviewOnboarding}
          descriptionMode="tooltip"
          grouped={true}
        />
        <DebugRun
          run="onboardingRun"
          onRun={onRunOnboarding}
          descriptionMode="tooltip"
          grouped={true}
        />
        <DebugRun
          run="modelMigration"
          onRun={onRunModelMigration}
          descriptionMode="tooltip"
          grouped={true}
        />
        <UpdateChecksToggle descriptionMode="tooltip" grouped={true} />
        <SoundPicker
          label={t("settings.debug.soundTheme.label")}
          description={t("settings.debug.soundTheme.description")}
        />
        <WordCorrectionThreshold descriptionMode="tooltip" grouped={true} />
        <PasteDelay descriptionMode="tooltip" grouped={true} />
        <PasteDelay
          descriptionMode="tooltip"
          grouped={true}
          settingKey="paste_delay_after_ms"
          labelKey="settings.debug.pasteDelayAfter.title"
          descriptionKey="settings.debug.pasteDelayAfter.description"
        />
        <ReliablePasteToggle descriptionMode="tooltip" grouped={true} />
        <HoldThreshold descriptionMode="tooltip" grouped={true} />
        <RecordingBuffer descriptionMode="tooltip" grouped={true} />
        <LanguageDetectionWindow descriptionMode="tooltip" grouped={true} />
        <AlwaysOnMicrophone descriptionMode="tooltip" grouped={true} />
        <ClamshellMicrophoneSelector descriptionMode="tooltip" grouped={true} />
        <KeyboardDiagnostic />
        <LiveLogViewer descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      {/* DanyWhere: the experimental settings live here, always shown,
          instead of behind the "Experimental Features" switch in Advanced. */}
      <SettingsGroup title={t("settings.advanced.groups.experimental")}>
        <KeyboardImplementationSelector
          descriptionMode="tooltip"
          grouped={true}
        />
        <AccelerationSelector descriptionMode="tooltip" grouped={true} />
        <LazyStreamClose descriptionMode="tooltip" grouped={true} />
        <AirPodsMode descriptionMode="tooltip" grouped={true} />
        <VadBackendSelector descriptionMode="tooltip" grouped={true} />
        <LanguageDetection descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
