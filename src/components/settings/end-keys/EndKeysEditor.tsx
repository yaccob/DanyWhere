import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Trash2 } from "lucide-react";
import type { EndKey } from "@/bindings";
import { Input } from "../../ui/Input";
import { Button } from "../../ui/Button";
import { AppendTextInput } from "../AppendText";
import { EndKeyShortcut } from "./EndKeyShortcut";
import {
  endKeyName,
  newEndKey,
  removeEndKey,
  startsEndingWith,
  updateEndKey,
} from "@/lib/endKeys";

interface EndKeysEditorProps {
  /** The end shortcuts, in their order. */
  value: EndKey[];
  /** The default end per start shortcut, to keep those in use. */
  defaultEnds: Partial<Record<string, string>>;
  /** The changed list. */
  onChange: (next: EndKey[]) => Promise<void> | void;
  disabled?: boolean;
}

/**
 * DanyWhere: the list of end shortcuts (end_keys.rs), each with its name,
 * its own key combination, its "Append" field, and whether it
 * post-processes and sends context. A controlled component: the list comes
 * in and a changed one goes out, so it can edit the defaults or a profile's
 * own list alike.
 */
export const EndKeysEditor: React.FC<EndKeysEditorProps> = ({
  value,
  defaultEnds,
  onChange,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const change = (id: string, part: Partial<EndKey>) =>
    onChange(updateEndKey(value, id, part));

  return (
    <div className="flex flex-col divide-y divide-line">
      {value.map((end) => (
        <EndKeyRow
          key={end.id}
          end={end}
          inUse={startsEndingWith(end.id, defaultEnds).length > 0}
          onChange={(part) => change(end.id, part)}
          onDelete={() => onChange(removeEndKey(value, end.id))}
          disabled={disabled}
        />
      ))}
      <div className="px-4 py-3">
        <Button
          variant="secondary"
          size="sm"
          disabled={disabled}
          onClick={() =>
            onChange([
              ...value,
              newEndKey(value, t("settings.endKeys.newName"), Date.now()),
            ])
          }
        >
          {t("settings.endKeys.add")}
        </Button>
      </div>
    </div>
  );
};

interface EndKeyRowProps {
  end: EndKey;
  /** A start shortcut ends with it by default: it can't go. */
  inUse: boolean;
  onChange: (part: Partial<EndKey>) => Promise<void> | void;
  onDelete: () => Promise<void> | void;
  disabled: boolean;
}

const EndKeyRow: React.FC<EndKeyRowProps> = ({
  end,
  inUse,
  onChange,
  onDelete,
  disabled,
}) => {
  const { t } = useTranslation();
  const shownName = endKeyName(end, t);
  // The name is stored when the field loses focus.
  const [name, setName] = useState(end.name);
  useEffect(() => setName(end.name), [end.name]);

  return (
    <div className="flex flex-col gap-2 px-4 py-3">
      <div className="flex items-center gap-2">
        <Input
          variant="compact"
          className="min-w-0 flex-1"
          value={name}
          placeholder={shownName}
          aria-label={t("settings.endKeys.name")}
          onChange={(event) => setName(event.target.value)}
          onBlur={() => {
            if (name !== end.name) void onChange({ name: name.trim() });
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
          disabled={disabled}
        />
        <EndKeyShortcut
          endId={end.id}
          value={end.shortcut}
          onChange={(shortcut) => onChange({ shortcut })}
          disabled={disabled}
        />
        <button
          type="button"
          disabled={disabled || inUse}
          onClick={() => void onDelete()}
          aria-label={t("settings.endKeys.delete", { name: shownName })}
          title={
            inUse
              ? t("settings.endKeys.inUse")
              : t("settings.endKeys.delete", { name: shownName })
          }
          className="p-1 rounded-md text-ink-3 hover:text-text hover:bg-logo-primary/10 disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <Trash2 className="h-4 w-4" />
        </button>
      </div>
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
          {t("settings.endKeys.builtin.post_process")}
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
    </div>
  );
};
