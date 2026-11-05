import type { RecordingStorageLimit, RecordingStorageUsage } from "@/bindings";

const MB = 1_000_000;
const GB = 1_000_000_000;

const LIMIT_BYTES: Record<RecordingStorageLimit, number | null> = {
  mb_100: 100 * MB,
  mb_500: 500 * MB,
  gb_1: GB,
  gb_5: 5 * GB,
  unlimited: null,
};

/** The storage limits for History's recordings, smallest first. */
export const STORAGE_LIMITS: RecordingStorageLimit[] = [
  "mb_100",
  "mb_500",
  "gb_1",
  "gb_5",
  "unlimited",
];

/** The limit in bytes; `null` when unlimited. */
export const storageLimitBytes = (
  limit: RecordingStorageLimit,
): number | null => LIMIT_BYTES[limit];

/** A size in MB or GB as Finder counts them (1 MB = 1,000,000 bytes). */
export const formatStorageSize = (bytes: number, locale: string): string => {
  const gigabytes = bytes >= GB;
  const value = gigabytes ? bytes / GB : bytes / MB;
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: gigabytes ? "gigabyte" : "megabyte",
    unitDisplay: "short",
    maximumFractionDigits: value < 10 ? 1 : 0,
  }).format(value);
};

/** Starred entries and transcribed files alone take more than the limit. */
export const protectedExceedsLimit = (
  usage: RecordingStorageUsage,
  limit: RecordingStorageLimit,
): boolean => {
  const bytes = storageLimitBytes(limit);
  return bytes !== null && usage.protected_bytes > bytes;
};
