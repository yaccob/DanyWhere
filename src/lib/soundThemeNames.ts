/** DanyWhere: the names of the sound themes, as Handy's sound picker lists
 * them; the page "Profile" and the state lines show a profile's theme by
 * the same name (qa 2026-11-22_1731). Marimba and Pop are names and stay
 * as they are; the user's own sounds are called in the interface's
 * language (qa 2026-11-22_1852). */
export const SOUND_THEME_NAMES: Readonly<Record<string, string>> = {
  marimba: "Marimba",
  pop: "Pop",
};

/** A sound theme's name as the interface shows it; a theme this version
 * doesn't know by its stored value. */
export const soundThemeName = (
  theme: string,
  t: (key: string) => string,
): string =>
  theme === "custom"
    ? t("settings.debug.soundTheme.custom")
    : (SOUND_THEME_NAMES[theme] ?? theme);
