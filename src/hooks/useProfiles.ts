import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Result } from "@/bindings";
import {
  DEFAULT_PROFILE_ID,
  activeProfile,
  profileDisplayName,
  type ProfileLike,
} from "@/lib/utils/profiles";
import { hasAssignments } from "@/lib/utils/profileApps";
import { useModelStore } from "@/stores/modelStore";
import { notify } from "@/stores/noticeStore";
import { useSettings } from "./useSettings";

/** Whether a profile command went through; a failure was already shown. */
export type ProfileCommandOutcome<T> = { ok: true; data: T } | { ok: false };

/** DanyWhere: the profiles from the settings and the commands that change
 * them. Each command refreshes the settings afterwards, as the word list
 * commands do; a failure shows as a notice with the backend's reason. */
export const useProfiles = () => {
  const { t } = useTranslation();
  const { getSetting, refreshSettings } = useSettings();
  const [busy, setBusy] = useState(false);

  const profiles = getSetting("profiles") ?? [];
  // DanyWhere: the profile in use, `null` while the defaults are in use;
  // `activeId` names either (`DEFAULT_PROFILE_ID` for the defaults).
  const active = activeProfile(profiles, getSetting("active_profile_id"));
  const activeId = active?.id ?? DEFAULT_PROFILE_ID;
  // DanyWhere: choosing the profile by application (`app_profiles.rs`).
  const autoByApp = getSetting("profile_auto_by_app") ?? true;
  const assigned = hasAssignments(profiles);
  // The defaults' fixed name, the same the tray and the overlay use.
  const defaultName = t("tray.defaultSettings");
  const displayName = useCallback(
    (profile: ProfileLike) => profileDisplayName(profile, defaultName),
    [defaultName],
  );

  const run = async <T>(
    action: () => Promise<Result<T, string>>,
    errorTitle: string,
    modelMayChange = false,
  ): Promise<ProfileCommandOutcome<T>> => {
    setBusy(true);
    try {
      const result = await action();
      if (result.status === "error") {
        notify({ title: errorTitle, detail: result.error });
        return { ok: false };
      }
      return { ok: true, data: result.data };
    } catch (error) {
      notify({ title: errorTitle, detail: String(error) });
      return { ok: false };
    } finally {
      await refreshSettings();
      // A profile brings its own model; the model pages read it from here.
      if (modelMayChange) await useModelStore.getState().loadCurrentModel();
      setBusy(false);
    }
  };

  const switchTo = async (id: string) => {
    if (id === activeId) return;
    await run(
      () => commands.switchProfile(id),
      t("settings.profiles.errors.switch"),
      true,
    );
  };

  const duplicate = (sourceId: string, name: string) =>
    run(
      () => commands.duplicateProfile(sourceId, name),
      t("settings.profiles.errors.duplicate"),
    );

  const rename = (id: string, name: string) =>
    run(
      () => commands.renameProfile(id, name),
      t("settings.profiles.errors.rename"),
    );

  const remove = (id: string) =>
    run(
      () => commands.deleteProfile(id),
      t("settings.profiles.errors.delete"),
      true,
    );

  const setApps = (id: string, apps: string[]) =>
    run(
      () => commands.setProfileApps(id, apps),
      t("settings.profiles.errors.apps"),
    );

  const setAutoByApp = (enabled: boolean) =>
    run(
      () => commands.setProfileAutoByApp(enabled),
      t("settings.profiles.errors.autoByApp"),
    );

  return {
    profiles,
    active,
    activeId,
    autoByApp,
    assigned,
    setApps,
    setAutoByApp,
    busy,
    defaultName,
    displayName,
    switchTo,
    duplicate,
    rename,
    remove,
  };
};
