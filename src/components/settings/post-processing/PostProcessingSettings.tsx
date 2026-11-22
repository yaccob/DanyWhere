import React from "react";
import { useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import type { PostProcessApiFormat, PromptOrderEntry } from "@/bindings";

import { Alert } from "../../ui/Alert";
import { Dropdown, SettingContainer, SettingsGroup } from "@/components/ui";
import { ResetButton } from "../../ui/ResetButton";

import { ProviderSelect } from "../PostProcessingSettingsApi/ProviderSelect";
import { ProviderLocationBadge } from "../PostProcessingSettingsApi/ProviderLocationBadge";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import { EffortSetting } from "../PostProcessingSettingsApi/EffortSetting";
import { usePostProcessProviderState } from "../PostProcessingSettingsApi/usePostProcessProviderState";
import { ShortcutInput } from "../ShortcutInput";
import { useSettings } from "../../../hooks/useSettings";
// DanyWhere: context from earlier dictations in the same window.
import { DictationContextSettings } from "../dictation-context/DictationContextSettings";
// DanyWhere: one prompt library for all profiles.
import { PromptLibrary } from "./PromptLibrary";
// DanyWhere (strand E2): each profile value with its chain and the profiles
// that deviate from it. The provider's settings are shown as text under the
// defaults: their controls take the values of the page's scope as props.
import { ScopedSetting } from "../profiles/scope/ScopedSetting";

const PostProcessingSettingsApiComponent: React.FC = () => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState();

  return (
    <>
      <ScopedSetting field="post_process_provider_id" textOnly>
        <SettingContainer
          title={t("settings.postProcessing.api.provider.title")}
          description={t("settings.postProcessing.api.provider.description")}
          descriptionMode="tooltip"
          layout="horizontal"
          grouped={true}
        >
          <div className="flex items-center gap-2">
            <ProviderSelect
              options={state.providerOptions}
              value={state.selectedProviderId}
              onChange={state.handleProviderSelect}
            />
            {/* DanyWhere: on this computer or in the cloud. */}
            <ProviderLocationBadge provider={state.selectedProvider} />
          </div>
        </SettingContainer>
      </ScopedSetting>

      {state.isAppleProvider ? (
        state.appleIntelligenceUnavailable ? (
          <Alert variant="error" contained>
            {t("settings.postProcessing.api.appleIntelligence.unavailable")}
          </Alert>
        ) : null
      ) : (
        <>
          {state.isCustomProvider && (
            <ScopedSetting
              field="post_process_providers"
              entry={state.selectedProviderId}
              textOnly
            >
              <SettingContainer
                title={t("settings.postProcessing.api.apiFormat.title")}
                description={t(
                  "settings.postProcessing.api.apiFormat.description",
                )}
                descriptionMode="tooltip"
                layout="horizontal"
                grouped={true}
              >
                <Dropdown
                  options={[
                    {
                      value: "openai",
                      label: t(
                        "settings.postProcessing.api.apiFormat.options.openai",
                      ),
                    },
                    {
                      value: "anthropic",
                      label: t(
                        "settings.postProcessing.api.apiFormat.options.anthropic",
                      ),
                    },
                  ]}
                  selectedValue={state.apiFormat}
                  onSelect={(value) =>
                    state.handleApiFormatChange(value as PostProcessApiFormat)
                  }
                  disabled={state.isApiFormatUpdating}
                />
              </SettingContainer>
            </ScopedSetting>
          )}

          {state.selectedProvider?.id === "custom" && (
            <ScopedSetting
              field="post_process_providers"
              entry={state.selectedProviderId}
              textOnly
            >
              <SettingContainer
                title={t("settings.postProcessing.api.baseUrl.title")}
                description={t(
                  "settings.postProcessing.api.baseUrl.description",
                )}
                descriptionMode="tooltip"
                layout="horizontal"
                grouped={true}
              >
                <div className="flex items-center gap-2">
                  <BaseUrlField
                    value={state.baseUrl}
                    onBlur={state.handleBaseUrlChange}
                    placeholder={
                      state.apiFormat === "anthropic"
                        ? t(
                            "settings.postProcessing.api.baseUrl.placeholderAnthropic",
                          )
                        : t("settings.postProcessing.api.baseUrl.placeholder")
                    }
                    disabled={state.isBaseUrlUpdating}
                    className="min-w-[380px]"
                  />
                </div>
              </SettingContainer>
            </ScopedSetting>
          )}

          <ScopedSetting
            field="post_process_api_keys"
            entry={state.selectedProviderId}
            textOnly
          >
            <SettingContainer
              title={t("settings.postProcessing.api.apiKey.title")}
              description={t("settings.postProcessing.api.apiKey.description")}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <ApiKeyField
                  value={state.apiKey}
                  onBlur={state.handleApiKeyChange}
                  placeholder={t(
                    "settings.postProcessing.api.apiKey.placeholder",
                  )}
                  disabled={state.isApiKeyUpdating}
                  className="min-w-[320px]"
                />
              </div>
            </SettingContainer>
          </ScopedSetting>
        </>
      )}

      {!state.isAppleProvider && (
        <ScopedSetting
          field="post_process_models"
          entry={state.selectedProviderId}
          textOnly
        >
          <SettingContainer
            title={t("settings.postProcessing.api.model.title")}
            description={
              state.isCustomProvider
                ? t("settings.postProcessing.api.model.descriptionCustom")
                : t("settings.postProcessing.api.model.descriptionDefault")
            }
            descriptionMode="tooltip"
            layout="stacked"
            grouped={true}
          >
            <div className="flex items-center gap-2">
              <ModelSelect
                value={state.model}
                options={state.modelOptions}
                disabled={state.isModelUpdating}
                isLoading={state.isFetchingModels}
                placeholder={
                  state.modelOptions.length > 0
                    ? t(
                        "settings.postProcessing.api.model.placeholderWithOptions",
                      )
                    : t(
                        "settings.postProcessing.api.model.placeholderNoOptions",
                      )
                }
                onSelect={state.handleModelSelect}
                onCreate={state.handleModelCreate}
                onBlur={() => {}}
                className="flex-1 min-w-[380px]"
              />
              <ResetButton
                onClick={state.handleRefreshModels}
                disabled={state.isFetchingModels}
                ariaLabel={t("settings.postProcessing.api.model.refreshModels")}
                className="flex h-10 w-10 items-center justify-center"
              >
                <RefreshCcw
                  className={`h-4 w-4 ${state.isFetchingModels ? "animate-spin" : ""}`}
                />
              </ResetButton>
            </div>
          </SettingContainer>
        </ScopedSetting>
      )}

      {/* DanyWhere: how much the model thinks, per profile. */}
      {!state.isAppleProvider && (
        <ScopedSetting
          field="post_process_efforts"
          entry={state.selectedProviderId}
          textOnly
        >
          <EffortSetting
            providerId={state.selectedProviderId}
            model={state.model}
          />
        </ScopedSetting>
      )}
    </>
  );
};

export const PostProcessingSettingsApi = React.memo(
  PostProcessingSettingsApiComponent,
);
PostProcessingSettingsApi.displayName = "PostProcessingSettingsApi";

// DanyWhere: the prompt library with the active profile's order, instead
// of Handy's one selected prompt.
export const PostProcessingSettingsPrompts = React.memo(PromptLibrary);
PostProcessingSettingsPrompts.displayName = "PostProcessingSettingsPrompts";

export const PostProcessingSettings: React.FC = () => {
  const { t } = useTranslation();
  // DanyWhere (strand E2): a deviating profile's prompts under the
  // defaults, as the names of its active ones (P7.2).
  const { getSetting } = useSettings();
  const library = getSetting("post_process_prompts") ?? [];
  const promptSummary = (values: Record<string, unknown>) => {
    const order = Array.isArray(values.post_process_prompt_order)
      ? (values.post_process_prompt_order as PromptOrderEntry[])
      : [];
    const names = order
      .filter((entry) => entry.active)
      .map((entry) => library.find((p) => p.id === entry.id)?.name)
      .filter((name): name is string => Boolean(name));
    return names.length > 0
      ? t("settings.profileView.summary.active", { names: names.join(", ") })
      : t("settings.profileView.summary.noneActive");
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.postProcessing.hotkey.title")}>
        <ScopedSetting field="bindings" entry="transcribe_with_post_process">
          <ShortcutInput
            shortcutId="transcribe_with_post_process"
            descriptionMode="tooltip"
            grouped={true}
          />
        </ScopedSetting>
      </SettingsGroup>

      <SettingsGroup title={t("settings.postProcessing.api.title")}>
        <PostProcessingSettingsApi />
      </SettingsGroup>

      <SettingsGroup title={t("settings.postProcessing.prompts.title")}>
        <ScopedSetting
          field="post_process_prompt_order"
          summary={promptSummary}
        >
          <PostProcessingSettingsPrompts />
        </ScopedSetting>
      </SettingsGroup>

      {/* DanyWhere: each of its rows has its own wrapper. */}
      <SettingsGroup title={t("settings.postProcessing.context.title")}>
        <DictationContextSettings />
      </SettingsGroup>
    </div>
  );
};
