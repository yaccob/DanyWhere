// DanyWhere: History's ✨ button. A click post-processes the current version,
// the one the entry shows, with the active prompts of the profile in use;
// the result goes on top as a new version and every version stays, so
// nothing asks first (user, 2026-11-19, H1 and H5). Its menu (H6, strand
// E2) lists every prompt of the shared library by name, in the order of the
// profile in use, the active ones checked; choosing one post-processes the
// version shown with that prompt alone, the result on top as well.
import React, { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown, WandSparkles } from "lucide-react";
import type { PostProcessAvailability } from "./postProcessEntry";
import type { PromptMenuItem } from "./promptMenu";

const KEYS = "settings.history.versions";

interface PostProcessButtonProps {
  /** Whether post-processing can run with the current settings; when it
   * can't, the button says why. */
  availability: PostProcessAvailability;
  /** Post-processes the current version. */
  onRun: () => void;
  /** The library's prompts for the menu (`promptMenuItems`). */
  prompts: PromptMenuItem[];
  /** Post-processes the current version with prompt `promptId` alone. */
  onRunPrompt: (promptId: string) => void;
  /** Whether a prompt chosen by name can run: on and set up, active
   * prompts or not. */
  menuUsable: boolean;
  /** While the user edits the text: nothing may change the stack. */
  disabled?: boolean;
}

export const PostProcessButton: React.FC<PostProcessButtonProps> = ({
  availability,
  onRun,
  prompts,
  onRunPrompt,
  menuUsable,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const ready = availability === "ready";
  const usable = ready && !disabled;
  const menuEnabled = menuUsable && !disabled && prompts.length > 0;
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const toggleRef = useRef<HTMLButtonElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const menuId = useId();

  // Close on a press outside; the first item takes the focus.
  useEffect(() => {
    if (!open) return;
    itemRefs.current[0]?.focus({ preventScroll: true });
    const onDown = (event: MouseEvent) => {
      if (!wrapRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  useEffect(() => {
    if (!menuEnabled) setOpen(false);
  }, [menuEnabled]);

  const close = () => {
    setOpen(false);
    toggleRef.current?.focus({ preventScroll: true });
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

  const title = ready
    ? `${t(`${KEYS}.postProcessFurther`)}\n${t(`${KEYS}.postProcessFurtherHint`)}`
    : availability === "off"
      ? t("settings.history.postProcessOff")
      : t("settings.history.postProcessNotSetUp");
  const menuLabel = t("settings.history.promptMenu.button");

  return (
    <div ref={wrapRef} className="relative flex items-center">
      <button
        type="button"
        onClick={usable ? onRun : undefined}
        aria-disabled={!usable || undefined}
        title={title}
        className={`p-1.5 rounded-md flex items-center justify-center transition-colors ${
          usable
            ? "cursor-pointer text-ink-3 hover:text-logo-primary"
            : "cursor-not-allowed text-text/20"
        }`}
      >
        <WandSparkles width={16} height={16} />
      </button>
      <button
        ref={toggleRef}
        type="button"
        onClick={() =>
          menuEnabled ? (open ? close() : setOpen(true)) : undefined
        }
        onKeyDown={(event) => {
          if (menuEnabled && !open && event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
          }
        }}
        aria-disabled={!menuEnabled || undefined}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        title={menuLabel}
        aria-label={menuLabel}
        className={`-ms-1 py-1.5 pe-1 rounded-md flex items-center justify-center transition-colors ${
          menuEnabled
            ? `cursor-pointer hover:text-logo-primary ${open ? "text-logo-primary" : "text-ink-3"}`
            : "cursor-not-allowed text-text/20"
        }`}
      >
        <ChevronDown width={12} height={12} aria-hidden="true" />
      </button>
      {open && (
        <div
          id={menuId}
          role="menu"
          aria-label={menuLabel}
          onKeyDown={onMenuKeyDown}
          className="absolute end-0 top-full mt-1 z-50 w-64 max-w-[80vw] bg-background border border-line rounded-lg shadow-lg p-1"
        >
          <div className="px-2 py-1 text-xs text-ink-3">
            {t("settings.history.promptMenu.title")}
          </div>
          {prompts.map((prompt, index) => (
            <button
              key={prompt.id}
              ref={(item) => {
                itemRefs.current[index] = item;
              }}
              type="button"
              role="menuitemcheckbox"
              aria-checked={prompt.active}
              title={prompt.tooltip}
              onClick={() => {
                close();
                onRunPrompt(prompt.id);
              }}
              className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-start text-sm text-text hover:bg-wash focus:bg-wash focus:outline-none cursor-pointer"
            >
              <span className="flex size-4 shrink-0 items-center justify-center text-logo-primary">
                {prompt.active && (
                  <Check width={14} height={14} aria-hidden="true" />
                )}
              </span>
              <span className="min-w-0 flex-1 truncate">{prompt.label}</span>
              {prompt.active && (
                <span className="sr-only">
                  {t("settings.history.promptMenu.active")}
                </span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
