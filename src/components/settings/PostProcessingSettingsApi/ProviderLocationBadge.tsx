import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { providerLocation } from "../../../lib/providerLocation";

interface ProviderLocationBadgeProps {
  provider?: { id: string; base_url: string };
}

/**
 * DanyWhere: says next to the provider whether post-processing stays on this
 * computer or sends the dictation out (`lib/providerLocation`).
 */
export const ProviderLocationBadge: React.FC<ProviderLocationBadgeProps> = ({
  provider,
}) => {
  const { t } = useTranslation();
  if (!provider) return null;

  const local = providerLocation(provider) === "local";
  const label = local
    ? type() === "macos"
      ? t("settings.postProcessing.api.provider.location.onThisMac")
      : t("settings.postProcessing.api.provider.location.onThisComputer")
    : t("settings.postProcessing.api.provider.location.cloud");
  const hint = local
    ? t("settings.postProcessing.api.provider.location.localHint")
    : t("settings.postProcessing.api.provider.location.cloudHint");

  return (
    <span
      title={hint}
      aria-label={`${label}: ${hint}`}
      className={`shrink-0 whitespace-nowrap rounded-full px-2 py-0.5 text-xs font-medium ${
        local ? "bg-logo-primary/20" : "bg-wash border border-line"
      }`}
    >
      {label}
    </span>
  );
};
