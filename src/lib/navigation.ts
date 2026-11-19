/**
 * DanyWhere: where the settings window is. The sidebar is a tree of three
 * areas (qa 2026-11-19_1711 P3, P6, P11): the defaults, the profile settings
 * and the application settings, then History and About. Everything here is
 * pure, so it can be checked without a window (navigation.test.ts).
 */
import { DEFAULT_PROFILE_ID } from "./utils/profiles";

/** The pages of the defaults: today's pages minus the application settings. */
export const DEFAULTS_PAGES = [
  "general",
  "models",
  "postprocessing",
  "advanced",
] as const;
export type DefaultsPage = (typeof DEFAULTS_PAGES)[number];

/** A profile's pages: "Profile" first, then the same pages as the defaults. */
export const PROFILE_PAGES = ["profile", ...DEFAULTS_PAGES] as const;
export type ProfilePage = (typeof PROFILE_PAGES)[number];

/** The pages of the settings that hold for the application, not a dictation. */
export const APPLICATION_PAGES = [
  "general",
  "updates",
  "storage",
  "accessibility",
  "debug",
] as const;
export type ApplicationPage = (typeof APPLICATION_PAGES)[number];

export type NavArea =
  | "defaults"
  | "profiles"
  | "application"
  | "history"
  | "about";

/** What the content area shows. */
export type NavLocation =
  | { kind: "defaults"; page: DefaultsPage }
  /** The overview of all profiles. */
  | { kind: "profiles" }
  | { kind: "profile"; profileId: string; page: ProfilePage }
  | { kind: "application"; page: ApplicationPage }
  | { kind: "history" }
  | { kind: "about" };

/** Where the window opens. */
export const START_LOCATION: NavLocation = {
  kind: "defaults",
  page: "general",
};

/** The part of the settings the tree depends on. */
export interface NavSettings {
  profiles?:
    | readonly {
        id: string;
        values?: { post_process_enabled?: boolean | null } | null;
      }[]
    | null;
  post_process_enabled?: boolean | null;
  debug_mode?: boolean | null;
}

/** A profile as the tree needs it. */
export interface NavProfile {
  id: string;
  postProcessEnabled: boolean;
}

export interface NavContext {
  /** Whether the area "Profile settings" shows: the settings carry profiles. */
  profilesEnabled: boolean;
  /** The profiles listed under the profile settings, in stored order. */
  profiles: NavProfile[];
  /** Post-processing is on in the settings the defaults' pages edit. */
  postProcessEnabled: boolean;
  debugMode: boolean;
}

/** A label: a translation key, or a profile whose name the caller shows. */
export type NavLabel = { key: string } | { profileId: string };

export type NavIcon = NavArea;

/** One row of the tree. */
export interface NavItem {
  /** Unique among the rows, usable as a React key. */
  id: string;
  /** 0 = an area (or History, About), 1 = its page or profile, 2 = a profile's page. */
  level: 0 | 1 | 2;
  label: NavLabel;
  /** What a click on the row shows. */
  target: NavLocation;
  /** The row of what the content area shows. */
  current: boolean;
  /** A profile row whose pages are listed under it. */
  open?: boolean;
  /** Areas, History and About carry an icon. */
  icon?: NavIcon;
}

/** The tree's view of the settings. The profile with the default id is the
 * defaults themselves since "Default" became them (P1), so it is no profile
 * under the profile settings; the overview still lists every profile. */
export const navContext = (
  settings: NavSettings | null | undefined,
): NavContext => {
  const stored = settings?.profiles ?? [];
  return {
    profilesEnabled: stored.length > 0,
    profiles: stored
      .filter((profile) => profile.id !== DEFAULT_PROFILE_ID)
      .map((profile) => ({
        id: profile.id,
        postProcessEnabled: profile.values?.post_process_enabled === true,
      })),
    postProcessEnabled: settings?.post_process_enabled === true,
    debugMode: settings?.debug_mode === true,
  };
};

/** The defaults' pages; Post Process only while post-processing is on, as
 * the sidebar did before. */
export const defaultsPages = (ctx: NavContext): DefaultsPage[] =>
  DEFAULTS_PAGES.filter(
    (page) => page !== "postprocessing" || ctx.postProcessEnabled,
  );

/** A profile's pages; its Post Process only while its own post-processing
 * is on. */
export const profilePages = (profile: NavProfile): ProfilePage[] =>
  PROFILE_PAGES.filter(
    (page) => page !== "postprocessing" || profile.postProcessEnabled,
  );

/** The application settings' pages; Debug only in debug mode, as before. */
export const applicationPages = (ctx: NavContext): ApplicationPage[] =>
  APPLICATION_PAGES.filter((page) => page !== "debug" || ctx.debugMode);

const PAGE_LABELS: Record<
  DefaultsPage | ProfilePage | ApplicationPage,
  string
> = {
  general: "sidebar.general",
  models: "sidebar.models",
  postprocessing: "sidebar.postProcessing",
  advanced: "sidebar.advanced",
  profile: "sidebar.profile",
  updates: "sidebar.updates",
  storage: "sidebar.storage",
  // The page is named after the macOS permission it is about.
  accessibility: "onboarding.permissions.accessibility.name",
  debug: "sidebar.debug",
};

/** The translation key of a page's label; a page reads the same in every
 * area ("General" of the defaults and of the application). */
export const pageLabelKey = (
  page: DefaultsPage | ProfilePage | ApplicationPage,
): string => PAGE_LABELS[page];

const AREA_LABELS: Record<NavArea, string> = {
  defaults: "sidebar.areas.defaults",
  profiles: "sidebar.areas.profiles",
  application: "sidebar.areas.application",
  history: "sidebar.history",
  about: "sidebar.about",
};

export const areaLabelKey = (area: NavArea): string => AREA_LABELS[area];

/** A string that tells places apart, e.g. "profile/work/models". */
export const locationKey = (location: NavLocation): string => {
  switch (location.kind) {
    case "defaults":
    case "application":
      return `${location.kind}/${location.page}`;
    case "profile":
      return `profile/${location.profileId}/${location.page}`;
    default:
      return location.kind;
  }
};

export const sameLocation = (a: NavLocation, b: NavLocation): boolean =>
  locationKey(a) === locationKey(b);

/** What a click on an area's row (or History, About) shows: its first page,
 * or for the profile settings the overview of all profiles. */
export const selectArea = (area: NavArea): NavLocation => {
  switch (area) {
    case "defaults":
      return { kind: "defaults", page: DEFAULTS_PAGES[0] };
    case "profiles":
      return { kind: "profiles" };
    case "application":
      return { kind: "application", page: APPLICATION_PAGES[0] };
    case "history":
      return { kind: "history" };
    case "about":
      return { kind: "about" };
  }
};

/** What a click on a profile's row shows. From another profile's page it
 * keeps the page, so two profiles can be compared page by page; otherwise,
 * or when the profile lacks that page, its page "Profile". */
export const selectProfile = (
  profileId: string,
  current: NavLocation,
  ctx: NavContext,
): NavLocation => {
  const profile = ctx.profiles.find((p) => p.id === profileId);
  const keep =
    current.kind === "profile" &&
    profile !== undefined &&
    profilePages(profile).includes(current.page);
  return {
    kind: "profile",
    profileId,
    page: keep ? current.page : "profile",
  };
};

/** What the window shows for `location` now. A page that is gone (Post
 * Process without post-processing, Debug without debug mode) gives way to
 * its area's first page; a removed profile to the overview of all profiles;
 * the profile settings without profiles to the start. */
export const resolveLocation = (
  location: NavLocation,
  ctx: NavContext,
): NavLocation => {
  switch (location.kind) {
    case "defaults":
      return defaultsPages(ctx).includes(location.page)
        ? location
        : selectArea("defaults");
    case "profiles":
      return ctx.profilesEnabled ? location : START_LOCATION;
    case "profile": {
      if (!ctx.profilesEnabled) return START_LOCATION;
      const profile = ctx.profiles.find((p) => p.id === location.profileId);
      if (!profile) return { kind: "profiles" };
      return profilePages(profile).includes(location.page)
        ? location
        : { kind: "profile", profileId: profile.id, page: "profile" };
    }
    case "application":
      return applicationPages(ctx).includes(location.page)
        ? location
        : selectArea("application");
    default:
      return location;
  }
};

/** The rows of the sidebar's tree for `location` (already resolved). */
export const navTree = (ctx: NavContext, location: NavLocation): NavItem[] => {
  const shown = locationKey(location);
  const page = (target: NavLocation, label: string, level: 1 | 2): NavItem => ({
    id: locationKey(target),
    level,
    label: { key: label },
    target,
    current: locationKey(target) === shown,
  });
  const area = (which: NavArea): NavItem => {
    const target = selectArea(which);
    return {
      id: which,
      level: 0,
      label: { key: areaLabelKey(which) },
      target,
      // An area whose click shows a page of its own (the overview, History,
      // About) is current then; Defaults and Application settings never are,
      // their page is.
      current:
        (which === "profiles" || which === "history" || which === "about") &&
        locationKey(target) === shown,
      icon: which,
    };
  };

  const items: NavItem[] = [area("defaults")];
  for (const p of defaultsPages(ctx)) {
    items.push(page({ kind: "defaults", page: p }, pageLabelKey(p), 1));
  }
  if (ctx.profilesEnabled) {
    items.push(area("profiles"));
    for (const profile of ctx.profiles) {
      const open =
        location.kind === "profile" && location.profileId === profile.id;
      items.push({
        id: `profile/${profile.id}`,
        level: 1,
        label: { profileId: profile.id },
        target: selectProfile(profile.id, location, ctx),
        current: false,
        open,
      });
      if (!open) continue;
      for (const p of profilePages(profile)) {
        items.push(
          page(
            { kind: "profile", profileId: profile.id, page: p },
            pageLabelKey(p),
            2,
          ),
        );
      }
    }
  }
  items.push(area("application"));
  for (const p of applicationPages(ctx)) {
    items.push(page({ kind: "application", page: p }, pageLabelKey(p), 1));
  }
  items.push(area("history"), area("about"));
  return items;
};

/** The path above a page: its area, the profile, the page. */
export const navPath = (location: NavLocation): NavLabel[] => {
  switch (location.kind) {
    case "defaults":
      return [
        { key: areaLabelKey("defaults") },
        { key: pageLabelKey(location.page) },
      ];
    case "profiles":
      return [{ key: areaLabelKey("profiles") }];
    case "profile":
      return [
        { key: areaLabelKey("profiles") },
        { profileId: location.profileId },
        { key: pageLabelKey(location.page) },
      ];
    case "application":
      return [
        { key: areaLabelKey("application") },
        { key: pageLabelKey(location.page) },
      ];
    case "history":
      return [{ key: areaLabelKey("history") }];
    case "about":
      return [{ key: areaLabelKey("about") }];
  }
};

/** The Models page of the defaults fills the window: its table scrolls, not
 * the page. */
export const fillsWindow = (location: NavLocation): boolean =>
  location.kind === "defaults" && location.page === "models";
