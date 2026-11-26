import i18n from "i18next";
import { type } from "@tauri-apps/plugin-os";
import { useSettingsStore } from "@/stores/settingsStore";
import { formatKeyCombination, type OSType } from "@/lib/utils/keyboard";
import { keyRefLabel } from "@/lib/startKeys";
import type { KeyRefLabel, Translate } from "@/lib/shortcutRefusal";

type Values = Readonly<Record<string, unknown>>;

/** This system, as the shortcut controls name it (`useOsType`). */
const osType = (): OSType => {
  const os = type();
  return os === "macos" || os === "windows" || os === "linux" ? os : "unknown";
};

/**
 * DanyWhere: what a start, hold or end shortcut a refusal of the backend
 * refers to is called (`conflict:hold_key:<id>`, `start-key-cannot-end:<id>`;
 * qa 2026-11-26_1810): looked up in the settings as last loaded, in the
 * values in use, the defaults' and each profile's, since an id names the
 * same entry in every layer. Not a hook, so the scoped writes and the
 * shortcut capture can use it too.
 */
export const storeKeyRefLabel =
  (t: Translate = (key, options) => i18n.t(key, options)): KeyRefLabel =>
  (ref) => {
    const settings = useSettingsStore.getState().settings as unknown as
      | (Values & {
          default_values?: Values | null;
          profiles?: ReadonlyArray<{ values?: Values }> | null;
        })
      | null;
    if (!settings) return null;
    const os = osType();
    return keyRefLabel(
      ref,
      [
        settings,
        settings.default_values,
        ...(settings.profiles ?? []).map((profile) => profile.values),
      ],
      (keys) => formatKeyCombination(keys, os, t),
      (shortcut) => t("settings.endKeys.held", { shortcut }),
    );
  };
