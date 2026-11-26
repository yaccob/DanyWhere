import React from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../../../ui/Button";
import type { Machine, Way } from "@/lib/postProcessSetup/questions";
import { memoryLabel } from "@/lib/postProcessSetup/labels";
import { usePostProcessSetupStore } from "@/stores/postProcessSetupStore";
import { CardLine, SetupCard } from "./SetupCard";
import { SignInButton } from "./SignInButton";
import { GuidedKey } from "./GuidedKey";

const KEY = "settings.postProcessing.setup";

interface SetupSuggestionProps {
  way: Way;
  machine: Machine;
  /** The name of a provider as the provider list has it. */
  labelOf: (providerId: string) => string;
  /** In onboarding "Later" leads, so the suggestion's button doesn't look
   * like the next step. */
  prominent: boolean;
  inert?: boolean;
  /** Today's provider settings opened, for a server of one's own. */
  onOpenProviders?: () => void;
}

/** "Set up a server of your own": the custom provider, whose address and
 * model are among today's settings. */
const OwnServer: React.FC<{
  prominent: boolean;
  inert: boolean;
  onOpenProviders?: () => void;
}> = ({ prominent, inert, onOpenProviders }) => {
  const { t } = useTranslation();
  const refresh = usePostProcessSetupStore((s) => s.refresh);
  const choose = async () => {
    await commands.applyPostProcessChoice({
      provider_id: "custom",
      model: null,
      api_key: null,
      effort: null,
    });
    await refresh();
    onOpenProviders?.();
  };
  return (
    <div className="space-y-1">
      <Button
        variant={prominent ? "primary" : "outline"}
        size="md"
        disabled={inert}
        onClick={() => void choose()}
      >
        {t(`${KEY}.local.ownServer`)}
      </Button>
      {onOpenProviders && (
        <CardLine tone="quiet">{t(`${KEY}.local.ownServerHint`)}</CardLine>
      )}
    </div>
  );
};

/**
 * DanyWhere: the way the answers lead to, with its button (sidecar qa
 * 2026-11-26_1840, section 4 "Wohin die Antworten führen", and section 6).
 * The sentences carry what the trials found (qa 2026-11-26_1955): sign up
 * at OpenRouter with Google or GitHub, its welcome asks for a payment
 * method that the free models don't need, then open the page again.
 */
export const SetupSuggestion: React.FC<SetupSuggestionProps> = ({
  way,
  machine,
  labelOf,
  prominent,
  inert = false,
  onOpenProviders,
}) => {
  const { t, i18n } = useTranslation();
  const memory = memoryLabel(machine.memoryBytes, i18n.language);
  const onMac = machine.platform === "macos";

  switch (way.kind) {
    case "openrouter_free":
    case "openrouter_credit": {
      const free = way.kind === "openrouter_free";
      return (
        <SetupCard
          title={t(
            free
              ? `${KEY}.openrouter.freeTitle`
              : `${KEY}.openrouter.creditTitle`,
          )}
        >
          <CardLine>
            {t(
              free
                ? `${KEY}.openrouter.freeBody`
                : `${KEY}.openrouter.creditBody`,
            )}
          </CardLine>
          {/* Without m-0, so the card's space-y-2 leaves a gap above the
              button (Tailwind 4 puts space-y in :where()). */}
          <ul className="list-disc space-y-1 ps-5 text-sm text-ink-3">
            <li className="text-pretty">{t(`${KEY}.openrouter.tipAccount`)}</li>
            {/* With credit no free model is used, not even before the
                top-up (sidecar qa 2026-11-26_2250, D, H1). */}
            <li className="text-pretty">
              {t(
                free
                  ? `${KEY}.openrouter.tipPayment`
                  : `${KEY}.openrouter.tipPaymentCredit`,
              )}
            </li>
          </ul>
          <SignInButton
            provider="openrouter"
            intent={free ? "free" : "credit"}
            label={t(`${KEY}.openrouter.connect`)}
            prominent={prominent}
            inert={inert}
          />
        </SetupCard>
      );
    }
    case "chatgpt_plan":
      return (
        <SetupCard title={t(`${KEY}.chatgpt.title`)}>
          <CardLine>{t(`${KEY}.chatgpt.body`)}</CardLine>
          {way.dataNotDescribed && (
            <CardLine tone="warning">
              {t(`${KEY}.chatgpt.dataNotDescribed`)}
            </CardLine>
          )}
          <SignInButton
            provider="chatgpt"
            label={t(`${KEY}.chatgpt.continue`)}
            prominent={prominent}
            inert={inert}
          />
          {/* OpenAI's notice before the first use of the plan. */}
          <CardLine tone="quiet">{t(`${KEY}.chatgpt.firstUse`)}</CardLine>
        </SetupCard>
      );
    case "guided_key":
      return (
        <GuidedKey
          provider={way.provider}
          providerLabel={labelOf(way.provider)}
          prominent={prominent}
          inert={inert}
        />
      );
    case "local_model":
      return (
        <SetupCard title={t(`${KEY}.local.laterTitle`)}>
          <CardLine>{t(`${KEY}.local.laterBody`)}</CardLine>
          <OwnServer
            prominent={prominent}
            inert={inert}
            onOpenProviders={onOpenProviders}
          />
        </SetupCard>
      );
    case "nowhere_unavailable":
      return (
        <SetupCard title={t(`${KEY}.local.unavailableTitle`)}>
          <CardLine>
            {t(
              onMac
                ? `${KEY}.local.unavailableBodyMac`
                : `${KEY}.local.unavailableBody`,
              { memory },
            )}
          </CardLine>
          <OwnServer
            prominent={prominent}
            inert={inert}
            onOpenProviders={onOpenProviders}
          />
        </SetupCard>
      );
  }
};
