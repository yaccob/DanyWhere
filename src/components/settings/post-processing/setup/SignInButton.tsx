import React from "react";
import { useTranslation } from "react-i18next";
import type { SignInProvider } from "@/bindings";
import type { Translate } from "@/lib/displayNames";
import { Button } from "../../../ui/Button";
import { Spinner } from "../../../ui/Spinner";
import { usePostProcessSetupStore } from "@/stores/postProcessSetupStore";
import { CardLine } from "./SetupCard";

const KEY = "settings.postProcessing.setup.signIn";

/** What a failed sign-in's `error` means to the user. */
const failureText = (t: Translate, error: string | null) => {
  switch (error) {
    case "timed_out":
      return t(`${KEY}.timedOut`);
    case "denied":
      return t(`${KEY}.denied`);
    case "not_eligible":
      return t(`${KEY}.notEligible`);
    default:
      return t(`${KEY}.failed`, { error: error ?? "" });
  }
};

interface SignInButtonProps {
  provider: SignInProvider;
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
      {mine && <CardLine tone="error">{failureText(t, mine.error)}</CardLine>}
      <Button
        variant={prominent ? "primary" : "outline"}
        size="md"
        disabled={inert || (signIn !== null && !mine)}
        onClick={() => void startSignIn(provider)}
      >
        {mine ? t(`${KEY}.tryAgain`) : label}
      </Button>
    </div>
  );
};
