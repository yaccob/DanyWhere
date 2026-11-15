// DanyWhere: the switch "Send Context" (`send_context.rs`), on the Context
// settings and, small, under the profile selector in the sidebar.
import React from "react";
import { useTranslation } from "react-i18next";
import { useSendContext } from "@/hooks/useSendContext";
import { ToggleSwitch } from "../../ui/ToggleSwitch";

/** The switch on the Context settings (page "Post Process"). */
export const SendContextSwitch: React.FC = () => {
  const { t } = useTranslation();
  const { enabled, setEnabled, busy } = useSendContext();
  return (
    <ToggleSwitch
      checked={enabled}
      onChange={(value) => void setEnabled(value)}
      isUpdating={busy}
      label={t("settings.postProcessing.context.sendContext.label")}
      description={t("settings.postProcessing.context.sendContext.description")}
      descriptionMode="tooltip"
      grouped={true}
    />
  );
};

/** The same switch, small, in the sidebar while the active profile
 * post-processes (without post-processing no context is ever sent). */
export const SendContextCompact: React.FC = () => {
  const { t } = useTranslation();
  const { enabled, postProcessing, setEnabled, busy } = useSendContext();
  if (!postProcessing) return null;
  return (
    <label
      className="flex items-center gap-2 px-1 pt-2 text-xs text-ink-2 cursor-pointer"
      title={t("settings.postProcessing.context.sendContext.description")}
    >
      <input
        type="checkbox"
        checked={enabled}
        disabled={busy}
        onChange={(event) => void setEnabled(event.target.checked)}
        className="accent-logo-primary"
      />
      {t("settings.postProcessing.context.sendContext.short")}
    </label>
  );
};
