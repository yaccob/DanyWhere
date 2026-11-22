import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { cancelOnEscape } from "@/lib/escapeLayers";
import { menuOffset } from "@/lib/menuPlacement";

export interface DropdownOption {
  value: string;
  label: React.ReactNode;
  description?: string;
  disabled?: boolean;
}

/** DanyWhere: the menu is at least as wide as its button and grows with
 * its longest entry up to 20rem, never past the window less its margins
 * (`MENU_MARGIN`, 8 px each side). A chosen entry's check and bold type
 * thus never break it into two lines (qa 2026-11-22_0233); longer entries
 * still wrap. Descriptions don't widen it: they wrap at the entries' width. */
const MENU_WIDTH = "min-w-full w-max max-w-[min(20rem,calc(100vw-16px))]";

interface DropdownProps {
  options: DropdownOption[];
  className?: string;
  /** The menu's width, instead of `MENU_WIDTH`; where it opens is
   * measured either way. */
  menuClassName?: string;
  selectedValue: string | null;
  onSelect: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  onOpen?: () => void;
  /** Where the menu opens: below the button (default), or above it. */
  placement?: "below" | "above";
  /** Highlight colour for hover and selection. */
  tone?: "brand" | "neutral";
  /** DanyWhere: no minimum width, for a narrow column such as the sidebar. */
  compact?: boolean;
}

export const Dropdown: React.FC<DropdownProps> = ({
  options,
  selectedValue,
  onSelect,
  className = "",
  menuClassName,
  placeholder = "Select an option...",
  disabled = false,
  onOpen,
  placement = "below",
  tone = "brand",
  compact = false,
}) => {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  // Where the open menu's left edge goes, so it stays inside the window.
  const [menuLeft, setMenuLeft] = useState<number | null>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  // DanyWhere: Escape closes the open list, heard on the window: WebKit
  // gives the clicked button no keyboard focus, so a key handler here never
  // heard it (qa 2026-11-22_0233). A dialog or menu behind stays open until
  // the next Escape. The focus goes back to the button.
  useEffect(() => {
    if (!isOpen || disabled) return;
    return cancelOnEscape(window, () => {
      setIsOpen(false);
      triggerRef.current?.focus();
    });
  }, [isOpen, disabled]);

  // DanyWhere: the open menu stays inside the window, measured before it is
  // painted: flush with the button's start edge, else with its end edge
  // (`menuOffset`). Its width follows the entries, so it is measured again
  // when they or the choice change.
  useLayoutEffect(() => {
    if (!isOpen || disabled) return;
    const box = dropdownRef.current;
    const menu = menuRef.current;
    if (!box || !menu) return;
    const anchor = box.getBoundingClientRect();
    setMenuLeft(
      menuOffset({
        anchorLeft: anchor.left,
        anchorRight: anchor.right,
        menuWidth: menu.getBoundingClientRect().width,
        viewportWidth: document.documentElement.clientWidth,
        rtl: getComputedStyle(box).direction === "rtl",
      }),
    );
  }, [isOpen, disabled, options, selectedValue]);

  // The trigger's hover; in the menu, hover is grey so it never looks chosen.
  const hover =
    tone === "neutral" ? "hover:bg-wash" : "hover:bg-logo-primary/10";
  const chosen = tone === "neutral" ? "bg-line" : "bg-logo-primary/15";

  const selectedOption = options.find(
    (option) => option.value === selectedValue,
  );

  const handleSelect = (value: string) => {
    onSelect(value);
    setIsOpen(false);
  };

  const handleToggle = () => {
    if (disabled) return;
    if (!isOpen) onOpen?.();
    setIsOpen(!isOpen);
  };

  return (
    <div className={`relative ${className}`} ref={dropdownRef}>
      <button
        type="button"
        ref={triggerRef}
        className={`px-2 py-[5px] text-sm font-semibold bg-wash border border-line-strong rounded-md ${compact ? "min-w-0" : "min-w-[200px]"} w-full text-start grid grid-cols-[1fr_auto] gap-2 items-center transition-all duration-150 ${
          disabled
            ? "opacity-50 cursor-not-allowed"
            : `${hover} cursor-pointer ${tone === "neutral" ? "hover:border-ink-3" : "hover:border-logo-primary"}`
        }`}
        onClick={handleToggle}
        disabled={disabled}
        aria-expanded={isOpen}
      >
        <span className="truncate">{selectedOption?.label || placeholder}</span>
        <svg
          className={`w-4 h-4 transition-transform duration-200 ${isOpen ? "transform rotate-180" : ""}`}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M19 9l-7 7-7-7"
          />
        </svg>
      </button>
      {isOpen && !disabled && (
        <div
          ref={menuRef}
          style={
            menuLeft === null ? undefined : { left: menuLeft, right: "auto" }
          }
          className={`absolute start-0 ${placement === "above" ? "bottom-full mb-1" : "top-full mt-1"} bg-background border border-line rounded-lg shadow-lg z-50 max-h-60 overflow-y-auto p-1 ${
            menuClassName ?? MENU_WIDTH
          }`}
        >
          {options.length === 0 ? (
            <div className="px-2 py-1.5 text-sm text-ink-3">
              {t("common.noOptionsFound")}
            </div>
          ) : (
            options.map((option) => {
              const isChosen = selectedValue === option.value;
              return (
                <button
                  key={option.value}
                  type="button"
                  className={`flex w-full items-center gap-2 rounded-md px-2 text-sm text-start transition-colors duration-150 ${
                    option.description ? "py-2" : "py-1.5"
                  } ${isChosen ? chosen : "hover:bg-wash"} ${
                    option.disabled ? "opacity-50 cursor-not-allowed" : ""
                  }`}
                  onClick={() => handleSelect(option.value)}
                  disabled={option.disabled}
                >
                  <span className="min-w-0 flex-1">
                    <span
                      className={`block whitespace-normal break-words ${
                        isChosen ? "font-semibold" : ""
                      }`}
                    >
                      {option.label}
                    </span>
                    {option.description && (
                      // w-0 min-w-full: wraps at the entries' width
                      // instead of widening the menu to its own.
                      <span className="mt-0.5 block w-0 min-w-full whitespace-normal text-xs font-normal leading-snug text-ink-3">
                        {option.description}
                      </span>
                    )}
                  </span>
                  {isChosen && (
                    <svg
                      width="16"
                      height="16"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth={2.2}
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      aria-hidden="true"
                      className="shrink-0 text-ink-2"
                    >
                      <path d="M5 12.5l4.5 4.5L19 7.5" />
                    </svg>
                  )}
                </button>
              );
            })
          )}
        </div>
      )}
    </div>
  );
};
