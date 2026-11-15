// DanyWhere: the effort (how much the model thinks) next to the
// post-processing model, per profile (src-tauri/src/post_process_effort.rs).
import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type EffortOptions } from "@/bindings";
import { Dropdown, SettingContainer } from "@/components/ui";
import { useSettings } from "../../../hooks/useSettings";
import { effortChoice, MODEL_DEFAULT_EFFORT } from "./effortChoice";

type EffortSettingProps = {
  providerId: string;
  model: string;
};

export const EffortSetting: React.FC<EffortSettingProps> = ({
  providerId,
  model,
}) => {
  const { t } = useTranslation();
  const { settings, refreshSettings, postProcessModelOptions } = useSettings();
  const [options, setOptions] = useState<EffortOptions | null>(null);
  const [saving, setSaving] = useState(false);
  const stored = settings?.post_process_efforts?.[providerId] ?? null;
  // A loaded model list may say more about the model's levels.
  const modelList = postProcessModelOptions[providerId];

  useEffect(() => {
    let cancelled = false;
    void commands
      .getPostProcessEffortOptions(providerId, model)
      .then((result) => {
        if (cancelled) return;
        if (result.status === "ok") {
          setOptions(result.data);
        } else {
          console.error("Failed to load effort options:", result.error);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [providerId, model, modelList, stored]);

  const handleSelect = useCallback(
    async (value: string) => {
      setSaving(true);
      try {
        const result = await commands.changePostProcessEffortSetting(
          providerId,
          value === MODEL_DEFAULT_EFFORT ? null : value,
        );
        if (result.status === "error") {
          console.error("Failed to store the effort:", result.error);
        }
        await refreshSettings();
      } finally {
        setSaving(false);
      }
    },
    [providerId, refreshSettings],
  );

  const choice = effortChoice(options, stored);
  const levelLabel = (value: string) =>
    t(
      `settings.postProcessing.api.effort.levels.${value === MODEL_DEFAULT_EFFORT ? "default" : value}`,
    );
  const hint = choice.refused
    ? t("settings.postProcessing.api.effort.refused")
    : choice.noEffort
      ? t("settings.postProcessing.api.effort.notSupported")
      : choice.guessed
        ? t("settings.postProcessing.api.effort.guessed")
        : null;

  return (
    <SettingContainer
      title={t("settings.postProcessing.api.effort.title")}
      description={t("settings.postProcessing.api.effort.description")}
      descriptionMode="tooltip"
      layout="horizontal"
      grouped={true}
    >
      <div className="flex flex-col items-end gap-1">
        <Dropdown
          options={choice.values.map((value) => ({
            value,
            label: levelLabel(value),
          }))}
          selectedValue={choice.selected}
          onSelect={(value) => void handleSelect(value)}
          disabled={saving || choice.noEffort}
        />
        {hint && (
          <p className="text-xs text-ink-3 max-w-[320px] text-right">{hint}</p>
        )}
      </div>
    </SettingContainer>
  );
};
