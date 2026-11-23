import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Trash2 } from "lucide-react";
import type { EndKey, ShortcutBinding } from "@/bindings";
import { Input } from "../../ui/Input";
import { Button } from "../../ui/Button";
import { AppendTextInput } from "../AppendText";
import { EndKeyShortcut } from "./EndKeyShortcut";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { useOsType } from "@/hooks/useOsType";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import { usePressAway } from "@/hooks/usePressAway";
import {
  canRemove,
  endKeyName,
  nameToStore,
  newEndKey,
  removeEndKey,
  startsSharing,
  updateEndKey,
  type FormatShortcut,
} from "@/lib/endKeys";

interface EndKeysEditorProps {
  /** The end shortcuts, in their order. */
  value: EndKey[];
  /** The shortcuts of the same scope, to tell an end that has a start
   * shortcut's combination. */
  bindings: Partial<Record<string, ShortcutBinding>>;
  /** The changed list. */
  onChange: (next: EndKey[]) => Promise<void> | void;
  disabled?: boolean;
  /** Post-processing is off in the same scope: an end with "Post-process"
   * says that it doesn't (`postProcessingOn`). */
  postProcessingOff?: boolean;
}

/**
 * DanyWhere: the list of end shortcuts (end_keys.rs, qa 2026-11-22_2134),
 * each with its name, its own key combination, its "Append" field, and
 * whether it post-processes and sends context. A controlled component: the
 * list comes in and a changed one goes out, so it can edit the defaults or
 * a profile's own list alike.
 */
export const EndKeysEditor: React.FC<EndKeysEditorProps> = ({
  value,
  bindings,
  onChange,
  disabled = false,
  postProcessingOff = false,
}) => {
  const { t } = useTranslation();
  const osType = useOsType();
  const format = useCallback<FormatShortcut>(
    (keys) => formatKeyCombination(keys, osType, t),
    [osType, t],
  );
  const change = (id: string, part: Partial<EndKey>) =>
    onChange(updateEndKey(value, id, part));
  const removable = canRemove(value);

  return (
    <div className="flex flex-col divide-y divide-line">
      {value.map((end) => (
        <EndKeyRow
          key={end.id}
          end={end}
          format={format}
          sharedWith={startsSharing(end, bindings)}
          removable={removable}
          onChange={(part) => change(end.id, part)}
          onDelete={() => onChange(removeEndKey(value, end.id))}
          disabled={disabled}
          postProcessingOff={postProcessingOff}
        />
      ))}
      <div className="px-4 py-3">
        <AddEndKey
          value={value}
          onAdd={(end) => onChange([...value, end])}
          disabled={disabled}
        />
      </div>
    </div>
  );
};

interface AddEndKeyProps {
  value: EndKey[];
  onAdd: (end: EndKey) => Promise<void> | void;
  disabled: boolean;
}

/**
 * "Add End Shortcut": records a combination first and adds the end only
 * with it; Escape or a press elsewhere adds nothing.
 */
const AddEndKey: React.FC<AddEndKeyProps> = ({ value, onAdd, disabled }) => {
  const { t } = useTranslation();
  const osType = useOsType();
  // The new end's id is `end_<now>` (unless taken); `now` is kept so the
  // capture's id stays the same while it runs.
  const [now, setNow] = useState(() => Date.now());
  // The capture must keep one callback while it runs: a new one would
  // restart it and lose the keys pressed so far.
  const latest = useRef({ value, onAdd, now });
  latest.current = { value, onAdd, now };
  const onCapture = useCallback(async (shortcut: string) => {
    const { value: list, onAdd: add, now: at } = latest.current;
    await add(newEndKey(list, shortcut, at));
    setNow(Date.now());
  }, []);
  const { capturing, partial, start, cancel } = useShortcutCapture(
    `end_key:end_${now}`,
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
        {partial
          ? formatKeyCombination(partial, osType, t)
          : t("settings.endKeys.addPressKeys")}
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
      {t("settings.endKeys.add")}
    </Button>
  );
};

interface EndKeyRowProps {
  end: EndKey;
  format: FormatShortcut;
  /** The start shortcuts with the same combination. */
  sharedWith: string[];
  /** It isn't the last one, so it may go. */
  removable: boolean;
  onChange: (part: Partial<EndKey>) => Promise<void> | void;
  onDelete: () => Promise<void> | void;
  disabled: boolean;
  postProcessingOff: boolean;
}

const EndKeyRow: React.FC<EndKeyRowProps> = ({
  end,
  format,
  sharedWith,
  removable,
  onChange,
  onDelete,
  disabled,
  postProcessingOff,
}) => {
  const { t } = useTranslation();
  const shownName = endKeyName(end, format);
  // The field shows the own name or the combination's as plain text; it is
  // stored when the field loses focus, as nothing when it is the
  // combination's or blank, so the name follows the combination.
  const [name, setName] = useState(shownName);
  useEffect(() => setName(shownName), [shownName]);
  const nameId = `end-key-name-${end.id}`;

  const storeName = () => {
    const next = nameToStore(name, end, format);
    setName(next === "" ? format(end.shortcut) : next);
    if (next !== end.name) void onChange({ name: next });
  };

  return (
    <div className="flex flex-col gap-2 px-4 py-3">
      <div className="flex items-end gap-2">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <label htmlFor={nameId} className="text-xs text-ink-3">
            {t("settings.endKeys.name")}
          </label>
          <Input
            id={nameId}
            variant="compact"
            className="min-w-0 w-full"
            value={name}
            onChange={(event) => setName(event.target.value)}
            onBlur={storeName}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
            disabled={disabled}
          />
        </div>
        <EndKeyShortcut
          endId={end.id}
          value={end.shortcut}
          onChange={(shortcut) => onChange({ shortcut })}
          disabled={disabled}
        />
        <button
          type="button"
          disabled={disabled || !removable}
          onClick={() => void onDelete()}
          aria-label={t("settings.endKeys.delete", { name: shownName })}
          title={
            removable
              ? t("settings.endKeys.delete", { name: shownName })
              : t("settings.endKeys.lastOne")
          }
          className="p-1 mb-1 rounded-md text-ink-3 hover:text-text hover:bg-logo-primary/10 disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <Trash2 className="h-4 w-4" />
        </button>
      </div>
      {sharedWith.length > 0 && (
        <p className="text-xs text-ink-3">
          {t("settings.endKeys.sameAsStart", {
            start: sharedWith
              .map((binding) =>
                t(`settings.general.shortcut.bindings.${binding}.name`),
              )
              .join(", "),
          })}
        </p>
      )}
      <div className="flex flex-col gap-1">
        <span
          className="text-xs text-ink-3"
          title={t("settings.endKeys.appendDescription")}
        >
          {t("settings.endKeys.append")}
        </span>
        <AppendTextInput
          value={end.append_text}
          onChange={(append_text) => onChange({ append_text })}
          disabled={disabled}
        />
      </div>
      <div className="flex flex-wrap items-center gap-4 text-sm">
        <label className="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            className="accent-logo-primary"
            checked={end.post_process}
            disabled={disabled}
            onChange={(event) =>
              void onChange({ post_process: event.target.checked })
            }
          />
          {t("settings.endKeys.postProcess")}
        </label>
        <label
          className="flex items-center gap-2 cursor-pointer"
          title={t("settings.endKeys.sendContextHint")}
        >
          <input
            type="checkbox"
            className="accent-logo-primary"
            checked={end.send_context}
            disabled={disabled || !end.post_process}
            onChange={(event) =>
              void onChange({ send_context: event.target.checked })
            }
          />
          {t("settings.endKeys.sendContext")}
        </label>
      </div>
      {/* DanyWhere: with post-processing off no end post-processes (user,
          2026-11-23, option a); the box keeps its value for when it is on. */}
      {end.post_process && postProcessingOff && (
        <p className="text-xs text-ink-3">
          {t("settings.endKeys.postProcessOff")}
        </p>
      )}
    </div>
  );
};
