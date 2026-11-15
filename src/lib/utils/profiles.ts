/** Helpers for the profiles: names, the active profile, what may be deleted. */

export interface ProfileLike {
  id: string;
  name: string;
}

/** The id of the profile every store starts with (backend: `profiles.rs`). */
export const DEFAULT_PROFILE_ID = "default";

/** The name to show. The default profile is stored without a name, so it
 * shows `defaultName` (the translated "Default") until it is renamed. */
export const profileDisplayName = (
  profile: ProfileLike,
  defaultName: string,
): string => profile.name.trim() || defaultName;

/** The profile in use: the one `activeId` names, else the first, as the
 * backend falls back. */
export const activeProfile = <P extends ProfileLike>(
  profiles: P[],
  activeId: string | null | undefined,
): P | null =>
  profiles.find((profile) => profile.id === activeId) ?? profiles[0] ?? null;

/** Whether the sidebar shows the page "Profiles": once the settings carry
 * profiles, from one on (it can be duplicated or renamed there). */
export const profilesPageEnabled = (
  settings: { profiles?: readonly unknown[] | null } | null | undefined,
): boolean => (settings?.profiles?.length ?? 0) > 0;

/** Whether profile `id` may be deleted: it exists and isn't the last one. */
export const canDeleteProfile = (
  profiles: ProfileLike[],
  id: string,
): boolean =>
  profiles.length > 1 && profiles.some((profile) => profile.id === id);

/** The profile in use once `deletedId` is gone: the backend switches from a
 * deleted active profile to the first other one first. */
export const profileAfterDelete = <P extends ProfileLike>(
  profiles: P[],
  activeId: string | null | undefined,
  deletedId: string,
): P | null => {
  const active = activeProfile(profiles, activeId);
  if (active && active.id !== deletedId) return active;
  return profiles.find((profile) => profile.id !== deletedId) ?? null;
};

export type ProfileNameCheck =
  | { kind: "ok"; name: string }
  | { kind: "empty" }
  | { kind: "unchanged" }
  | { kind: "taken" };

/** Checks a name typed for a duplicate (`renaming` unset) or for profile
 * `renaming`. Names are trimmed and must differ, ignoring case, from the
 * names the other profiles show. Only the default profile may be left
 * without a name, which shows the translated name again. */
export const checkProfileName = (
  profiles: ProfileLike[],
  draft: string,
  defaultName: string,
  renaming?: ProfileLike,
): ProfileNameCheck => {
  const name = draft.trim();
  if (renaming && name === renaming.name.trim()) return { kind: "unchanged" };
  if (!name) {
    return renaming?.id === DEFAULT_PROFILE_ID
      ? { kind: "ok", name }
      : { kind: "empty" };
  }
  const lower = name.toLocaleLowerCase();
  const taken = profiles.some(
    (profile) =>
      profile.id !== renaming?.id &&
      profileDisplayName(profile, defaultName).toLocaleLowerCase() === lower,
  );
  return taken ? { kind: "taken" } : { kind: "ok", name };
};
