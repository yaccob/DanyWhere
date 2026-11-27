import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
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
import {
  signInActive,
  signInIntentOf,
  usePostProcessSetupStore,
} from "@/stores/postProcessSetupStore";
import { CardLine } from "./SetupCard";
import { ConnectedCard } from "./ConnectedCard";
import { SetupQuestions } from "./SetupQuestions";
import { PendingSignIn, SetupSuggestion, signInOfWay } from "./SetupSuggestion";

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
  /** Opens today's provider settings, with `providerId` chosen there. */
  onOpenProviders?: (providerId?: string) => void;
}

/** How many setups on the page "Post Process" are mounted. */
let settingsPagesShown = 0;

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
  const kept = useSettingsStore((s) => s.settings?.post_process_setup);
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

  // Leaving the page closes questions opened again, as "Keep the current
  // setup" does, so the next visit shows the way in use (sidecar qa
  // 2026-11-26_2250, D, N15). Checked a turn later, after StrictMode's
  // second mount.
  useEffect(() => {
    if (place !== "settings") return;
    settingsPagesShown += 1;
    return () => {
      settingsPagesShown -= 1;
      setTimeout(() => {
        const setup = usePostProcessSetupStore.getState();
        if (settingsPagesShown === 0 && setup.editing) setup.closeQuestions();
      }, 0);
    };
  }, [place]);

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
  // A sign-in in progress keeps the questions; a failed one doesn't (D, H2).
  const signingIn = signInActive(store.signIn);
  const showCard = ready && !store.editing && !signingIn;
  const putOff = !onboarding && store.skipped && !store.editing && !signingIn;

  if (showCard && status) {
    return (
      <div className="px-4 py-3">
        <ConnectedCard
          status={status}
          providerLabel={labelOf(status.provider_id ?? "")}
          intent={signInIntentOf(answers)}
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
  // A sign-in the suggestion doesn't offer, e.g. from a notice's "Sign in
  // again", shows its waiting on its own (D, M6).
  const pending =
    store.signIn && signInOfWay(way) !== store.signIn.provider
      ? store.signIn
      : null;
  return (
    <div className="space-y-4 px-4 py-3 text-start">
      {/* Not a CardLine: its m-0 would cancel the gap of space-y-4 below
          it (Tailwind 4 puts space-y in :where()). */}
      {!onboarding && (
        <p className="text-sm text-pretty text-ink-3">{t(`${KEY}.intro`)}</p>
      )}
      {pending && <PendingSignIn signIn={pending} />}
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
