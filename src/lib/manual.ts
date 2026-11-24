/**
 * DanyWhere: where the user manual is, and what a link inside it does.
 *
 * The manual is a static site (Astro Starlight) built into `public/manual/`
 * (`bun run build:manual`), so it ships inside the app and is served from
 * the app's own origin, matching the installed version and readable offline.
 * The settings window shows it on a page of its own (`ManualPage`), in a
 * frame. English is the root locale (`/manual/<page>/`), German, Spanish and
 * French sit under their language (`/manual/de/<page>/`). Every page's
 * address ends in `/`, which the app serves as that page's `index.html`;
 * for a file the build lacks it serves the app's own `index.html` instead.
 * Everything here is pure (manual.test.ts).
 */

/** Where the manual lives on the app's origin. */
const MANUAL_ROOT = "/manual/";

/** The languages the manual has besides English, its root locale. */
const MANUAL_LANGUAGES = ["de", "es", "fr"] as const;
type ManualLanguage = (typeof MANUAL_LANGUAGES)[number];

/** The manual's pages the app links to, by their slug. */
export const MANUAL_PAGES = {
  startAndEndShortcuts: "how-it-works/start-and-end-shortcuts/",
  defaultsAndProfiles: "how-it-works/defaults-and-profiles/",
} as const;

/** The manual's language for an interface language such as `de-AT`; none
 * for English and every language the manual lacks. */
const manualLanguage = (appLanguage: string): ManualLanguage | null => {
  const primary = appLanguage.split(/[-_]/)[0].toLowerCase();
  return MANUAL_LANGUAGES.find((language) => language === primary) ?? null;
};

const SLUG = /^[a-z0-9-]$/;
const isSlug = (text: string): boolean => [...text].every((c) => SLUG.test(c));

/** A page's slug as it goes after the language: lowercase letters, digits
 * and `-` in segments separated by single `/`, ending in `/`, and an
 * optional `#anchor` of the same letters. Anything else is none. */
const pagePath = (page: string): string | null => {
  const hash = page.indexOf("#");
  const path = hash < 0 ? page : page.slice(0, hash);
  const anchor = hash < 0 ? null : page.slice(hash + 1);
  if (
    !isSlug(path.replace(/\//g, "")) ||
    path.startsWith("/") ||
    path.includes("//")
  ) {
    return null;
  }
  if (anchor !== null && (anchor === "" || !isSlug(anchor))) return null;
  const slash = path !== "" && !path.endsWith("/") ? "/" : "";
  return `${path}${slash}${anchor === null ? "" : `#${anchor}`}`;
};

/** The language's home page of the manual, e.g. `/manual/de/`. */
const homeOf = (language: ManualLanguage | null): string =>
  language ? `${MANUAL_ROOT}${language}/` : MANUAL_ROOT;

/**
 * The address of a manual page on the app's origin, in the manual's
 * language for `appLanguage` (the interface's, e.g. `de`, `pt-BR`). `page`
 * is a page's slug such as `how-it-works/start-and-end-shortcuts/`,
 * optionally with `#anchor`; a missing or invalid one gives the home page.
 */
export const manualUrl = (
  appLanguage: string,
  page?: string | null,
): string => {
  const home = homeOf(manualLanguage(appLanguage));
  const path = page ? pagePath(page) : null;
  return path === null ? home : `${home}${path}`;
};

/**
 * The pages to show for `manualUrl(appLanguage, page)`, in order, for when
 * the build lacks one: the page itself, its language's home page, the
 * English home page, each once.
 */
export const manualCandidates = (
  appLanguage: string,
  page?: string | null,
): string[] => {
  const pages = [
    manualUrl(appLanguage, page),
    homeOf(manualLanguage(appLanguage)),
    MANUAL_ROOT,
  ];
  return pages.filter((url, index) => pages.indexOf(url) === index);
};

/** What a click on a link inside the manual does. */
export type ManualLink =
  /** A page of the manual: the frame goes there. */
  | "manual"
  /** A link leaving the app (a website, an e-mail address): the default
   * browser or mail program opens it, the manual stays where it is. */
  | "external"
  /** Anything else (the app's own pages, scripts, files): nothing. */
  | "blocked";

const parse = (text: string): URL | null => {
  try {
    return new URL(text);
  } catch {
    return null;
  }
};

/**
 * Decides a link to `url` (absolute, as the page resolved it) inside the
 * manual. `appOrigin` is the app's own address, e.g. `location.href`; its
 * scheme, host and port count. They are compared one by one: a custom
 * scheme such as `tauri:` has no origin of its own to compare by.
 */
export const manualLink = (url: string, appOrigin: string): ManualLink => {
  const link = parse(url);
  const app = parse(appOrigin);
  if (!link || !app) return "blocked";
  if (link.protocol === app.protocol && link.host === app.host) {
    // Parsed, so `/manual/../index.html` is `/index.html`.
    const path = link.pathname;
    return path === "/manual" || path.startsWith(MANUAL_ROOT)
      ? "manual"
      : "blocked";
  }
  return ["http:", "https:", "mailto:"].includes(link.protocol)
    ? "external"
    : "blocked";
};

/** Whether a link to `url` inside the manual opens in the default browser
 * (or mail program) instead (`manualLink`). */
export const isExternal = (url: string, appOrigin: string): boolean =>
  manualLink(url, appOrigin) === "external";
