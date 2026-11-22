/** DanyWhere: helpers for assigning applications to profiles
 * (backend: `app_profiles.rs`). */

export interface ProfileWithApps {
  id: string;
  name: string;
  /** Optional in the bindings: a stored profile may lack it. */
  apps?: string[];
}

/** The applications of `profile`. */
export const appsOf = (profile: ProfileWithApps): string[] =>
  profile.apps ?? [];

export interface ApplicationLike {
  id: string;
  name: string;
  running: boolean;
}

/** Whether any profile has applications assigned; only then does choosing
 * the profile by application do anything. */
export const hasAssignments = (profiles: ProfileWithApps[]): boolean =>
  profiles.some((profile) => appsOf(profile).length > 0);

/** The profile other than `exceptId` that application `appId` is assigned
 * to, if any: assigning it here moves it away from there. */
export const ownerOfApp = <P extends ProfileWithApps>(
  profiles: P[],
  appId: string,
  exceptId: string,
): P | null =>
  profiles.find(
    (profile) => profile.id !== exceptId && appsOf(profile).includes(appId),
  ) ?? null;

/** `apps` with `appId` added at the end, trimmed, once. */
export const withApp = (apps: string[], appId: string): string[] => {
  const id = appId.trim();
  if (!id || apps.includes(id)) return apps;
  return [...apps, id];
};

/** `apps` without `appId`. */
export const withoutApp = (apps: string[], appId: string): string[] =>
  apps.filter((app) => app !== appId);

/** The applications that can still be added to a profile that has
 * `assigned`, in the backend's order (running first). */
export const appChoices = <A extends ApplicationLike>(
  applications: A[],
  assigned: string[],
): A[] => applications.filter((app) => !assigned.includes(app.id));

/** The name to show for `appId`: the listed application's, else the id. */
export const appLabel = (
  appId: string,
  applications: ApplicationLike[],
): string => applications.find((app) => app.id === appId)?.name ?? appId;

/** The applications an import left out (`dropped_apps`: another profile here
 * is already used for them), grouped by that profile, in the profiles'
 * order. */
export const droppedByOwner = <P extends ProfileWithApps>(
  dropped: string[],
  profiles: P[],
): { profile: P; apps: string[] }[] =>
  profiles
    .map((profile) => ({
      profile,
      apps: dropped.filter((app) => appsOf(profile).includes(app)),
    }))
    .filter(({ apps }) => apps.length > 0);

/** What stands before the applications of a profile: "Used automatically
 * in:" before them, and a sentence of its own while there are none. */
export const appsCaption = (assigned: readonly string[]): string =>
  assigned.length === 0
    ? "settings.profiles.apps.none"
    : "settings.profiles.apps.label";
