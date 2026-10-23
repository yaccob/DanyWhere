/**
 * Parses a duration in seconds as typed by the user ("2.5" or "2,5") into
 * whole milliseconds, rounded to tenths of a second. Returns null for input
 * that is not a number or lies outside [min, max] seconds.
 */
export const parseSecondsToMs = (
  input: string,
  min: number,
  max: number,
): number | null => {
  const normalized = input.trim().replace(",", ".");
  if (!/^\d+(\.\d+)?$/.test(normalized)) return null;
  const seconds = Number(normalized);
  if (seconds < min || seconds > max) return null;
  return Math.round(seconds * 10) * 100;
};

/** Formats milliseconds as seconds with at most one decimal, per locale. */
export const formatMsAsSeconds = (ms: number, locale: string): string =>
  new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(ms / 1000);
