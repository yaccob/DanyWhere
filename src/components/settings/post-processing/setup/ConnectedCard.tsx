import React from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { SetupStatus } from "@/bindings";
import { Button } from "../../../ui/Button";
import { DoneMark } from "../../../shared/DoneMark";
import { failureSentence } from "@/lib/postProcessFailure";
import { providerPages } from "@/lib/postProcessSetup/guides";
import { dollarsLabel, secondsLabel } from "@/lib/postProcessSetup/labels";
import { usePostProcessSetupStore } from "@/stores/postProcessSetupStore";
import { CardLine, SetupCard } from "./SetupCard";

const KEY = "settings.postProcessing.setup";

/** Ways that need nothing to disconnect: no key, no sign-in. */
const NOTHING_TO_DISCONNECT = ["own_server", "on_device"];

/** The sample sentence and what the provider made of it. */
const SampleRun: React.FC<{ providerLabel: string }> = ({ providerLabel }) => {
  const { t, i18n } = useTranslation();
  const sample = usePostProcessSetupStore((s) => s.sample);
  const providerId = usePostProcessSetupStore(
    (s) => s.status?.provider_id ?? "",
  );
  if (!sample) return null;
  return (
    <div className="space-y-1 rounded-md bg-wash px-3 py-2" aria-live="polite">
      <p className="m-0 text-xs font-semibold uppercase tracking-wide text-ink-3">
        {t(`${KEY}.sample.title`)}
      </p>
      <CardLine tone="quiet">
        {t(`${KEY}.sample.spoken`, { text: t(`${KEY}.sample.text`) })}
      </CardLine>
      {sample.phase === "running" && (
        <CardLine tone="quiet">{t(`${KEY}.sample.running`)}</CardLine>
      )}
      {sample.phase === "done" && sample.result && (
        <>
          <CardLine>
            {t(`${KEY}.sample.result`, { text: sample.result.output })}
          </CardLine>
          <CardLine tone="quiet">
            {t(`${KEY}.sample.took`, {
              seconds: secondsLabel(sample.result.millis, i18n.language),
              model: sample.result.model,
            })}
          </CardLine>
        </>
      )}
      {sample.phase === "failed" && sample.failure && (
        <CardLine tone="error">
          {t(`${KEY}.sample.failed`)}{" "}
          {failureSentence(t, sample.failure, {
            id: providerId,
            label: providerLabel,
          })}
        </CardLine>
      )}
    </div>
  );
};

interface ConnectedCardProps {
  status: SetupStatus;
  /** The provider's name as the provider list has it. */
  providerLabel: string;
  /** The answer "how should it go on", to nudge a top-up it chose. */
  wantsCredit: boolean;
  inert?: boolean;
  /** Settings: today's provider, model and effort, under "Advanced". */
  advanced?: { open: boolean; onToggle: () => void };
}

/**
 * DanyWhere: the setup once a way works (sidecar qa 2026-11-26_1810,
 * sketch 2): what it is connected to, a sample, where usage or credit is
 * managed, disconnecting, and the questions again. For ChatGPT the texts
 * follow OpenAI's guidelines ("Using ChatGPT plan", "Manage usage").
 */
export const ConnectedCard: React.FC<ConnectedCardProps> = ({
  status,
  providerLabel,
  wantsCredit,
  inert = false,
  advanced,
}) => {
  const { t, i18n } = useTranslation();
  const runSample = usePostProcessSetupStore((s) => s.runSample);
  const checkTopUp = usePostProcessSetupStore((s) => s.checkTopUp);
  const disconnect = usePostProcessSetupStore((s) => s.disconnect);
  const openQuestions = usePostProcessSetupStore((s) => s.openQuestions);
  const notToppedUp = usePostProcessSetupStore((s) => s.notToppedUp);
  const running = usePostProcessSetupStore(
    (s) => s.sample?.phase === "running",
  );
  const way = status.way;
  const pages = providerPages(
    way?.kind === "chatgpt_plan" ? "chatgpt" : (status.provider_id ?? ""),
  );
  const name = way?.kind === "chatgpt_plan" ? "ChatGPT" : providerLabel;
  const awaitsTopUp =
    way?.kind === "openrouter" && way.free_tier && wantsCredit;

  const lines: React.ReactNode[] = [];
  let manage: { label: string; url: string } | null = null;
  if (way?.kind === "chatgpt_plan") {
    lines.push(t(`${KEY}.connected.usingPlan`));
    if (way.plan) lines.push(t(`${KEY}.connected.plan`, { plan: way.plan }));
    lines.push(t(`${KEY}.connected.picksModel`));
    if (pages.usage)
      manage = { label: t(`${KEY}.connected.manageUsage`), url: pages.usage };
  } else if (way?.kind === "openrouter") {
    lines.push(
      awaitsTopUp
        ? t(`${KEY}.connected.noCreditYet`)
        : t(
            way.free_tier
              ? `${KEY}.connected.freeModels`
              : `${KEY}.connected.withCredit`,
          ),
    );
    if (way.limit_usd != null)
      lines.push(
        t(`${KEY}.connected.keyLimit`, {
          limit: dollarsLabel(way.limit_usd, i18n.language),
        }),
      );
    lines.push(t(`${KEY}.connected.picksModel`));
    if (pages.credit)
      manage = {
        label: t(
          awaitsTopUp ? `${KEY}.connected.topUp` : `${KEY}.connected.credit`,
        ),
        url: pages.credit,
      };
  } else {
    if (status.model)
      lines.push(t(`${KEY}.connected.model`, { model: status.model }));
    if (pages.usage)
      manage = { label: t(`${KEY}.connected.manageUsage`), url: pages.usage };
  }
  const canDisconnect = !NOTHING_TO_DISCONNECT.includes(way?.kind ?? "");

  return (
    <div className="space-y-3">
      <SetupCard
        icon={<DoneMark />}
        title={t(`${KEY}.connected.title`, { provider: name })}
      >
        {lines.map((line, index) => (
          <CardLine key={index}>{line}</CardLine>
        ))}
        {notToppedUp && (
          <CardLine tone="warning">
            {t(`${KEY}.connected.notToppedUp`)}
          </CardLine>
        )}
        <div className="flex flex-wrap gap-2">
          <Button
            variant="secondary"
            size="sm"
            disabled={inert || running}
            onClick={() => void runSample()}
          >
            {t(`${KEY}.connected.tryIt`)}
          </Button>
          {manage && (
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void openUrl(manage.url)}
            >
              {manage.label}
            </Button>
          )}
          {awaitsTopUp && (
            <Button
              variant="secondary"
              size="sm"
              disabled={inert}
              onClick={() => void checkTopUp()}
            >
              {t(`${KEY}.connected.toppedUp`)}
            </Button>
          )}
          {canDisconnect && (
            <Button
              variant="danger-ghost"
              size="sm"
              disabled={inert}
              onClick={() => void disconnect()}
            >
              {t(`${KEY}.connected.disconnect`)}
            </Button>
          )}
        </div>
        <SampleRun providerLabel={name} />
      </SetupCard>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <Button variant="ghost" size="sm" onClick={openQuestions}>
          {t(`${KEY}.connected.answerAgain`)}
        </Button>
        {advanced && (
          <Button
            variant="ghost"
            size="sm"
            aria-expanded={advanced.open}
            onClick={advanced.onToggle}
          >
            {t(`${KEY}.connected.advanced`)}
          </Button>
        )}
      </div>
    </div>
  );
};
