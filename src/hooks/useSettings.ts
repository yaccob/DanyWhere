import { useEffect, useMemo } from "react";
import { useSettingsStore } from "../stores/settingsStore";
import {
  commands,
  type AppSettings as Settings,
  type AudioDevice,
  type PostProcessApiFormat,
} from "@/bindings";
// DanyWhere: the page's scope, the defaults or one profile (strand E2).
import { useSettingsScope } from "@/components/settings/profiles/scope/SettingsScope";
import {
  removeEntry,
  writeEntry,
  writeValue,
} from "@/components/settings/profiles/scope/profileWriter";
import {
  profileWrites,
  scopeValues,
  scopedSettings,
  type LayeredSettings,
} from "@/lib/profileScope";
import { PROFILE_FIELD_NAMES } from "@/lib/profileFields";
import { withoutModelOptions } from "@/components/settings/PostProcessingSettingsApi/modelAutoFetch";

interface UseSettingsReturn {
  // State
  settings: Settings | null;
  isLoading: boolean;
  isUpdating: (key: string) => boolean;
  audioDevices: AudioDevice[];
  outputDevices: AudioDevice[];
  audioFeedbackEnabled: boolean;
  postProcessModelOptions: Record<string, string[]>;
  updateChecksLocked: boolean | null;

  // Actions
  updateSetting: <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ) => Promise<void>;
  resetSetting: (key: keyof Settings) => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshAudioDevices: () => Promise<void>;
  refreshOutputDevices: () => Promise<void>;

  // Binding-specific actions
  updateBinding: (id: string, binding: string) => Promise<void>;
  resetBinding: (id: string) => Promise<void>;

  // Convenience getters
  getSetting: <K extends keyof Settings>(key: K) => Settings[K] | undefined;

  // Post-processing helpers
  setPostProcessProvider: (providerId: string) => Promise<void>;
  updatePostProcessBaseUrl: (
    providerId: string,
    baseUrl: string,
  ) => Promise<void>;
  updatePostProcessApiFormat: (
    providerId: string,
    apiFormat: PostProcessApiFormat,
  ) => Promise<void>;
  updatePostProcessApiKey: (
    providerId: string,
    apiKey: string,
  ) => Promise<void>;
  updatePostProcessModel: (providerId: string, model: string) => Promise<void>;
  fetchPostProcessModels: (providerId: string) => Promise<string[]>;
  // DanyWhere: the effort per provider; `null` is the model's default.
  setPostProcessEffort: (
    providerId: string,
    effort: string | null,
  ) => Promise<void>;
}

/** DanyWhere: forgets a provider's loaded model list, so the page loads it
 * again with the changed key or address (as the store does). */
const forgetModelOptions = (providerId: string) =>
  useSettingsStore.setState((state) => ({
    postProcessModelOptions: withoutModelOptions(
      state.postProcessModelOptions,
      providerId,
    ),
  }));

/** The effort through its own command, for the values in use. */
const changeEffortInUse = async (providerId: string, effort: string | null) => {
  const result = await commands.changePostProcessEffortSetting(
    providerId,
    effort,
  );
  if (result.status === "error") {
    console.error("Failed to store the effort:", result.error);
  }
  await useSettingsStore.getState().refreshSettings();
};

export const useSettings = (): UseSettingsReturn => {
  const store = useSettingsStore();
  // DanyWhere: inside a page of the defaults or of a profile, the values
  // read and written are the scope's (qa 2026-11-19_1711 P4, P7).
  const scope = useSettingsScope();

  // Initialize on first mount
  useEffect(() => {
    if (store.isLoading) {
      store.initialize();
    }
  }, [store.initialize, store.isLoading]);

  const settings = useMemo(() => {
    if (!scope || !store.settings) return store.settings;
    const values = scopeValues(
      store.settings as unknown as LayeredSettings,
      scope,
    );
    return values ? scopedSettings(store.settings, values) : store.settings;
  }, [scope, store.settings]);

  const base: UseSettingsReturn = {
    settings: store.settings,
    isLoading: store.isLoading,
    isUpdating: store.isUpdatingKey,
    audioDevices: store.audioDevices,
    outputDevices: store.outputDevices,
    audioFeedbackEnabled: store.settings?.audio_feedback || false,
    postProcessModelOptions: store.postProcessModelOptions,
    updateChecksLocked: store.updateChecksLocked,
    updateSetting: store.updateSetting,
    resetSetting: store.resetSetting,
    refreshSettings: store.refreshSettings,
    refreshAudioDevices: store.refreshAudioDevices,
    refreshOutputDevices: store.refreshOutputDevices,
    updateBinding: store.updateBinding,
    resetBinding: store.resetBinding,
    getSetting: store.getSetting,
    setPostProcessProvider: store.setPostProcessProvider,
    updatePostProcessBaseUrl: store.updatePostProcessBaseUrl,
    updatePostProcessApiFormat: store.updatePostProcessApiFormat,
    updatePostProcessApiKey: store.updatePostProcessApiKey,
    updatePostProcessModel: store.updatePostProcessModel,
    fetchPostProcessModels: store.fetchPostProcessModels,
    setPostProcessEffort: changeEffortInUse,
  };
  if (!scope) return base;

  // DanyWhere: the scoped reads and writes. Settings that are no profile
  // value (the application's) change as before.
  const getSetting = <K extends keyof Settings>(
    key: K,
  ): Settings[K] | undefined => settings?.[key];

  const updateSetting = async <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ) => {
    const writes = profileWrites(
      String(key),
      value,
      (settings ?? {}) as unknown as Record<string, unknown>,
      PROFILE_FIELD_NAMES,
    );
    if (writes === null) {
      await store.updateSetting(key, value);
      return;
    }
    for (const [field, next] of writes) {
      await writeValue(scope, field, next);
    }
  };

  const bindingOf = (id: string) => {
    const binding = settings?.bindings?.[id];
    if (!binding) throw new Error(`Unknown shortcut ${id}`);
    return binding;
  };

  const provider = (providerId: string) =>
    settings?.post_process_providers?.find((p) => p.id === providerId);

  return {
    ...base,
    settings,
    audioFeedbackEnabled: settings?.audio_feedback || false,
    getSetting,
    updateSetting,
    resetSetting: async (key) => {
      const value = store.defaultSettings?.[key];
      if (value !== undefined) await updateSetting(key, value);
    },
    updateBinding: async (id, binding) => {
      const saved = await writeEntry(scope, "bindings", id, {
        ...bindingOf(id),
        current_binding: binding,
      });
      // The shortcut field rolls back on a throw, as with Handy's command.
      if (!saved) throw new Error(`Shortcut ${id} not saved`);
    },
    resetBinding: async (id) => {
      const current = bindingOf(id);
      await writeEntry(scope, "bindings", id, {
        ...current,
        current_binding: current.default_binding,
      });
    },
    setPostProcessProvider: async (providerId) => {
      forgetModelOptions(providerId);
      await writeValue(scope, "post_process_provider_id", providerId);
    },
    updatePostProcessBaseUrl: async (providerId, baseUrl) => {
      const current = provider(providerId);
      if (!current) return;
      const saved = await writeEntry(
        scope,
        "post_process_providers",
        providerId,
        { ...current, base_url: baseUrl },
      );
      // A model of the old address is almost certainly wrong for the new
      // one (as Handy's store does).
      if (saved) await writeEntry(scope, "post_process_models", providerId, "");
      forgetModelOptions(providerId);
    },
    updatePostProcessApiFormat: async (providerId, apiFormat) => {
      const current = provider(providerId);
      if (!current) return;
      forgetModelOptions(providerId);
      await writeEntry(scope, "post_process_providers", providerId, {
        ...current,
        api_format: apiFormat,
      });
    },
    updatePostProcessApiKey: async (providerId, apiKey) => {
      forgetModelOptions(providerId);
      await writeEntry(scope, "post_process_api_keys", providerId, apiKey);
    },
    updatePostProcessModel: async (providerId, model) => {
      await writeEntry(scope, "post_process_models", providerId, model);
    },
    setPostProcessEffort: async (providerId, effort) => {
      if (effort !== null) {
        await writeEntry(scope, "post_process_efforts", providerId, effort);
      } else if (settings?.post_process_efforts?.[providerId] !== undefined) {
        await removeEntry(scope, "post_process_efforts", providerId);
      }
    },
  };
};
