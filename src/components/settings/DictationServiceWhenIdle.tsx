import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { DictationServiceWhenIdle } from "@/bindings";

interface DictationServiceWhenIdleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * DanyWhere: what the dictation shortcut does when it reaches an app while
 * nothing is recording (e.g. DanyWhere wasn't running yet). It never inserts
 * an earlier dictation.
 */
export const DictationServiceWhenIdleSetting: React.FC<DictationServiceWhenIdleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const options = [
      {
        value: "start_recording",
        label: t(
          "settings.general.dictationServiceWhenIdle.options.startRecording",
        ),
      },
      {
        value: "do_nothing",
        label: t("settings.general.dictationServiceWhenIdle.options.doNothing"),
      },
    ];

    const selected = (getSetting("dictation_service_when_idle") ||
      "start_recording") as DictationServiceWhenIdle;

    return (
      <SettingContainer
        title={t("settings.general.dictationServiceWhenIdle.title")}
        description={t("settings.general.dictationServiceWhenIdle.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          selectedValue={selected}
          onSelect={(value) =>
            updateSetting(
              "dictation_service_when_idle",
              value as DictationServiceWhenIdle,
            )
          }
          disabled={isUpdating("dictation_service_when_idle")}
        />
      </SettingContainer>
    );
  });
