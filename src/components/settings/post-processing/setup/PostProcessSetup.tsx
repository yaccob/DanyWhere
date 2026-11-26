import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import type { AppSettings, PostProcessSetupSettings } from "@/bindings";
import { Button } from "../../../ui/Button";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { providerName } from "@/lib/displayNames";
import { guideFor } from "@/lib/postProcessSetup/guides";
import {
  suggest,
  type Machine,
  type SetupAnswers,
} from "@/lib/postProcessSetup/questions";
import { useSettingsStore } from "@/stores/settingsStore";
import { usePostProcessSetupStore } from "@/stores/postProcessSetupStore";
import { CardLine } from "./SetupCard";
import { ConnectedCard } from "./ConnectedCard";
import { SetupQuestions } from "./SetupQuestions";
import { SetupSuggestion } from "./SetupSuggestion";

const KEY = "settings.postProcessing.setup";

const EMPTY: SetupAnswers = {
  texts_may_go: null,
  subscription: null,
  next_step: null,
};

export interface PostProcessSetupProps {
  /** On the page "Post Process", or on onboarding's screen, whose footer
   * has "Later". */
  place: "settings" | "onboarding";
  /** Onboarding's debug preview: answers stay here, nothing is set up. */
  inert?: boolean;
  /** Settings: whether today's provider settings show below. */
  providersOpen?: boolean;
  onToggleProviders?: () => void;
  onOpenProviders?: () => void;
}

/**
 * DanyWhere: setting up post-processing with questions anyone can answer
 * (sidecar qa 2026-11-26_1840 and 2026-11-26_2250, 4.5). Until a way works,
 * the questions come one after the other with the suggestion under them;
 * once one works, the card "Connected"; "Later" puts it off to one line.
 */
export const PostProcessSetup: React.FC<PostProcessSetupProps> = ({
  place,
  inert = false,
  providersOpen = false,
  onToggleProviders,
  onOpenProviders,
}) => {
  const { t } = useTranslation();
  const os = useOsType();
  const { settings } = useSettings();
  const kept = useSettingsStore(
    (s) =>
      (s.settings as (AppSettings & PostProcessSetupSettings) | null)
        ?.post_process_setup,
  );
  const settingsLoaded = useSettingsStore((s) => s.settings !== null);
  const store = usePostProcessSetupStore();
  const { hydrate, listen, loadStatus } = store;

  useEffect(() => {
    void listen();
    void loadStatus();
  }, [listen, loadStatus]);

  useEffect(() => {
    if (settingsLoaded) hydrate(kept ?? {});
  }, [settingsLoaded, kept, hydrate]);

  const status = store.status;
  const machine: Machine = {
    // Until the backend says how much memory there is, nothing is greyed
    // out by mistake.
    memoryBytes: status?.memory_bytes ?? Number.MAX_SAFE_INTEGER,
    platform: os === "windows" || os === "linux" ? os : "macos",
  };
  const answers = store.answers ?? EMPTY;
  const providers = settings?.post_process_providers ?? [];
  const labelOf = (id: string) => {
    const provider = providers.find((p) => p.id === id);
    return provider ? providerName(provider, t) : (guideFor(id)?.service ?? id);
  };
  const onboarding = place === "onboarding";

  const ready = status?.readiness === "ready";
  const showCard = ready && !store.editing && store.signIn === null;
  const putOff =
    !onboarding && store.skipped && !store.editing && store.signIn === null;

  if (showCard && status) {
    return (
      <div className="px-4 py-3">
        <ConnectedCard
          status={status}
          providerLabel={labelOf(status.provider_id)}
          wantsCredit={answers.next_step === "credit"}
          inert={inert}
          advanced={
            onToggleProviders
              ? { open: providersOpen, onToggle: onToggleProviders }
              : undefined
          }
        />
      </div>
    );
  }

  const knowMyWay = !onboarding && onToggleProviders && (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-ink-3">
      <span>{t(`${KEY}.knowMyWay`)}</span>
      <Button
        variant="ghost"
        size="sm"
        aria-expanded={providersOpen}
        onClick={onToggleProviders}
      >
        {t(`${KEY}.allProviders`)}
      </Button>
      {ready && store.editing ? (
        <Button variant="ghost" size="sm" onClick={store.closeQuestions}>
          {t(`${KEY}.keepCurrent`)}
        </Button>
      ) : (
        !putOff && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void store.putOff(inert)}
          >
            {t(`${KEY}.later`)}
          </Button>
        )
      )}
    </div>
  );

  if (putOff) {
    return (
      <div className="space-y-2 px-4 py-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <CardLine>{t(`${KEY}.notSetUp`)}</CardLine>
          <Button variant="primary" size="sm" onClick={store.openQuestions}>
            {t(`${KEY}.setUp`)}
          </Button>
        </div>
        {knowMyWay}
      </div>
    );
  }

  const way = suggest(answers, machine);
  return (
    <div className="space-y-4 px-4 py-3 text-start">
      {!onboarding && <CardLine tone="quiet">{t(`${KEY}.intro`)}</CardLine>}
      <SetupQuestions
        answers={answers}
        machine={machine}
        onAnswer={(next) => store.answer(next, machine, inert)}
      />
      {way && (
        <SetupSuggestion
          way={way}
          machine={machine}
          labelOf={labelOf}
          prominent={!onboarding}
          inert={inert}
          onOpenProviders={onboarding ? undefined : onOpenProviders}
        />
      )}
      {knowMyWay}
    </div>
  );
};
