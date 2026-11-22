// DanyWhere: names and short texts of a profile's values, for the state
// line ("Default: …"), the page "Profile", the confirmations and the
// settings view (qa 2026-11-19_1711 P4, P5, P7, P8). The texts themselves
// are `lib/valueLabels.ts`; this hook gives it the translations and what
// the app knows: models, providers, prompts, the defaults, the shortcut
// format of this system.
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { useModelStore } from "@/stores/modelStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { languageName } from "@/lib/constants/languages";
import { clipboardPasteKeys, formatKeyCombination } from "@/lib/utils/keyboard";
import type { LayeredProfile, OwnValue } from "@/lib/profileScope";
import {
  defaultText,
  entryName,
  fieldLabel,
  offEntryText,
  ownValueLabel,
  ownValueRow,
  valueLabel,
  type LabelContext,
} from "@/lib/valueLabels";
import { kindOf } from "./fieldKinds";

type Values = Readonly<Record<string, unknown>>;

const NO_VALUES: Values = {};

export const useValueLabels = () => {
  const { t, i18n } = useTranslation();
  const { settings } = useSettings();
  const osType = useOsType();
  const models = useModelStore((s) => s.models);
  const defaults = useSettingsStore(
    (s) =>
      (s.settings as unknown as { default_values?: Values | null } | null)
        ?.default_values ?? NO_VALUES,
  );
  const providers = settings?.post_process_providers;
  const prompts = settings?.post_process_prompts;
  const language = i18n.language;

  const ctx = useMemo<LabelContext>(
    () => ({
      t,
      exists: (key) => i18n.exists(key),
      kindOf,
      defaults,
      models,
      providers,
      prompts,
      formatShortcut: (binding) => formatKeyCombination(binding, osType, t),
      languageName: (code) => languageName(code, language),
      optionParams: {
        keys: formatKeyCombination(clipboardPasteKeys(osType), osType, t),
      },
    }),
    [t, i18n, defaults, models, providers, prompts, osType, language],
  );

  return useMemo(
    () => ({
      fieldLabel: (field: string) => fieldLabel(ctx, field),
      /** An entry's name; `within` are the values to look it up in when
       * the caller doesn't have it (the defaults always follow). */
      entryName: (
        field: string,
        id: string,
        entry?: unknown,
        within?: Values,
      ) => entryName(ctx, field, id, entry, within),
      valueLabel: (field: string, value: unknown, within?: Values) =>
        valueLabel(ctx, field, value, within),
      /** An entry a profile switched off: "off", or what a provider's
       * missing entry means. */
      offEntryText: (field: string) => offEntryText(ctx, field),
      ownValueLabel: (value: OwnValue, within: Values) =>
        ownValueLabel(ctx, value, within),
      ownValueRow: (value: OwnValue, within: Values) =>
        ownValueRow(ctx, value, within),
      defaultText: (
        fields: readonly string[],
        entry: string | undefined,
        profile: LayeredProfile,
      ) => defaultText(ctx, fields, entry, profile),
    }),
    [ctx],
  );
};
