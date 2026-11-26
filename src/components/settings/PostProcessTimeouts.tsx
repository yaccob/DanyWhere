import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { parseWholeSeconds } from "@/lib/utils/seconds";
import { postProcessingOn } from "@/lib/endKeys";

// The same limits as `llm_timeouts.rs`; there is no 0 for "no limit".
const MIN_SECONDS = 5;
const MAX_SECONDS = 600;

type TimeoutKey = "post_process_timeout_secs" | "model_list_timeout_secs";

interface TimeoutSettingProps {
  settingKey: TimeoutKey;
  fallback: number;
  title: string;
  description: string;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** A time limit typed in whole seconds, checked against the limits above. */
const TimeoutSetting: React.FC<TimeoutSettingProps> = ({
  settingKey,
  fallback,
  title,
  description,
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const stored = Number(getSetting(settingKey) ?? fallback);
  const [draft, setDraft] = useState(String(stored));
  const [invalid, setInvalid] = useState(false);
  const range = { min: MIN_SECONDS, max: MAX_SECONDS };

  // Follow changes from elsewhere (reset, other windows).
  useEffect(() => setDraft(String(stored)), [stored]);

  const commit = () => {
    const seconds = parseWholeSeconds(draft, MIN_SECONDS, MAX_SECONDS);
    if (seconds === null) {
      setInvalid(true);
      setDraft(String(stored));
      return;
    }
    setInvalid(false);
    if (seconds !== stored) updateSetting(settingKey, seconds);
    setDraft(String(seconds));
  };

  return (
    <SettingContainer
      title={title}
      description={description}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center gap-2">
        <Input
          variant="compact"
          className={`w-20 text-end ${invalid ? "border-invalid" : ""}`}
          inputMode="numeric"
          value={draft}
          aria-invalid={invalid}
          title={
            invalid
              ? t("settings.advanced.llmTimeouts.invalid", range)
              : undefined
          }
          onChange={(e) => {
            setDraft(e.target.value);
            setInvalid(false);
          }}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
          }}
          disabled={isUpdating(settingKey)}
        />
        <span className="text-sm text-text/70">
          {t("settings.advanced.llmTimeouts.unit")}
        </span>
      </div>
    </SettingContainer>
  );
};

interface TimeoutProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/*
 * DanyWhere: the two time limits Handy fixes at 60 s and 15 s, how long
 * post-processing and a provider's model list may take. The first is a
 * profile value (the defaults' and a profile's Advanced page), the second
 * an application setting (Application Settings › Debug; plan
 * 2026-11-19_1910, qa 2026-11-22_1603).
 */

/** How long post-processing may take; shown only while post-processing is
 * on in the page's scope, since nothing else waits for it. */
export const PostProcessTimeout: React.FC<TimeoutProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting } = useSettings();
    const range = { min: MIN_SECONDS, max: MAX_SECONDS };

    if (
      !postProcessingOn({ post_process_mode: getSetting("post_process_mode") })
    )
      return null;

    return (
      <TimeoutSetting
        settingKey="post_process_timeout_secs"
        fallback={60}
        title={t("settings.advanced.llmTimeouts.postProcessTitle")}
        description={t(
          "settings.advanced.llmTimeouts.postProcessDescription",
          range,
        )}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

/** How long to wait for a provider's list of models. Always shown on the
 * Debug page: post-processing may be on in a profile while the one in use
 * has it off. */
export const ModelListTimeout: React.FC<TimeoutProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const range = { min: MIN_SECONDS, max: MAX_SECONDS };

    return (
      <TimeoutSetting
        settingKey="model_list_timeout_secs"
        fallback={15}
        title={t("settings.advanced.llmTimeouts.modelListTitle")}
        description={t(
          "settings.advanced.llmTimeouts.modelListDescription",
          range,
        )}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
