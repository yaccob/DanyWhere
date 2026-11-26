import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { PostProcessSetup } from "../../settings/post-processing/setup/PostProcessSetup";
import { usePostProcessSetupStore } from "@/stores/postProcessSetupStore";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";

interface PostProcessSetupScreenProps {
  progress: { index: number; count: number };
  onBack: () => void;
  /** "Later", or "Continue" once a way is set up. */
  onLeave: () => void;
  /** Debug preview: nothing is set up or saved. */
  preview?: boolean;
}

/**
 * DanyWhere: onboarding's screen "Improve your texts" (sidecar qa
 * 2026-11-26_1840, section 5: at the first installation, as the last
 * screen, with "Later"). The same questions and ways as the page "Post
 * Process"; "Later" is the most prominent button until a way works, then
 * "Continue".
 */
export const PostProcessSetupScreen: React.FC<PostProcessSetupScreenProps> = ({
  progress,
  onBack,
  onLeave,
  preview = false,
}) => {
  const { t } = useTranslation();
  const ready = usePostProcessSetupStore(
    (s) => s.status?.readiness === "ready",
  );
  const putOff = usePostProcessSetupStore((s) => s.putOff);

  const leave = async () => {
    if (!ready) await putOff(preview);
    onLeave();
  };

  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={ready ? "" : t("onboarding.postProcessSetup.laterHint")}
      primary={
        <Button variant="primary" size="lg" onClick={() => void leave()}>
          {t(
            ready
              ? "onboarding.shell.continue"
              : "onboarding.postProcessSetup.later",
          )}
        </Button>
      }
    >
      <ScreenMain
        layout="top"
        header={
          <ScreenHeader
            title={t("onboarding.postProcessSetup.title")}
            sub={t("onboarding.postProcessSetup.sub")}
          />
        }
      >
        <div className="min-h-0 w-full flex-1 overflow-y-auto rounded-panel border border-line-strong">
          <PostProcessSetup place="onboarding" inert={preview} />
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};
