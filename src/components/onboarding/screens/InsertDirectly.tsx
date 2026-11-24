import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { Spinner } from "../../ui/Spinner";
import { DoneMark } from "../../shared/DoneMark";
import { useSettings } from "@/hooks/useSettings";
import { useInputMethod } from "@/hooks/useInputMethod";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";

export interface InsertDirectlyProps {
  /** In the flow: its place. Standalone (the one-time guide): none. */
  progress?: { index: number; count: number };
  /** In the flow: back to the screen before. */
  onBack?: () => void;
  /** "Later", or "Continue" once the source is added. */
  onLeave: () => void;
  /** Debug preview: nothing is asked, set up or saved. */
  preview?: boolean;
}

/**
 * DanyWhere: after "Continue without Accessibility" (and once, at start, for
 * installations that chose it before the input method existed): adding
 * DanyWhere's own input source in System Settings, so end shortcuts insert a
 * dictation straight into the text field. The state follows System Settings
 * every second; leaving marks the guide as seen.
 */
export const InsertDirectly: React.FC<InsertDirectlyProps> = ({
  progress,
  onBack,
  onLeave,
  preview = false,
}) => {
  const { t } = useTranslation();
  const { updateSetting } = useSettings();
  const { status, name, settingUp, setUp } = useInputMethod({
    inert: preview,
  });
  // After the button: the state line waits for System Settings.
  const [asked, setAsked] = useState(false);
  const added = status?.added === true;

  const leave = () => {
    if (!preview) void updateSetting("input_method_guide_seen", true);
    onLeave();
  };

  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={added ? "" : t("onboarding.insertDirectly.laterHint")}
      primary={
        <Button
          variant={added ? "primary" : "outline"}
          size="lg"
          onClick={leave}
        >
          {t(
            added
              ? "onboarding.shell.continue"
              : "onboarding.insertDirectly.later",
          )}
        </Button>
      }
    >
      <ScreenMain
        header={
          <ScreenHeader
            title={t("onboarding.insertDirectly.title")}
            sub={t("settings.inputMethod.description")}
          />
        }
      >
        <div className="flex w-full flex-col gap-[12px] rounded-panel border border-line-strong px-[22px] py-[18px] text-start text-small leading-[1.45]">
          <p className="m-0 text-ink-2 text-pretty">
            {t("onboarding.insertDirectly.why")}
          </p>
          <p className="m-0 text-ink-2 text-pretty">
            {t("settings.inputMethod.howItWorks")}
          </p>
          {added ? (
            <p className="m-0 inline-flex animate-rise items-center gap-[8px] font-semibold text-moss">
              <DoneMark />
              {t("onboarding.insertDirectly.added")}
            </p>
          ) : (
            <>
              {name && (
                <p className="m-0 text-text text-pretty">
                  {t("settings.inputMethod.steps", { name })}
                </p>
              )}
              <div className="flex items-center justify-between gap-[18px]">
                <span className="inline-flex items-center gap-[6px] text-ink-3">
                  {asked && <Spinner className="shrink-0" />}
                  {t("onboarding.insertDirectly.notAdded")}
                </span>
                <Button
                  variant="primary"
                  size="md"
                  disabled={settingUp}
                  onClick={() => {
                    setAsked(true);
                    void setUp();
                  }}
                >
                  {t("settings.inputMethod.setUp")}
                </Button>
              </div>
            </>
          )}
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};
