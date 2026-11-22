/** Helpers for the profiles: names, the profile in use, what may be
 * deleted. The defaults ("Grundeinstellungen") are no profile: a profile
 * overrides them where it needs to (`profile_layers.rs`). */

export interface ProfileLike {
  id: string;
  name: string;
}

/** The id that stands for the defaults where a profile id is expected (in
 * use, switching); no profile has it (backend: `profiles.rs`). */
export const DEFAULT_PROFILE_ID = "default";

/** The name to show: the profile's own, trimmed; `defaultName` for a
 * nameless one (only "Standard" in a store from before the defaults). */
export const profileDisplayName = (
  profile: ProfileLike,
  defaultName: string,
): string => profile.name.trim() || defaultName;

/** The profile in use: the one `activeId` names; `null` while the defaults
 * are in use, also for an unknown id, where the backend puts them in use. */
export const activeProfile = <P extends ProfileLike>(
  profiles: P[],
  activeId: string | null | undefined,
): P | null => profiles.find((profile) => profile.id === activeId) ?? null;

/** Whether the sidebar shows the page "Profiles": once the settings are
 * loaded, also without any profile, since a new one is made there. */
export const profilesPageEnabled = (
  settings: { profiles?: readonly unknown[] | null } | null | undefined,
): boolean => Array.isArray(settings?.profiles);

/** Whether profile `id` may be deleted: every profile, the last one too;
 * the defaults can't. */
export const canDeleteProfile = (
  profiles: ProfileLike[],
  id: string,
): boolean =>
  id !== DEFAULT_PROFILE_ID && profiles.some((profile) => profile.id === id);

/** The profile in use once `deletedId` is gone; `null`: the defaults, which
 * the backend puts in use when the profile in use is deleted. */
export const profileAfterDelete = <P extends ProfileLike>(
  profiles: P[],
  activeId: string | null | undefined,
  deletedId: string,
): P | null => {
  const active = activeProfile(profiles, activeId);
  return active && active.id !== deletedId ? active : null;
};

export type ProfileNameCheck =
  | { kind: "ok"; name: string }
  | { kind: "empty" }
  | { kind: "unchanged" }
  | { kind: "taken" };

/** Checks a name typed for a new profile (`renaming` unset) or for profile
 * `renaming`. Names are trimmed, can't be empty and must differ, ignoring
 * case, from the names the other profiles show and from the defaults'
 * translated name (`defaultName`). */
export const checkProfileName = (
  profiles: ProfileLike[],
  draft: string,
  defaultName: string,
  renaming?: ProfileLike,
): ProfileNameCheck => {
  const name = draft.trim();
  if (renaming && name === renaming.name.trim()) return { kind: "unchanged" };
  if (!name) return { kind: "empty" };
  const lower = name.toLocaleLowerCase();
  const taken =
    defaultName.trim().toLocaleLowerCase() === lower ||
    profiles.some(
      (profile) =>
        profile.id !== renaming?.id &&
        profileDisplayName(profile, defaultName).toLocaleLowerCase() === lower,
    );
  return taken ? { kind: "taken" } : { kind: "ok", name };
};
