// DanyWhere: the tooltip of the History star (user, 2026-11-16). The star
// (`saved` in the database) keeps an entry's recording when the storage for
// recordings is full; transcripts are never deleted anyway. Handy's tooltip
// "Save transcription" came from an entry limit DanyWhere doesn't have, so
// its keys `settings.history.save` / `unsave` stay unused.

const KEYS = "settings.history.keepRecording";

/** The star's tooltip: what it does, then a line on what follows. */
export const keepRecordingTooltip = (
  saved: boolean,
  t: (key: string) => string,
): string =>
  saved
    ? `${t(`${KEYS}.keptLabel`)}\n${t(`${KEYS}.keptDescription`)}`
    : `${t(`${KEYS}.label`)}\n${t(`${KEYS}.description`)}`;
