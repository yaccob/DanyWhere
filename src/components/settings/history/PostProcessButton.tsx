// DanyWhere: History's ✨ button for an entry with versions. A click
// post-processes the newest version further (a new version above it, which
// never loses anything); a small chevron next to it opens the other actions
// the stack allows: post-processing the newest version in place, and redoing
// the post-processing from the version below it.
import React, { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, WandSparkles } from "lucide-react";
import type { HistoryEntry } from "@/bindings";
import { confirmLoss } from "./confirmLoss";
import type { PostProcessAvailability } from "./postProcessEntry";
import {
  postProcessActions,
  redoNeedsConfirm,
  type PostProcessAction,
} from "./versions";

const KEYS = "settings.history.versions";

/** The label and the hint of each action, under `settings.history.versions`. */
const ACTION_TEXT: Record<PostProcessAction, { label: string; hint: string }> =
  {
    post_process: {
      label: "postProcessFurther",
      hint: "postProcessFurtherHint",
    },
    post_process_replace: {
      label: "postProcessThis",
      hint: "postProcessThisHint",
    },
    post_process_redo: { label: "redo", hint: "redoHint" },
  };

interface PostProcessButtonProps {
  entry: Pick<HistoryEntry, "versions">;
  /** Whether post-processing can run with the current settings; when it
   * can't, the button says why and the menu is gone. */
  availability: PostProcessAvailability;
  /** Runs an action the user chose (and agreed to, for a redo that loses
   * their edits). */
  onRun: (action: PostProcessAction) => void;
  /** While the user edits the text: nothing may change the stack. */
  disabled?: boolean;
}

export const PostProcessButton: React.FC<PostProcessButtonProps> = ({
  entry,
  availability,
  onRun,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const chevronRef = useRef<HTMLButtonElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const menuId = useId();

  const ready = availability === "ready";
  const more = postProcessActions(entry).filter((a) => a !== "post_process");
  const showMenu = ready && more.length > 0;
  const usable = ready && !disabled;

  // Close on a press outside; the first item takes the focus on opening.
  useEffect(() => {
    if (!open) return;
    itemRefs.current[0]?.focus({ preventScroll: true });
    const onDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  // The stack changed so that no other action is left, or editing began:
  // nothing to show.
  useEffect(() => {
    if (!showMenu || disabled) setOpen(false);
  }, [showMenu, disabled]);

  const close = () => {
    setOpen(false);
    chevronRef.current?.focus({ preventScroll: true });
  };

  const run = async (action: PostProcessAction) => {
    setOpen(false);
    if (action === "post_process_redo" && redoNeedsConfirm(entry)) {
      const agreed = await confirmLoss({
        title: t(`${KEYS}.confirmRedoTitle`),
        message: t(`${KEYS}.confirmRedoMessage`),
        okLabel: t(`${KEYS}.confirmRedo`),
        cancelLabel: t(`${KEYS}.cancel`),
      });
      if (!agreed) return;
    }
    onRun(action);
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
    else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === "Tab") setOpen(false);
  };

  const mainTitle = ready
    ? `${t(`${KEYS}.postProcessFurther`)}\n${t(`${KEYS}.postProcessFurtherHint`)}`
    : availability === "off"
      ? t("settings.history.postProcessOff")
      : t("settings.history.postProcessNotSetUp");

  const iconClass = usable
    ? "cursor-pointer text-ink-3 hover:text-logo-primary"
    : "cursor-not-allowed text-text/20";

  return (
    <div ref={rootRef} className="relative flex items-center">
      <button
        type="button"
        onClick={usable ? () => onRun("post_process") : undefined}
        aria-disabled={!usable || undefined}
        title={mainTitle}
        className={`p-1.5 rounded-md flex items-center justify-center transition-colors ${iconClass} ${
          showMenu ? "pe-0.5" : ""
        }`}
      >
        <WandSparkles width={16} height={16} />
      </button>
      {showMenu && (
        <button
          ref={chevronRef}
          type="button"
          disabled={disabled}
          onClick={() => (open ? close() : setOpen(true))}
          onKeyDown={(event) => {
            if (!open && event.key === "ArrowDown") {
              event.preventDefault();
              setOpen(true);
            }
          }}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-controls={open ? menuId : undefined}
          title={t(`${KEYS}.moreActions`)}
          aria-label={t(`${KEYS}.moreActions`)}
          className={`py-1.5 pe-1 rounded-md flex items-center justify-center transition-colors ${iconClass} ${
            open ? "text-logo-primary" : ""
          }`}
        >
          <ChevronDown
            width={12}
            height={12}
            aria-hidden="true"
            className={`transition-transform duration-200 ${open ? "rotate-180" : ""}`}
          />
        </button>
      )}
      {open && showMenu && (
        <div
          id={menuId}
          role="menu"
          aria-label={t(`${KEYS}.moreActions`)}
          onKeyDown={onMenuKeyDown}
          className="absolute end-0 top-full mt-1 z-50 w-72 max-w-[80vw] bg-background border border-line rounded-lg shadow-lg p-1"
        >
          {more.map((action, index) => (
            <button
              key={action}
              ref={(item) => {
                itemRefs.current[index] = item;
              }}
              type="button"
              role="menuitem"
              onClick={() => void run(action)}
              className="flex w-full flex-col items-start rounded-md px-2 py-2 text-start hover:bg-wash focus:bg-wash focus:outline-none cursor-pointer"
            >
              <span className="text-sm text-text">
                {t(`${KEYS}.${ACTION_TEXT[action].label}`)}
              </span>
              <span className="mt-0.5 text-xs leading-snug text-ink-3">
                {t(`${KEYS}.${ACTION_TEXT[action].hint}`)}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
