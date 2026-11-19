// DanyWhere: names and short texts of a profile's values, for the state
// line ("Default: …"), the page "Profile", the confirmations and the
// settings view (qa 2026-11-19_1711 P4, P5, P7, P8). Generic: a field's name
// comes from the registry, an entry's from its own name, a value's text
// from `valueText`, with an option's translated label where one exists.
import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { useModelStore } from "@/stores/modelStore";
import { fieldInfo } from "@/lib/profileFields";
import { entryLabel, valueText, type OwnValue } from "@/lib/profileScope";

/** Values that are never shown, only whether they are set. */
const SECRET_FIELDS = new Set(["post_process_api_keys"]);

/** Fields whose entries are named after a post-processing provider. */
const PROVIDER_MAPS = new Set([
  "post_process_models",
  "post_process_efforts",
  "post_process_api_keys",
  "post_process_providers",
]);

const camel = (text: string) =>
  text.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase());

export const useValueLabels = () => {
  const { t, i18n } = useTranslation();
  const { settings } = useSettings();
  const models = useModelStore((s) => s.models);
  const providers = settings?.post_process_providers;

  const fieldLabel = useCallback(
    (field: string) => {
      const info = fieldInfo(field);
      return info ? t(info.label) : field;
    },
    [t],
  );

  const entryName = useCallback(
    (field: string, id: string, entry?: unknown) => {
      if (field === "bindings" || field === "default_ends") {
        return t(`settings.general.shortcut.bindings.${id}.name`, id);
      }
      if (PROVIDER_MAPS.has(field)) {
        return providers?.find((p) => p.id === id)?.label ?? id;
      }
      const name = entryLabel(entry, id);
      if (field === "end_keys" && name === id) {
        return t(`settings.endKeys.builtin.${id}`, id);
      }
      return name;
    },
    [providers, t],
  );

  /** The text of `value` of `field` (or of its entry). */
  const valueLabel = useCallback(
    (field: string, value: unknown): string => {
      if (SECRET_FIELDS.has(field) || typeof value === "undefined") {
        return value
          ? t("settings.profileView.value.hidden")
          : t("settings.profileView.value.empty");
      }
      if (field === "selected_model" && typeof value === "string") {
        return models.find((m) => m.id === value)?.name ?? value;
      }
      if (typeof value === "string" && value) {
        // An option of a dropdown, translated where its key follows the
        // setting's name ("…pasteMethod.title" → "…pasteMethod.options.direct").
        const base = fieldInfo(field)?.label.replace(/\.(title|label)$/, "");
        const option = base ? `${base}.options.${camel(value)}` : null;
        if (option && i18n.exists(option)) {
          const text = t(option);
          if (typeof text === "string" && !text.includes("{{")) return text;
        }
      }
      const shown = valueText(value);
      if ("text" in shown) return shown.text;
      if (shown.key === "items") {
        return t("settings.profileView.value.items", { count: shown.count });
      }
      return t(`settings.profileView.value.${shown.key}`);
    },
    [i18n, models, t],
  );

  /** "Setting" or "Setting: entry" for an own value of a profile. */
  const ownValueLabel = useCallback(
    (value: OwnValue) =>
      value.entry === undefined
        ? fieldLabel(value.field)
        : `${fieldLabel(value.field)}: ${entryName(value.field, value.entry)}`,
    [entryName, fieldLabel],
  );

  return { fieldLabel, entryName, valueLabel, ownValueLabel };
};
