import React from "react";
import { useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import type { PostProcessApiFormat } from "@/bindings";

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
// DanyWhere: most of this page belongs to the active profile.
import { ProfileScopeHint } from "../profiles/ProfileScopeHint";

const PostProcessingSettingsApiComponent: React.FC = () => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState();

  return (
    <>
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

      {state.isAppleProvider ? (
        state.appleIntelligenceUnavailable ? (
          <Alert variant="error" contained>
            {t("settings.postProcessing.api.appleIntelligence.unavailable")}
          </Alert>
        ) : null
      ) : (
        <>
          {state.isCustomProvider && (
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
          )}

          {state.selectedProvider?.id === "custom" && (
            <SettingContainer
              title={t("settings.postProcessing.api.baseUrl.title")}
              description={t("settings.postProcessing.api.baseUrl.description")}
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
          )}

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
        </>
      )}

      {!state.isAppleProvider && (
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
                  : t("settings.postProcessing.api.model.placeholderNoOptions")
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
      )}

      {/* DanyWhere: how much the model thinks, per profile. */}
      {!state.isAppleProvider && (
        <EffortSetting
          providerId={state.selectedProviderId}
          model={state.model}
        />
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

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      {/* DanyWhere */}
      <ProfileScopeHint page="postProcessing" className="px-4" />
      <SettingsGroup title={t("settings.postProcessing.hotkey.title")}>
        <ShortcutInput
          shortcutId="transcribe_with_post_process"
          descriptionMode="tooltip"
          grouped={true}
        />
      </SettingsGroup>

      <SettingsGroup title={t("settings.postProcessing.api.title")}>
        <PostProcessingSettingsApi />
      </SettingsGroup>

      <SettingsGroup title={t("settings.postProcessing.prompts.title")}>
        <PostProcessingSettingsPrompts />
      </SettingsGroup>

      {/* DanyWhere */}
      <SettingsGroup title={t("settings.postProcessing.context.title")}>
        <DictationContextSettings />
      </SettingsGroup>
    </div>
  );
};
