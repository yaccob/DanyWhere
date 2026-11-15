// DanyWhere: the box a History entry shows its text in, and edits it in.
// As tall as its text up to a maximum per kind of entry (the newest, the
// others); beyond it the box scrolls inside, under the pointer, and the list
// scrolls on once the box reaches its end. The grip at its bottom edge sets
// that maximum for every box of the kind; a double-click on it goes back to
// the default. The logic lives in `historyBox.ts`.
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@/stores/settingsStore";
import { Highlighted } from "./HistorySearchBar";
import {
  FALLBACK_METRICS,
  activatesEditing,
  boxHeightSetting,
  boxKind,
  boxMetrics,
  boxSize,
  clampBoxHeight,
  dragHeight,
  editBoxHeight,
  heightOnRelease,
  maxBoxHeight,
  scrollToShow,
} from "./historyBox";

export interface HistoryTextBoxProps {
  /** The text shown while not editing. */
  text: string;
  /** Where the search matches `text` (`matchRanges`); the box scrolls the
   * first match into view when it is hidden. */
  highlights: [number, number][];
  /** The first entry of the list shown: its own default (15 lines instead
   * of 4) and its own dragged height. */
  newest: boolean;
  /** Edit mode: a textarea in the same box, at the box's height. */
  editing?: boolean;
  /** The textarea's text while editing. */
  draft?: string;
  onDraftChange?: (draft: string) => void;
  /** Keys in the textarea (e.g. Escape, ⌘↩). */
  onKeyDown?: (event: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  /** A click on the shown text that selected nothing (a drag that selects
   * text doesn't count). `caret` is where in `text` it was clicked; editing
   * that starts right after it puts the caret there. */
  onActivate?: (caret: number) => void;
  /** The textarea's accessible name. */
  editLabel?: string;
  /** Classes added to the text and the textarea, e.g. `italic`. */
  className?: string;
}

/** Where in `container`'s text the point `node`/`offset` lies. */
const textOffset = (
  container: HTMLElement,
  node: Node | null,
  offset: number,
): number | null => {
  if (!node || !container.contains(node)) return null;
  const range = document.createRange();
  range.selectNodeContents(container);
  range.setEnd(node, offset);
  return range.toString().length;
};

/** Where in `container`'s text a click at `x`/`y` put the caret. */
const clickedOffset = (
  container: HTMLElement,
  x: number,
  y: number,
): number | null => {
  const selection = window.getSelection();
  if (selection && selection.rangeCount > 0) {
    const found = textOffset(
      container,
      selection.focusNode,
      selection.focusOffset,
    );
    if (found !== null) return found;
  }
  const point = document.caretRangeFromPoint?.(x, y);
  return point
    ? textOffset(container, point.startContainer, point.startOffset)
    : null;
};

const BOX =
  "relative block w-full overflow-y-auto rounded-md border px-2 py-1.5 text-sm text-text whitespace-pre-wrap break-words";

export const HistoryTextBox: React.FC<HistoryTextBoxProps> = ({
  text,
  highlights,
  newest,
  editing = false,
  draft = "",
  onDraftChange,
  onKeyDown,
  onActivate,
  editLabel,
  className = "",
}) => {
  const { t } = useTranslation();
  const kind = boxKind(newest);
  const setting = boxHeightSetting(kind);
  const stored = useSettingsStore((state) => state.settings?.[setting] ?? null);
  const updateSetting = useSettingsStore((state) => state.updateSetting);

  const viewRef = useRef<HTMLDivElement>(null);
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const [metrics, setMetrics] = useState(FALLBACK_METRICS);
  // The height while the grip is dragged; null otherwise.
  const [dragged, setDragged] = useState<number | null>(null);
  const drag = useRef<{ startY: number; startHeight: number } | null>(null);
  // The shown box's last height and the scroll position of either field,
  // so editing starts at the same height and place and returns there.
  const viewHeight = useRef(0);
  const scrollTop = useRef(0);
  const pendingCaret = useRef<number | null>(null);
  const press = useRef<{ x: number; y: number; onScrollbar: boolean } | null>(
    null,
  );

  // The edit field's height, set when editing starts (during render, so
  // the textarea never shows at another height). A box that starts out
  // editing has no shown height yet and takes its kind's maximum.
  const [wasEditing, setWasEditing] = useState(editing);
  const [editHeight, setEditHeight] = useState(() =>
    maxBoxHeight(kind, stored, FALLBACK_METRICS),
  );
  if (editing !== wasEditing) {
    setWasEditing(editing);
    if (editing) setEditHeight(editBoxHeight(viewHeight.current, metrics));
  }

  const maxHeight = maxBoxHeight(kind, stored, metrics);
  const size = boxSize({ editing, dragged, maxHeight, editHeight });

  useLayoutEffect(() => {
    const element = viewRef.current ?? areaRef.current;
    if (!element) return;
    const measured = boxMetrics(getComputedStyle(element));
    setMetrics((previous) =>
      previous.lineHeight === measured.lineHeight &&
      previous.chrome === measured.chrome
        ? previous
        : measured,
    );
  }, []);

  // Each field takes over the other's scroll position; the edit field
  // also the focus and the caret where the text was clicked.
  useLayoutEffect(() => {
    if (editing) {
      const area = areaRef.current;
      if (!area) return;
      area.scrollTop = scrollTop.current;
      const caret = Math.min(
        pendingCaret.current ?? area.value.length,
        area.value.length,
      );
      pendingCaret.current = null;
      area.focus({ preventScroll: true });
      area.setSelectionRange(caret, caret);
    } else if (viewRef.current) {
      viewRef.current.scrollTop = scrollTop.current;
    }
  }, [editing]);

  useLayoutEffect(() => {
    const view = viewRef.current;
    if (!view || editing) return;
    const measure = () => {
      viewHeight.current = view.getBoundingClientRect().height;
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(view);
    return () => observer.disconnect();
  }, [editing]);

  // A search match the box hides is scrolled into view.
  useLayoutEffect(() => {
    const view = viewRef.current;
    if (!view || editing || highlights.length === 0) return;
    const mark = view.querySelector("mark");
    if (!mark) return;
    const target = scrollToShow({
      top: mark.offsetTop,
      height: mark.offsetHeight,
      scrollTop: view.scrollTop,
      clientHeight: view.clientHeight,
    });
    if (target !== null) view.scrollTop = target;
  }, [highlights, editing]);

  // A box that goes away mid-drag (the list changed) gives the cursor back.
  useEffect(
    () => () => {
      if (drag.current) document.body.style.removeProperty("cursor");
    },
    [],
  );

  const field = () => (editing ? areaRef.current : viewRef.current);

  const endDrag = () => {
    drag.current = null;
    setDragged(null);
    document.body.style.removeProperty("cursor");
  };

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    const element = field();
    if (event.button !== 0 || !element) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      startY: event.clientY,
      startHeight: element.getBoundingClientRect().height,
    };
    document.body.style.cursor = "ns-resize";
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current) return;
    setDragged(
      dragHeight(current.startHeight, current.startY, event.clientY, metrics),
    );
  };

  const handlePointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current) return;
    const height = heightOnRelease(
      current.startY,
      event.clientY,
      dragHeight(current.startHeight, current.startY, event.clientY, metrics),
    );
    endDrag();
    if (height === null) return;
    if (editing) setEditHeight(height);
    void updateSetting(setting, height);
  };

  // A double-click puts the kind back to its default. Read from the click's
  // count: `pointerdown` is cancelled (no text selection, the edit field
  // keeps the focus), which may cost the `dblclick` event, never `click`.
  const handleClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (event.detail !== 2) return;
    void updateSetting(setting, null);
    const area = areaRef.current;
    if (editing && area) {
      // The edit field shrinks to its text, up to the default lines.
      const borders = area.offsetHeight - area.clientHeight;
      setEditHeight(
        clampBoxHeight(
          Math.min(
            maxBoxHeight(kind, null, metrics),
            area.scrollHeight + borders,
          ),
          metrics,
        ),
      );
    }
  };

  const handleMouseDown = (event: React.MouseEvent<HTMLDivElement>) => {
    const view = event.currentTarget;
    const rect = view.getBoundingClientRect();
    press.current = {
      x: event.clientX,
      y: event.clientY,
      // Right of or below the content: a classic scroll bar.
      onScrollbar:
        event.clientX - rect.left >= view.clientLeft + view.clientWidth ||
        event.clientY - rect.top >= view.clientTop + view.clientHeight,
    };
  };

  const handleMouseUp = (event: React.MouseEvent<HTMLDivElement>) => {
    const pressed = press.current;
    press.current = null;
    if (!pressed || !onActivate) return;
    const selection = window.getSelection();
    const click = {
      button: event.button,
      collapsed: !selection || selection.isCollapsed,
      distance: Math.hypot(
        event.clientX - pressed.x,
        event.clientY - pressed.y,
      ),
      onScrollbar: pressed.onScrollbar,
    };
    if (!activatesEditing(click)) return;
    const view = event.currentTarget;
    const caret =
      clickedOffset(view, event.clientX, event.clientY) ?? text.length;
    pendingCaret.current = caret;
    scrollTop.current = view.scrollTop;
    viewHeight.current = view.getBoundingClientRect().height;
    onActivate(caret);
  };

  const gripTitle =
    kind === "newest"
      ? t("settings.history.box.resizeNewest")
      : t("settings.history.box.resizeOther");

  return (
    <div className="group/box relative w-full">
      {editing ? (
        <textarea
          ref={areaRef}
          value={draft}
          onChange={(event) => onDraftChange?.(event.target.value)}
          onKeyDown={onKeyDown}
          onScroll={(event) => {
            scrollTop.current = event.currentTarget.scrollTop;
          }}
          aria-label={editLabel}
          style={size}
          className={`${BOX} resize-none bg-wash border-logo-primary focus:outline-none ${className}`}
        />
      ) : (
        <div
          ref={viewRef}
          onMouseDown={handleMouseDown}
          onMouseUp={handleMouseUp}
          onScroll={(event) => {
            scrollTop.current = event.currentTarget.scrollTop;
          }}
          style={size}
          className={`${BOX} select-text cursor-text border-line hover:border-line-strong transition-colors ${className}`}
        >
          <Highlighted text={text} ranges={highlights} />
        </div>
      )}
      <div
        role="separator"
        aria-orientation="horizontal"
        aria-label={gripTitle}
        title={gripTitle}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={endDrag}
        onClick={handleClick}
        className={`group/grip absolute inset-x-0 -bottom-1 h-2.5 flex items-center justify-center cursor-ns-resize touch-none transition-opacity ${
          dragged !== null
            ? "opacity-100"
            : "opacity-0 group-hover/box:opacity-100"
        }`}
      >
        <span
          className={`h-1 w-10 rounded-full transition-colors ${
            dragged !== null
              ? "bg-logo-primary"
              : "bg-line-strong group-hover/grip:bg-logo-primary"
          }`}
        />
      </div>
    </div>
  );
};
