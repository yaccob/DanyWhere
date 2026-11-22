// DanyWhere: names the app shows for values whose stored form is English,
// and where Handy's reset buttons show (qa 2026-11-22_1632, the user's
// decisions on points 4 and 6). The stored values stay as they are; only
// what is shown is translated.
import type { Scope } from "./profileScope";

/** Translates a key, with options (i18next's `t`). */
export type Translate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

/** Whether Handy's reset buttons show. They set a setting to its factory
 * value: right under the defaults and in the app's own settings, but in a
 * profile "back to the defaults" is the state line's Reset, and a second
 * button to the factory value next to it would only confuse. */
export const showsFactoryReset = (scope: Scope | null): boolean =>
  scope?.kind !== "profile";

/** The names the backend gives the system's default device in the device
 * lists (`commands/audio.rs`), and a profile's stored form of it. */
const DEFAULT_DEVICE_NAMES = new Set(["Default", "default"]);

/** What a device list shows for a device: the system's default device by a
 * translated name, every other device by its own. */
export const deviceLabel = (name: string, t: Translate): string =>
  DEFAULT_DEVICE_NAMES.has(name) ? t("settings.sound.systemDefault") : name;

/** The provider whose label comes from Handy's defaults in English. */
const CUSTOM_PROVIDER_ID = "custom";

/** What a provider is called: the custom provider by a translated name, the
 * others by their stored label (brand names). */
export const providerName = (
  provider: { id: string; label: string },
  t: Translate,
): string =>
  provider.id === CUSTOM_PROVIDER_ID
    ? t("settings.postProcessing.api.provider.custom")
    : provider.label;
