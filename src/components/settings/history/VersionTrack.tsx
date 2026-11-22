// DanyWhere: the track under a History entry with more than one version:
// one chip per version, bottom to top (Transcription → ✨ → ✎ …); the last
// one is the top, the entry's result. A click shows a version as the
// current one. All versions are alike (user, 2026-11-19, H3): a chip is
// dragged to another place, and the current version's menu moves it to the
// top, up or down (for the keyboard) or deletes it, the transcription too,
// as long as one is left. Nothing asks first (H4). Dragging uses pointer
// events: Tauri's `dragDropEnabled` turns HTML5 drag and drop off on Windows.
import React, {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowUpToLine,
  ChevronDown,
  ChevronRight,
  Ellipsis,
  Pencil,
  WandSparkles,
} from "lucide-react";
import type { HistoryEntry, HistoryVersion, VersionStep } from "@/bindings";
import { formatDateTime } from "@/utils/dateFormat";
import { cancelOnEscape } from "@/lib/escapeLayers";
import { menuOffset } from "@/lib/menuPlacement";
import {
  cancelChipDrag,
  describeStep,
  distinctLines,
  dropSlot,
  moveChip,
  moveTarget,
  pressChip,
  releaseChip,
  stepSeconds,
  topIndex,
  versionActions,
  versionMark,
  type ChipBox,
  type ChipDrag,
  type VersionAction,
} from "./versions";

const KEYS = "settings.history.versions";

/** Each menu item's label, under `settings.history.versions`. */
const ACTION_LABEL: Record<VersionAction, string> = {
  to_top: "moveToTop",
  up: "moveUp",
  down: "moveDown",
  delete: "deleteVersion",
};

interface VersionTrackProps {
  entry: Pick<HistoryEntry, "versions" | "timestamp">;
  /** The version the entry shows (`shownVersion`). */
  shown: number;
  /** The versions the search matches (`matchingVersions`). */
  matching: number[];
  /** The user chose a version to see. */
  onShow: (index: number) => void;
  /** The user moved version `from` to place `to`, by dragging it or from
   * its menu. */
  onMove: (from: number, to: number) => void;
  /** The user deleted version `index`. */
  onDelete: (index: number) => void;
  /** While a change is saved: the stack can't change. */
  disabled?: boolean;
}

/** What a chip shows: the word "Transcription", or ✨ and ✎ as icons. */
const ChipMark: React.FC<{ version: HistoryVersion }> = ({ version }) => {
  const { t } = useTranslation();
  const mark = versionMark(version);
  if (mark === "transcription") return <>{t(`${KEYS}.transcription`)}</>;
  return (
    <span className="flex items-center gap-0.5">
      {(mark === "ai" || mark === "ai_user") && (
        <WandSparkles width={12} height={12} aria-hidden="true" />
      )}
      {(mark === "user" || mark === "ai_user") && (
        <Pencil width={12} height={12} aria-hidden="true" />
      )}
    </span>
  );
};

/** Where a dragged chip would go. */
const DropMark: React.FC = () => (
  <span
    aria-hidden="true"
    className="w-0.5 self-stretch rounded-full bg-logo-primary"
  />
);

export const VersionTrack: React.FC<VersionTrackProps> = ({
  entry,
  shown,
  matching,
  onShow,
  onMove,
  onDelete,
  disabled = false,
}) => {
  const { t, i18n } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [drag, setDrag] = useState<ChipDrag | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const chipRefs = useRef<(HTMLButtonElement | null)[]>([]);
  // A drag just ended on the chip: the click that follows doesn't show it.
  const dragged = useRef(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuButtonRef = useRef<HTMLButtonElement>(null);
  const popupRef = useRef<HTMLDivElement>(null);
  // Where the open menu's left edge goes, so it stays inside the window.
  const [menuLeft, setMenuLeft] = useState<number | null>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const menuId = useId();

  // The open menu stays inside the window, measured before it is painted.
  useLayoutEffect(() => {
    if (!menuOpen) return;
    const anchor = menuRef.current?.getBoundingClientRect();
    const popup = popupRef.current?.getBoundingClientRect();
    if (!anchor || !popup) return;
    setMenuLeft(
      menuOffset({
        anchorLeft: anchor.left,
        anchorRight: anchor.right,
        menuWidth: popup.width,
        viewportWidth: document.documentElement.clientWidth,
        rtl: i18n.dir() === "rtl",
      }),
    );
  }, [menuOpen, i18n]);

  // Close the menu on a press outside or on Escape; its first item takes
  // the focus. Escape is heard on the window, so it closes the menu even
  // when a click left the focus outside it.
  useEffect(() => {
    if (!menuOpen) return;
    itemRefs.current[0]?.focus({ preventScroll: true });
    const onDown = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    const stopEscape = cancelOnEscape(window, () => {
      setMenuOpen(false);
      menuButtonRef.current?.focus({ preventScroll: true });
    });
    return () => {
      document.removeEventListener("mousedown", onDown);
      stopEscape();
    };
  }, [menuOpen]);

  // While a change is saved, nothing else may start.
  useEffect(() => {
    if (disabled) {
      setMenuOpen(false);
      setDrag(null);
    }
  }, [disabled]);

  // Escape ends a drag without moving anything, heard on the window: the
  // pressed chip has no keyboard focus in WebKit.
  useEffect(() => {
    if (!drag) return;
    return cancelOnEscape(window, () => {
      if (cancelChipDrag(drag).swallowClick) dragged.current = true;
      setDrag(null);
    });
  }, [drag]);

  const count = entry.versions.length;
  if (count <= 1) return null;

  const top = topIndex(entry);
  const belowTop = shown < top;
  // Showing a version below the top keeps the track open: the user sees
  // which one it is and how to make it the result.
  const open = expanded || belowTop;
  const actions = versionActions(entry, shown);
  const rtl = i18n.dir() === "rtl";

  const stepLine = (step: VersionStep, index: number, stepIndex: number) => {
    const { key, prompt } = describeStep(step);
    const name = t(`${KEYS}.step.${key}`, { prompt });
    const seconds = stepSeconds(entry, index, stepIndex);
    return seconds === null
      ? name
      : t(`${KEYS}.step.withTime`, {
          step: name,
          time: formatDateTime(String(seconds), i18n.language),
        });
  };

  const chipTitle = (version: HistoryVersion, index: number) =>
    [
      ...distinctLines(
        version.steps.map((step, stepIndex) =>
          stepLine(step, index, stepIndex),
        ),
      ),
      ...(matching.includes(index) ? [t(`${KEYS}.searchMatch`)] : []),
      ...(index === top ? [t(`${KEYS}.isResult`)] : []),
      index === shown ? t(`${KEYS}.shown`) : t(`${KEYS}.showVersion`),
      ...(disabled ? [] : [t(`${KEYS}.dragHint`)]),
    ].join("\n");

  const boxes = (): ChipBox[] =>
    entry.versions.map((_, index) => {
      const box = chipRefs.current[index]?.getBoundingClientRect();
      return box
        ? { left: box.left, right: box.right, top: box.top, bottom: box.bottom }
        : { left: 0, right: 0, top: 0, bottom: 0 };
    });

  const startDrag =
    (index: number) => (event: React.PointerEvent<HTMLButtonElement>) => {
      dragged.current = false;
      if (disabled || event.button !== 0) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      setDrag(pressChip(index, event.pointerId, event.clientX, event.clientY));
    };

  const moveDrag = (event: React.PointerEvent<HTMLButtonElement>) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    const next = moveChip(drag, event.clientX, event.clientY, (x, y) =>
      dropSlot(boxes(), x, y, rtl),
    );
    if (next !== drag) setDrag(next);
  };

  const endDrag = (event: React.PointerEvent<HTMLButtonElement>) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    const { move, swallowClick } = releaseChip(drag);
    if (swallowClick) dragged.current = true;
    if (move) onMove(move.from, move.to);
    setDrag(null);
  };

  const clickChip = (index: number) => {
    if (dragged.current) {
      dragged.current = false;
      return;
    }
    onShow(index);
  };

  const closeMenu = () => {
    setMenuOpen(false);
    menuButtonRef.current?.focus({ preventScroll: true });
  };

  const run = (action: VersionAction) => {
    closeMenu();
    if (action === "delete") onDelete(shown);
    else onMove(shown, moveTarget(entry, shown, action));
  };

  const onMenuKeyDown = (event: React.KeyboardEvent) => {
    const items = itemRefs.current.filter(
      (item): item is HTMLButtonElement => item !== null,
    );
    const at = items.findIndex((item) => item === document.activeElement);
    const move = (to: number) => {
      event.preventDefault();
      items[(to + items.length) % items.length]?.focus();
    };
    if (event.key === "ArrowDown") move(at + 1);
    else if (event.key === "ArrowUp") move(at - 1);
    else if (event.key === "Home") move(0);
    else if (event.key === "End") move(items.length - 1);
    else if (event.key === "Tab") setMenuOpen(false);
  };

  const summary = t(`${KEYS}.summary`, { count });
  const dropAt = drag?.active ? drag.slot : null;
  const actionClass =
    "flex items-center gap-1 font-medium hover:text-logo-primary cursor-pointer disabled:cursor-not-allowed disabled:text-text/20";

  return (
    <div className="flex flex-col gap-1 w-full text-xs text-ink-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        {belowTop ? (
          <span className="flex items-center gap-1 font-medium">
            <ChevronDown width={12} height={12} aria-hidden="true" />
            {summary}
          </span>
        ) : (
          <button
            type="button"
            onClick={() => setExpanded(!expanded)}
            aria-expanded={open}
            className="flex items-center gap-1 font-medium hover:text-logo-primary cursor-pointer"
          >
            {open ? (
              <ChevronDown width={12} height={12} aria-hidden="true" />
            ) : (
              <ChevronRight
                width={12}
                height={12}
                aria-hidden="true"
                className="rtl:rotate-180"
              />
            )}
            {summary}
          </button>
        )}
        {open && (
          <ol className="flex flex-wrap items-center gap-1">
            {entry.versions.map((version, index) => {
              const isShown = index === shown;
              const matches = matching.includes(index);
              const isDragged = drag?.active === true && drag.from === index;
              return (
                <li key={index} className="flex items-center gap-1">
                  {index > 0 && (
                    <ChevronRight
                      width={10}
                      height={10}
                      aria-hidden="true"
                      className="shrink-0 rtl:rotate-180"
                    />
                  )}
                  {dropAt === index && <DropMark />}
                  <button
                    ref={(chip) => {
                      chipRefs.current[index] = chip;
                    }}
                    type="button"
                    onClick={() => clickChip(index)}
                    onPointerDown={startDrag(index)}
                    onPointerMove={moveDrag}
                    onPointerUp={endDrag}
                    onPointerCancel={() => setDrag(null)}
                    aria-pressed={isShown}
                    title={chipTitle(version, index)}
                    className={`flex items-center rounded-md border px-1.5 py-0.5 touch-none select-none transition-colors ${
                      drag?.active ? "cursor-grabbing" : "cursor-pointer"
                    } ${isDragged ? "opacity-50" : ""} ${
                      // The shown chip gets a ring outside, with a gap of
                      // the surface: a petrol edge would vanish on the
                      // petrol match fill.
                      isShown
                        ? "border-line font-medium ring-1 ring-shown-version ring-offset-1 ring-offset-background"
                        : "border-line hover:border-logo-primary"
                    } ${
                      // A match keeps the match text: petrol on the match
                      // fill would hardly show.
                      matches
                        ? "bg-search-match text-search-match-text"
                        : isShown
                          ? "text-logo-primary"
                          : "hover:text-logo-primary"
                    }`}
                  >
                    <ChipMark version={version} />
                  </button>
                  {dropAt === count && index === count - 1 && <DropMark />}
                </li>
              );
            })}
          </ol>
        )}
        {belowTop && (
          <button
            type="button"
            onClick={() => onMove(shown, top)}
            disabled={disabled}
            title={t(`${KEYS}.moveToTopHint`)}
            className={actionClass}
          >
            <ArrowUpToLine width={12} height={12} aria-hidden="true" />
            {t(`${KEYS}.moveToTop`)}
          </button>
        )}
        {open && actions.length > 0 && (
          <div ref={menuRef} className="relative flex items-center">
            <button
              ref={menuButtonRef}
              type="button"
              disabled={disabled}
              onClick={() => (menuOpen ? closeMenu() : setMenuOpen(true))}
              onKeyDown={(event) => {
                if (!menuOpen && event.key === "ArrowDown") {
                  event.preventDefault();
                  setMenuOpen(true);
                }
              }}
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              aria-controls={menuOpen ? menuId : undefined}
              title={t(`${KEYS}.versionActions`)}
              aria-label={t(`${KEYS}.versionActions`)}
              className={`${actionClass} rounded-md px-1 ${
                menuOpen ? "text-logo-primary" : ""
              }`}
            >
              <Ellipsis width={14} height={14} aria-hidden="true" />
            </button>
            {menuOpen && (
              <div
                ref={popupRef}
                id={menuId}
                role="menu"
                aria-label={t(`${KEYS}.versionActions`)}
                onKeyDown={onMenuKeyDown}
                style={
                  menuLeft === null
                    ? undefined
                    : { left: menuLeft, right: "auto" }
                }
                className="absolute start-0 top-full mt-1 z-50 w-56 max-w-[80vw] bg-background border border-line rounded-lg shadow-lg p-1"
              >
                {actions.map((action, index) => (
                  <button
                    key={action}
                    ref={(item) => {
                      itemRefs.current[index] = item;
                    }}
                    type="button"
                    role="menuitem"
                    onClick={() => run(action)}
                    className="flex w-full items-center rounded-md px-2 py-1.5 text-start text-sm text-text hover:bg-wash focus:bg-wash focus:outline-none cursor-pointer"
                  >
                    {t(`${KEYS}.${ACTION_LABEL[action]}`)}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
