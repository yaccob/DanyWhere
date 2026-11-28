import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { commands, type FailureDto } from "@/bindings";
import { Button } from "../../../ui/Button";
import { Input } from "../../../ui/Input";
import { GUIDES } from "@/lib/postProcessSetup/guides";
import type { GuidedKeyProvider } from "@/lib/postProcessSetup/questions";
import { failureSentence } from "@/lib/postProcessFailure";
import { usePostProcessSetupStore } from "@/stores/postProcessSetupStore";
import { CardLine, SetupCard } from "./SetupCard";

const KEY = "settings.postProcessing.setup.guidedKey";

interface GuidedKeyProps {
  provider: GuidedKeyProvider;
  /** The provider's name as the provider list has it. */
  providerLabel: string;
  prominent: boolean;
  inert?: boolean;
}

/**
 * DanyWhere: a key to paste, made understandable (sidecar qa
 * 2026-11-26_1810, 3.4): the steps with a button to each page they need,
 * a field for the key and a check button. A key that works becomes the
 * defaults' provider, with the model DanyWhere recommends, and the sample
 * follows; a refused one says why and is kept nowhere.
 */
export const GuidedKey: React.FC<GuidedKeyProps> = ({
  provider,
  providerLabel,
  prominent,
  inert = false,
}) => {
  const { t } = useTranslation();
  const guide = GUIDES[provider];
  const finishWay = usePostProcessSetupStore((s) => s.finishWay);
  const [key, setKey] = useState("");
  const [checking, setChecking] = useState(false);
  const [failure, setFailure] = useState<FailureDto | null>(null);
  const [works, setWorks] = useState(false);

  const check = async () => {
    const pasted = key.trim();
    if (!pasted) return;
    setChecking(true);
    setFailure(null);
    setWorks(false);
    try {
      const checked = await commands.checkPostProcessKey(provider, pasted);
      if (checked.status === "error") {
        setFailure(checked.error);
        return;
      }
      const applied = await commands.applyPostProcessChoice({
        provider_id: provider,
        model: checked.data.recommended ?? checked.data.models[0] ?? null,
        api_key: pasted,
        effort: null,
      });
      if (applied.status === "error") {
        setFailure({ reason: "other", detail: applied.error });
        return;
      }
      setWorks(true);
      await finishWay();
    } finally {
      setChecking(false);
    }
  };

  return (
    <SetupCard title={t(`${KEY}.title`, { service: guide.service })}>
      <ol className="m-0 list-decimal space-y-2 ps-5 text-sm text-ink-2">
        {guide.steps.map((step) => (
          <li key={step.text} className="text-pretty">
            {t(step.text)}
            {step.link && (
              <Button
                variant="secondary"
                size="sm"
                className="ms-2"
                onClick={() => void openUrl(step.link!.url)}
              >
                {t(step.link.label)}
              </Button>
            )}
          </li>
        ))}
      </ol>
      <div className="flex flex-wrap items-center gap-2">
        <Input
          type="password"
          variant="compact"
          value={key}
          onChange={(event) => setKey(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void check();
          }}
          placeholder={t(`${KEY}.paste`)}
          aria-label={t(`${KEY}.fieldLabel`, { service: guide.service })}
          autoComplete="off"
          spellCheck={false}
          disabled={inert}
          className="min-w-[260px] flex-1"
        />
        <Button
          variant={prominent ? "primary" : "outline"}
          size="md"
          disabled={inert || checking || key.trim() === ""}
          onClick={() => void check()}
        >
          {t(checking ? `${KEY}.checking` : `${KEY}.check`)}
        </Button>
      </div>
      {works && <CardLine tone="done">{t(`${KEY}.works`)}</CardLine>}
      {failure && (
        <CardLine tone="error">
          {failureSentence(t, failure, { id: provider, label: providerLabel })}
        </CardLine>
      )}
    </SetupCard>
  );
};
