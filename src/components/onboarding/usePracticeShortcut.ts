import { useCallback } from "react";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import type { OSType } from "@/lib/utils/keyboard";
import { practiceChange, practiceShortcut } from "@/lib/startKeys";

/**
 * DanyWhere: the keys Demo and Try it practise with (qa 2026-11-26_1810):
 * tapping with the first start shortcut, holding with the first hold
 * shortcut, which out of the box are the same keys (⌃ Space). Changing
 * them there changes the first start shortcut and the hold shortcut on the
 * same keys, so both gestures keep working on the new keys. `preview`
 * records without storing.
 */
export const usePracticeShortcut = (
  os: OSType,
  { onDone, preview }: { onDone?: () => void; preview: boolean },
) => {
  const { getSetting, refreshSettings } = useSettings();
  const starts = getSetting("start_keys");
  const holds = getSetting("hold_keys");
  const shortcut = practiceShortcut(starts, holds);
  const captureId = starts?.[0]
    ? `start_key:${starts[0].id}`
    : `hold_key:${holds?.[0]?.id ?? ""}`;

  const change = useCallback(
    async (keys: string) => {
      const next = practiceChange(starts, holds, keys);
      try {
        if (next.start_keys) {
          const result = await commands.changeStartKeysSetting(next.start_keys);
          if (result.status === "error") throw new Error(result.error);
        }
        if (next.hold_keys) {
          const result = await commands.changeHoldKeysSetting(next.hold_keys);
          if (result.status === "error") throw new Error(result.error);
        }
      } finally {
        await refreshSettings();
      }
    },
    [starts, holds, refreshSettings],
  );

  const capture = useShortcutCapture(captureId, os, {
    onDone,
    inert: preview,
    escapeCancels: true,
    onCapture: preview ? undefined : change,
  });

  return { shortcut, capture };
};
