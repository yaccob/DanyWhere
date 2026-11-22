/**
 * DanyWhere: the jump from a profile's page "Profile" to one of its own
 * values (qa 2026-11-22_1603, B5): the value's page opens, its setting (the
 * profile wrapper around it, `ScopedSetting`) scrolls into view and lights
 * up briefly. The rules are here, pure, so they can be checked without a
 * window (settingJump.test.ts); `scope/settingJumpStore.ts` holds the
 * pending jump and `scope/useJumpHighlight.ts` scrolls and lights up.
 */
import type { OwnValue } from "./profileScope";

/** The setting a jump goes to: a field of a profile, or an entry of it. */
export interface JumpTarget {
  profileId: string;
  field: string;
  entry?: string;
}

/** A jump asked for, waiting for its setting to show. */
export interface PendingJump extends JumpTarget {
  /** When it was asked for (ms, `Date.now()`). */
  requestedAt: number;
}

/** How long a jump waits for its setting. The page opens at once, but a
 * setting may show only once its data is there. A jump whose setting
 * doesn't show by then (a page without it, a setting this device doesn't
 * draw, the selected provider being another) has just opened the page, as
 * before, and a later visit of the page doesn't jump. */
export const JUMP_WAIT_MS = 2000;

/** How long the setting stays lit after the jump. */
export const HIGHLIGHT_MS = 1500;

/** Where a row of the page "Profile" jumps to: the own value's field, or
 * its entry; a switched-off entry like an own one. */
export const jumpTarget = (profileId: string, value: OwnValue): JumpTarget =>
  value.entry === undefined
    ? { profileId, field: value.field }
    : { profileId, field: value.field, entry: value.entry };

/**
 * Whether the wrapper of `fields` (or of their entry `entry`) in profile
 * `profileId` is where `target` goes. A wrapper of one entry (a shortcut,
 * the shown provider's model) is the target of that entry only; a wrapper
 * of a whole list (the end shortcuts, the word lists) of each of its
 * entries; a wrapper of several fields of each of them.
 */
export const isJumpTarget = (
  target: JumpTarget,
  profileId: string,
  fields: readonly string[],
  entry?: string,
): boolean =>
  target.profileId === profileId &&
  fields.includes(target.field) &&
  (entry === undefined || target.entry === undefined || target.entry === entry);

/** Whether `jump` still waits for its setting at `now`. */
export const jumpWaits = (jump: PendingJump, now: number): boolean => {
  const waited = now - jump.requestedAt;
  return waited >= 0 && waited <= JUMP_WAIT_MS;
};

/** Whether the wrapper of `fields` (`entry`) in `profileId` takes `jump`
 * at `now`; the store then drops it, so the first such wrapper of the page
 * (in document order) is the only one. */
export const takesJump = (
  jump: PendingJump | null,
  now: number,
  profileId: string,
  fields: readonly string[],
  entry?: string,
): boolean =>
  jump !== null &&
  jumpWaits(jump, now) &&
  isJumpTarget(jump, profileId, fields, entry);

/** Where the setting lands in the scrolling area: in the middle when it
 * fits, else with its top at the top, so its title shows. */
export const scrollBlock = (
  height: number,
  viewportHeight: number,
): "center" | "start" => (height <= viewportHeight ? "center" : "start");

/** Smooth scrolling, unless the system asks for less motion. */
export const scrollBehavior = (reducedMotion: boolean): "smooth" | "auto" =>
  reducedMotion ? "auto" : "smooth";
