/**
 * DanyWhere: a menu opened at a button stays inside the window. It opens
 * at the button's start edge in the reading direction and moves back
 * towards the start just as far as it must (qa 2026-11-22_0107, error 5:
 * History's version menu ran up to 110 px past the right edge).
 */

/** The gap a menu keeps to the window's edges, in pixels. */
export const MENU_MARGIN = 8;

export interface MenuPlacementInput {
  /** The anchor's edges in the window (`getBoundingClientRect`); the menu
   * is placed relative to the anchor's left edge. */
  anchorLeft: number;
  anchorRight: number;
  menuWidth: number;
  viewportWidth: number;
  rtl: boolean;
}

/** Where the menu's left edge goes, in pixels from the anchor's left edge:
 * left-aligned with the anchor (right-aligned right to left) when it fits,
 * else shifted until it keeps `MENU_MARGIN` from both edges; a menu wider
 * than the window starts at the margin. */
export const menuOffset = ({
  anchorLeft,
  anchorRight,
  menuWidth,
  viewportWidth,
  rtl,
}: MenuPlacementInput): number => {
  const wanted = rtl ? anchorRight - menuWidth : anchorLeft;
  const last = viewportWidth - MENU_MARGIN - menuWidth;
  const left = Math.max(MENU_MARGIN, Math.min(wanted, last));
  return left - anchorLeft;
};
