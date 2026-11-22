import React, { useRef } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { useOsType } from "@/hooks/useOsType";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import { usePressAway } from "@/hooks/usePressAway";

interface EndKeyShortcutProps {
  /** The end shortcut's id, for the capture (registered as `end_key:<id>`). */
  endId: string;
  /** Its key combination; empty: none. */
  value: string;
  onChange: (shortcut: string) => Promise<void> | void;
  disabled?: boolean;
}

/**
 * DanyWhere: an end shortcut's own key combination (end_keys.rs): click to
 * record one, the cross removes it. Recorded like a binding, with every
 * shortcut suspended meanwhile, but given out instead of stored.
 */
export const EndKeyShortcut: React.FC<EndKeyShortcutProps> = ({
  endId,
  value,
  onChange,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const osType = useOsType();
  const { capturing, partial, start, cancel } = useShortcutCapture(
    `end_key:${endId}`,
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
          ? formatKeyCombination(partial, osType)
          : t("settings.general.shortcut.pressKeys")}
      </div>
    );
  }
  return (
    <div className="flex items-center gap-1">
      <button
        type="button"
        disabled={disabled}
        onClick={() => void start()}
        title={t("settings.endKeys.shortcutHint")}
        className="px-2 py-1 text-sm font-semibold bg-wash border border-line-strong hover:bg-logo-primary/10 rounded-md cursor-pointer hover:border-logo-primary disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {value
          ? formatKeyCombination(value, osType)
          : t("settings.endKeys.noShortcut")}
      </button>
      {value && (
        <button
          type="button"
          disabled={disabled}
          onClick={() => void onChange("")}
          aria-label={t("settings.endKeys.clearShortcut")}
          title={t("settings.endKeys.clearShortcut")}
          className="p-1 rounded-md text-ink-3 hover:text-text hover:bg-logo-primary/10 disabled:opacity-50"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      )}
    </div>
  );
};
