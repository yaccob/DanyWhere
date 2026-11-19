// DanyWhere: writing a value where the page says (qa 2026-11-19_1711 P7,
// P7.3, P10): into the defaults or one profile, through the commands of the
// coupling model (`profile_layers.rs`), never by the old rule "where the
// value in use came from". One place for every write of the scoped pages:
// `useSettings` goes through it, and so do the few components that called
// a command of their own (word lists, effort, end shortcuts).
import i18n from "i18next";
import { commands, type JsonValue } from "@/bindings";
import { useSettingsStore } from "@/stores/settingsStore";
import { notify } from "@/stores/noticeStore";
import {
  becomesOwn,
  patchScope,
  type LayeredProfile,
  type LayeredSettings,
  type Scope,
} from "@/lib/profileScope";
import { kindOf } from "./fieldKinds";
import { useDecoupleBar } from "./decoupleBarStore";
import { useSettingsScope } from "./SettingsScope";

type Outcome =
  | { status: "ok"; data: unknown }
  | { status: "error"; error: string };

const layered = (): LayeredSettings =>
  (useSettingsStore.getState().settings ?? {}) as unknown as LayeredSettings;

const profileOf = (scope: Scope): LayeredProfile | null =>
  scope.kind === "profile"
    ? (layered().profiles?.find((profile) => profile.id === scope.id) ?? null)
    : null;

/** A value as the commands take it: plain JSON. */
const json = (value: unknown): JsonValue =>
  (value === undefined ? null : value) as JsonValue;

/**
 * Runs `command`, tells a failure and refreshes the settings. In a profile,
 * a value that was coupled before becomes its own: the bar of P7.3 offers
 * to change the default instead.
 */
/** Turns a refusal of the backend into the notice's title; the default is
 * "Couldn't save the change" with the backend's reason. */
export type Describe = (error: string) => string | null;

const write = async (
  scope: Scope,
  field: string,
  entry: string | undefined,
  command: () => Promise<Outcome>,
  describe?: Describe,
): Promise<boolean> => {
  const store = useSettingsStore.getState();
  const profile = profileOf(scope);
  const decouples =
    profile !== null && becomesOwn(profile, field, kindOf(field), entry);
  const updateKey = entry === undefined ? field : `${field}:${entry}`;
  store.setUpdating(updateKey, true);
  try {
    const result = await command();
    if (result.status === "error") {
      const title = describe?.(result.error);
      notify({
        title: title ?? i18n.t("settings.profileView.errors.save"),
        detail: title ? undefined : result.error,
        level: "warning",
      });
      return false;
    }
    if (decouples && profile) {
      useDecoupleBar.getState().show({ profileId: profile.id, field, entry });
    }
    return true;
  } catch (error) {
    notify({
      title: i18n.t("settings.profileView.errors.save"),
      detail: String(error),
      level: "warning",
    });
    return false;
  } finally {
    await useSettingsStore.getState().refreshSettings();
    useSettingsStore.getState().setUpdating(updateKey, false);
  }
};

/** Sets `field` in `scope`; the page shows the new value at once. */
export const writeValue = (
  scope: Scope,
  field: string,
  value: unknown,
  describe?: Describe,
) => {
  const store = useSettingsStore.getState();
  if (store.settings) {
    store.setSettings(
      patchScope(
        store.settings as unknown as LayeredSettings,
        scope,
        field,
        value,
      ) as unknown as typeof store.settings,
    );
  }
  return write(
    scope,
    field,
    undefined,
    () => commands.setProfileValue(scope, field, json(value)),
    describe,
  );
};

/** Sets or adds entry `entryId` of list field `field` in `scope`. */
export const writeEntry = (
  scope: Scope,
  field: string,
  entryId: string,
  entry: unknown,
) =>
  write(scope, field, entryId, () =>
    commands.setProfileEntry(scope, field, entryId, json(entry)),
  );

/** Removes entry `entryId` from the defaults, or switches it off in a
 * profile (P10). */
export const removeEntry = (scope: Scope, field: string, entryId: string) =>
  write(scope, field, entryId, () =>
    commands.removeProfileEntry(scope, field, entryId),
  );

/** Runs a coupling command (reset, hold, …) and refreshes the settings. */
export const runCoupling = async (
  command: () => Promise<Outcome>,
): Promise<boolean> => {
  try {
    const result = await command();
    if (result.status === "error") {
      notify({
        title: i18n.t("settings.profileView.errors.save"),
        detail: result.error,
        level: "warning",
      });
      return false;
    }
    return true;
  } catch (error) {
    notify({
      title: i18n.t("settings.profileView.errors.save"),
      detail: String(error),
      level: "warning",
    });
    return false;
  } finally {
    await useSettingsStore.getState().refreshSettings();
  }
};

export interface ProfileWriter {
  /** The page's scope; `null` outside one, where the old commands apply. */
  scope: Scope | null;
  setValue: (field: string, value: unknown) => Promise<boolean>;
  setEntry: (
    field: string,
    entryId: string,
    entry: unknown,
  ) => Promise<boolean>;
  removeEntry: (field: string, entryId: string) => Promise<boolean>;
}

/** The writes of the page's scope. */
export const useProfileWriter = (): ProfileWriter => {
  const scope = useSettingsScope();
  return {
    scope,
    setValue: (field, value) =>
      scope ? writeValue(scope, field, value) : Promise.resolve(false),
    setEntry: (field, entryId, entry) =>
      scope ? writeEntry(scope, field, entryId, entry) : Promise.resolve(false),
    removeEntry: (field, entryId) =>
      scope ? removeEntry(scope, field, entryId) : Promise.resolve(false),
  };
};
