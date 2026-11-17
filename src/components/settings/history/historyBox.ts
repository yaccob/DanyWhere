// DanyWhere: the pure logic of History's text box (`HistoryTextBox.tsx`):
// how tall a box may grow before it scrolls, dragging its grip, and which
// click starts editing.
import type { AppSettings, HistoryBoxKind } from "@/bindings";

/** The newest entry (the first of the list shown) and every other one. */
export type { HistoryBoxKind };

/** Lines a box shows before it scrolls, unless a height was dragged. */
export const DEFAULT_BOX_LINES: Record<HistoryBoxKind, number> = {
  newest: 15,
  other: 4,
};

/** No drag makes a box smaller than this many lines. */
export const MIN_BOX_LINES = 2;

/** The largest height stored (`MAX_HEIGHT_PX` in `history_box.rs`). */
export const MAX_BOX_HEIGHT_PX = 4000;

/** A pointer that moved farther than this between press and release
 * dragged instead of clicking. */
export const CLICK_SLOP_PX = 4;

/** A box's line height and its padding plus border (top and bottom), in
 * pixels. */
export type BoxMetrics = { lineHeight: number; chrome: number };

/** Until the box is measured: `text-sm` (20 px lines), `py-1.5` and a 1 px
 * border. */
export const FALLBACK_METRICS: BoxMetrics = { lineHeight: 20, chrome: 14 };

/** The parts of a computed style the metrics come from. */
export type BoxStyle = Pick<
  CSSStyleDeclaration,
  | "lineHeight"
  | "fontSize"
  | "paddingTop"
  | "paddingBottom"
  | "borderTopWidth"
  | "borderBottomWidth"
>;

const pixels = (value: string): number | null => {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : null;
};

/** The box's metrics from its computed style; a `normal` line height counts
 * as 1.2 times the font size. */
export const boxMetrics = (style: BoxStyle): BoxMetrics => {
  const fontSize = pixels(style.fontSize);
  const lineHeight =
    pixels(style.lineHeight) ??
    (fontSize !== null ? fontSize * 1.2 : FALLBACK_METRICS.lineHeight);
  const chrome = [
    style.paddingTop,
    style.paddingBottom,
    style.borderTopWidth,
    style.borderBottomWidth,
  ].reduce((sum, value) => sum + (pixels(value) ?? 0), 0);
  return { lineHeight, chrome };
};

export type BoxHeightSetting =
  | "history_box_height_newest"
  | "history_box_height_other";

export const boxKind = (newest: boolean): HistoryBoxKind =>
  newest ? "newest" : "other";

/** The setting that holds the dragged height of `kind`. */
export const boxHeightSetting = (kind: HistoryBoxKind): BoxHeightSetting =>
  kind === "newest" ? "history_box_height_newest" : "history_box_height_other";

/** The dragged height of `kind` in `settings`; null = its default lines. */
export const storedBoxHeight = (
  settings: Partial<Pick<AppSettings, BoxHeightSetting>> | null | undefined,
  kind: HistoryBoxKind,
): number | null => settings?.[boxHeightSetting(kind)] ?? null;

/** The height of a box that shows `lines` lines. */
export const linesHeight = (lines: number, metrics: BoxMetrics): number =>
  lines * metrics.lineHeight + metrics.chrome;

export const minBoxHeight = (metrics: BoxMetrics): number =>
  linesHeight(MIN_BOX_LINES, metrics);

/** `height` within [two lines, MAX_BOX_HEIGHT_PX], in whole pixels. */
export const clampBoxHeight = (height: number, metrics: BoxMetrics): number =>
  Math.round(
    Math.min(MAX_BOX_HEIGHT_PX, Math.max(minBoxHeight(metrics), height)),
  );

/** How tall a box of `kind` may grow before it scrolls: the dragged height
 * if there is one, else its default lines. */
export const maxBoxHeight = (
  kind: HistoryBoxKind,
  stored: number | null | undefined,
  metrics: BoxMetrics,
): number =>
  stored !== null && stored !== undefined
    ? clampBoxHeight(stored, metrics)
    : linesHeight(DEFAULT_BOX_LINES[kind], metrics);

/** The height while the grip is dragged from `startY` to `y`. */
export const dragHeight = (
  startHeight: number,
  startY: number,
  y: number,
  metrics: BoxMetrics,
): number => clampBoxHeight(startHeight + (y - startY), metrics);

/** What a release stores: the dragged height, or null when the pointer
 * didn't move (a click, or a double-click's first click). */
export const heightOnRelease = (
  startY: number,
  y: number,
  height: number,
): number | null => (y === startY ? null : height);

/** The edit field's height when editing starts: the box's height, at least
 * two lines. */
export const editBoxHeight = (
  viewHeight: number,
  metrics: BoxMetrics,
): number => clampBoxHeight(viewHeight, metrics);

/** The box's height style: shown, as tall as its text up to `maxHeight`;
 * edited, `editHeight`; while dragged, exactly the dragged height. */
export const boxSize = (state: {
  editing: boolean;
  dragged: number | null;
  maxHeight: number;
  editHeight: number;
}): { height?: number; maxHeight?: number } => {
  if (state.dragged !== null) return { height: state.dragged };
  if (state.editing) return { height: state.editHeight };
  return { maxHeight: state.maxHeight };
};

/** Whether a click on the shown text starts editing: a plain click that
 * selected nothing, not a drag that selected text or a click on the
 * scroll bar. */
export const activatesEditing = (click: {
  button: number;
  collapsed: boolean;
  distance: number;
  onScrollbar: boolean;
}): boolean =>
  click.button === 0 &&
  click.collapsed &&
  click.distance <= CLICK_SLOP_PX &&
  !click.onScrollbar;

/** Whether the hidden edit button takes the focus when editing ends: when a key
 * ended it (Escape or ⌘↩ in the edit field, Enter on Save or Cancel) and
 * the focus went nowhere with the edit field and its buttons; never away
 * from where the user went on, and not after a click, which would leave a
 * focus ring around the box. */
export const returnsFocusAfterEditing = (state: {
  endedEditing: boolean;
  nothingFocused: boolean;
  byKey: boolean;
}): boolean => state.endedEditing && state.nothingFocused && state.byKey;

/** The scroll position that shows a part at `top` with `height` (both
 * within the box's content), centred; null if it shows already. */
export const scrollToShow = (part: {
  top: number;
  height: number;
  scrollTop: number;
  clientHeight: number;
}): number | null => {
  const shown =
    part.top >= part.scrollTop &&
    part.top + part.height <= part.scrollTop + part.clientHeight;
  if (shown) return null;
  return Math.max(
    0,
    Math.round(part.top - (part.clientHeight - part.height) / 2),
  );
};
