import React from "react";
import { useTranslation } from "react-i18next";
import { Trash2 } from "lucide-react";
import type { StartKey } from "@/bindings";
import { EndKeyShortcut } from "../end-keys/EndKeyShortcut";
import { AddKeyButton } from "../end-keys/AddKeyButton";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { useOsType } from "@/hooks/useOsType";
import { removeEndKey, updateEndKey } from "@/lib/endKeys";
import { newStartKey } from "@/lib/startKeys";

interface StartKeysEditorProps {
  /** The start shortcuts, in their order. */
  value: StartKey[];
  /** The changed list. */
  onChange: (next: StartKey[]) => Promise<void> | void;
  /** Whether an entry may go: a start or hold shortcut must stay. */
  removable: boolean;
  /** The grey line beside an entry ("Also a hold shortcut"), if any. */
  hintOf?: (entry: StartKey) => string | null;
  disabled?: boolean;
}

/**
 * DanyWhere: the list of start shortcuts (qa 2026-11-26_1713,
 * "Konzeptvorschlag"; 2026-11-26_1810): each only a key combination, since
 * a start shortcut has no values; click it to record another one. "Add
 * Start Shortcut" records a combination first. A controlled component, as
 * the end shortcuts' editor is.
 */
export const StartKeysEditor: React.FC<StartKeysEditorProps> = ({
  value,
  onChange,
  removable,
  hintOf,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const osType = useOsType();

  return (
    <div className="flex flex-col divide-y divide-line">
      {value.map((start) => {
        const keys = formatKeyCombination(start.shortcut, osType, t);
        const hint = hintOf?.(start) ?? null;
        const remove = t("settings.startKeys.delete", { shortcut: keys });
        return (
          <div key={start.id} className="flex items-center gap-3 px-4 py-3">
            <EndKeyShortcut
              captureId={`start_key:${start.id}`}
              value={start.shortcut}
              onChange={(shortcut) =>
                onChange(updateEndKey(value, start.id, { shortcut }))
              }
              hint={t("settings.startKeys.shortcutHint")}
              disabled={disabled}
            />
            <span className="min-w-0 flex-1 text-xs text-ink-3">{hint}</span>
            <button
              type="button"
              disabled={disabled || !removable}
              onClick={() => void onChange(removeEndKey(value, start.id))}
              aria-label={remove}
              title={removable ? remove : t("settings.startKeys.lastOne")}
              className="p-1 rounded-md text-ink-3 hover:text-text hover:bg-logo-primary/10 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <Trash2 className="h-4 w-4" />
            </button>
          </div>
        );
      })}
      <div className="px-4 py-3">
        <AddKeyButton
          captureId={(now) => `start_key:start_${now}`}
          label={t("settings.startKeys.add")}
          pressKeys={t("settings.startKeys.addPressKeys")}
          onAdd={(shortcut, now) =>
            onChange([...value, newStartKey(value, shortcut, now)])
          }
          disabled={disabled}
        />
      </div>
    </div>
  );
};
