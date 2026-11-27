import React from "react";
import { useTranslation } from "react-i18next";
import { LogLevelSelector } from "./LogLevelSelector";
import { LiveLogViewer } from "./LiveLogViewer";
import { PasteDelay } from "./PasteDelay";
import { HoldThreshold } from "./HoldThreshold";
import { ReliablePasteToggle } from "./ReliablePaste";
import { RecordingBuffer } from "./RecordingBuffer";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { WhatsNewPreview } from "./WhatsNewPreview";
import { ThemeFile } from "./ThemeFile";
import { SoundFile } from "./SoundFile";
import { KeyboardDiagnostic } from "./KeyboardDiagnostic";
import { DebugRun, OnboardingPreview } from "./OnboardingPreview";
import type { OnboardingStep } from "../../onboarding/onboardingStore";
// DanyWhere: the experimental settings, formerly behind a switch in Advanced.
// Those of a dictation moved to the defaults' Advanced page, the keyboard
// implementation and the update checks to the application settings.
import { AccelerationSelector } from "../AccelerationSelector";
import { LazyStreamClose } from "../LazyStreamClose";
import { VadBackendSelector } from "../VadBackendSelector";
// DanyWhere: an application setting, formerly next to the post-processing's
// time limit on Advanced.
import { ModelListTimeout } from "../PostProcessTimeouts";

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
        <SoundFile descriptionMode="tooltip" grouped={true} />
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
        <ModelListTimeout descriptionMode="tooltip" grouped={true} />
        <KeyboardDiagnostic />
        <LiveLogViewer descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      {/* DanyWhere: the experimental settings live here, always shown,
          instead of behind the "Experimental Features" switch in Advanced. */}
      <SettingsGroup title={t("settings.advanced.groups.experimental")}>
        <AccelerationSelector descriptionMode="tooltip" grouped={true} />
        <LazyStreamClose descriptionMode="tooltip" grouped={true} />
        <VadBackendSelector descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
