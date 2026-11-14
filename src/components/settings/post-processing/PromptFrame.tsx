import React from "react";
import { useTranslation } from "react-i18next";
import { Alert } from "../../ui/Alert";
import { hasOldParts } from "@/lib/promptFrame";

// DanyWhere: the fixed frame sent before every prompt
// (src-tauri/src/prompt_frame.rs). The model always gets it in English; this
// shows it in the app's language.

/** The frame, read-only, above the prompt's instructions. */
export const PromptFrameText: React.FC = () => {
  const { t } = useTranslation();
  return (
    <div className="space-y-1">
      <p className="text-xs font-semibold text-ink-3">
        {t("settings.postProcessing.prompts.frame.label")}
      </p>
      <p
        className="p-3 rounded-md border border-line bg-wash text-xs text-ink-3 whitespace-pre-wrap select-text"
        aria-readonly="true"
      >
        {t("settings.postProcessing.prompts.frame.text")}
      </p>
    </div>
  );
};

/** Below the instructions: what follows them, and a warning while the
 * prompt still has `${output}` or `<transcript>` tags. */
export const PromptFrameTip: React.FC<{ prompt: string }> = ({ prompt }) => {
  const { t } = useTranslation();
  return (
    <>
      {hasOldParts(prompt) && (
        <Alert variant="warning">
          {t("settings.postProcessing.prompts.frame.legacyHint")}
        </Alert>
      )}
      <p className="text-xs text-ink-3">
        {t("settings.postProcessing.prompts.frame.tip")}
      </p>
    </>
  );
};
