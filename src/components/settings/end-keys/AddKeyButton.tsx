import React, { useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { useOsType } from "@/hooks/useOsType";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import { usePressAway } from "@/hooks/usePressAway";

interface AddKeyButtonProps {
  /** The capture's id for a capture that starts at `now`, e.g.
   * `end_key:end_<now>`. */
  captureId: (now: number) => string;
  /** The button's text ("Add End Shortcut"). */
  label: string;
  /** What the capture asks for before a key is down. */
  pressKeys: string;
  /** Adds the entry with the recorded combination; `now` is when the
   * capture started, for the new entry's id. */
  onAdd: (shortcut: string, now: number) => Promise<void> | void;
  disabled?: boolean;
}

/**
 * DanyWhere: "Add End Shortcut", "Add Hold Shortcut", "Add Start
 * Shortcut" (qa 2026-11-22_2134, 2026-11-26_1810): records a combination
 * first and adds the entry only with it; Escape or a press elsewhere adds
 * nothing.
 */
export const AddKeyButton: React.FC<AddKeyButtonProps> = ({
  captureId,
  label,
  pressKeys,
  onAdd,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const osType = useOsType();
  // `now` is kept so the capture's id stays the same while it runs.
  const [now, setNow] = useState(() => Date.now());
  // The capture must keep one callback while it runs: a new one would
  // restart it and lose the keys pressed so far.
  const latest = useRef({ onAdd, now });
  latest.current = { onAdd, now };
  const onCapture = useCallback(async (shortcut: string) => {
    const { onAdd: add, now: at } = latest.current;
    await add(shortcut, at);
    setNow(Date.now());
  }, []);
  const { capturing, partial, start, cancel } = useShortcutCapture(
    captureId(now),
    osType,
    { onCapture, escapeCancels: true },
  );
  const ref = useRef<HTMLDivElement | null>(null);
  usePressAway(capturing, ref, cancel);

  if (capturing) {
    return (
      <div
        ref={ref}
        className="inline-block px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/20 rounded-md"
      >
        {partial ? formatKeyCombination(partial, osType, t) : pressKeys}
      </div>
    );
  }
  return (
    <Button
      variant="secondary"
      size="sm"
      disabled={disabled}
      onClick={() => void start()}
    >
      {label}
    </Button>
  );
};
