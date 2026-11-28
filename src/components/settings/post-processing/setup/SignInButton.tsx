import React from "react";
import { useTranslation } from "react-i18next";
import type { SignInFailure, SignInIntent, SignInProvider } from "@/bindings";
import type { Translate } from "@/lib/displayNames";
import { Button } from "../../../ui/Button";
import { Spinner } from "../../../ui/Spinner";
import {
  signInActive,
  usePostProcessSetupStore,
} from "@/stores/postProcessSetupStore";
import { CardLine } from "./SetupCard";

const KEY = "settings.postProcessing.setup.signIn";

/** The name a failure's text gives the sign-in. */
const PROVIDER_NAME: Record<SignInProvider, string> = {
  chatgpt: "ChatGPT",
  openrouter: "OpenRouter",
};

/** The text of each code the backend ends a failed sign-in with; a new
 * code doesn't compile until it has one. "cancelled" ends the sign-in
 * without a failure (`signInFinished` in the store). */
const FAILURE_TEXTS: Record<Exclude<SignInFailure, "cancelled">, string> = {
  timed_out: "timedOut",
  denied: "denied",
  not_eligible: "notEligible",
  no_free_model: "noFreeModel",
  unreachable: "unreachable",
  refused: "refused",
  failed: "didNotWork",
};

const isFailureCode = (error: string): error is keyof typeof FAILURE_TEXTS =>
  Object.prototype.hasOwnProperty.call(FAILURE_TEXTS, error);

/** What a failed sign-in's `error` means to the user: a code from the
 * finished event, or what a start that failed reports. */
const failureText = (
  t: Translate,
  provider: SignInProvider,
  error: string | null,
) => {
  if (error && isFailureCode(error)) {
    return t(`${KEY}.${FAILURE_TEXTS[error]}`, {
      provider: PROVIDER_NAME[provider],
    });
  }
  if (!error || error === "cancelled") return t(`${KEY}.didNotWork`);
  return t(`${KEY}.failed`, { error });
};

interface SignInButtonProps {
  provider: SignInProvider;
  /** The way of the setup the sign-in is for (`signInIntentOf`). */
  intent?: SignInIntent | null;
  /** The button's text: OpenAI's "Continue with ChatGPT", or "Connect
   * OpenRouter". */
  label: string;
  /** In onboarding "Later" leads, so the button doesn't look like the
   * next step. */
  prominent: boolean;
  inert?: boolean;
}

/**
 * DanyWhere: signing in through the browser (sidecar qa 2026-11-26_1955,
 * section 3): the button, then waiting for the browser's callback with
 * "Open the page again" (a new OpenRouter user lands in its welcome, not
 * back at the authorization) and "Cancel"; a failed sign-in says why and
 * can start again.
 */
export const SignInButton: React.FC<SignInButtonProps> = ({
  provider,
  intent = null,
  label,
  prominent,
  inert = false,
}) => {
  const { t } = useTranslation();
  const signIn = usePostProcessSetupStore((s) => s.signIn);
  const startSignIn = usePostProcessSetupStore((s) => s.startSignIn);
  const reopenSignIn = usePostProcessSetupStore((s) => s.reopenSignIn);
  const cancelSignIn = usePostProcessSetupStore((s) => s.cancelSignIn);
  const mine = signIn?.provider === provider ? signIn : null;
  // Only one sign-in runs at a time; a failed one holds up nothing.
  const otherRunning = signInActive(signIn) && !mine;

  if (mine && mine.phase !== "failed") {
    return (
      <div className="space-y-2" aria-live="polite">
        <p className="m-0 inline-flex items-center gap-2 text-sm font-medium text-text">
          <Spinner className="shrink-0" />
          {t(`${KEY}.waiting`)}
        </p>
        <CardLine tone="quiet">{t(`${KEY}.waitingHint`)}</CardLine>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="secondary"
            size="sm"
            disabled={mine.phase !== "waiting"}
            onClick={() => void reopenSignIn()}
          >
            {t(`${KEY}.reopen`)}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => void cancelSignIn()}>
            {t(`${KEY}.cancel`)}
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {mine && (
        <CardLine tone="error">{failureText(t, provider, mine.error)}</CardLine>
      )}
      <Button
        variant={prominent ? "primary" : "outline"}
        size="md"
        disabled={inert || otherRunning}
        onClick={() => void startSignIn(provider, intent)}
      >
        {mine ? t(`${KEY}.tryAgain`) : label}
      </Button>
    </div>
  );
};
