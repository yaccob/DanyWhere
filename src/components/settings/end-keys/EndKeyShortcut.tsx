import React, { useRef } from "react";
import { useTranslation } from "react-i18next";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { useOsType } from "@/hooks/useOsType";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import { usePressAway } from "@/hooks/usePressAway";

interface EndKeyShortcutProps {
  /** The capture's id, as the backend registers the entry:
   * `end_key:<id>`, `hold_key:<id>` or `start_key:<id>`. */
  captureId: string;
  /** Its key combination; every start, hold and end shortcut has one. */
  value: string;
  onChange: (shortcut: string) => Promise<void> | void;
  /** What the combination does, as the button's tooltip. */
  hint: string;
  disabled?: boolean;
}

/**
 * DanyWhere: the key combination of an end, hold or start shortcut
 * (end_keys.rs, qa 2026-11-26_1810): click to record another one. It can't
 * be removed: every entry needs one (qa 2026-11-22_2134). Recorded like a
 * binding, with every shortcut suspended meanwhile, but given out instead
 * of stored.
 */
export const EndKeyShortcut: React.FC<EndKeyShortcutProps> = ({
  captureId,
  value,
  onChange,
  hint,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const osType = useOsType();
  const { capturing, partial, start, cancel } = useShortcutCapture(
    captureId,
    osType,
    { onCapture: onChange },
  );
  const ref = useRef<HTMLDivElement | null>(null);
  usePressAway(capturing, ref, cancel);

  if (capturing) {
    return (
      <div
        ref={ref}
        className="px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/20 rounded-md"
      >
        {partial
          ? formatKeyCombination(partial, osType, t)
          : t("settings.general.shortcut.pressKeys")}
      </div>
    );
  }
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={() => void start()}
      title={hint}
      className="min-w-12 px-2 py-1 text-sm font-semibold bg-wash border border-line-strong hover:bg-logo-primary/10 rounded-md cursor-pointer hover:border-logo-primary disabled:opacity-50 disabled:cursor-not-allowed"
    >
      {formatKeyCombination(value, osType, t)}
    </button>
  );
};
