import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Trash2 } from "lucide-react";
import type { EndKey } from "@/bindings";
import { Input } from "../../ui/Input";
import { AppendTextInput } from "../AppendText";
import { EndKeyShortcut } from "./EndKeyShortcut";
import { AddKeyButton } from "./AddKeyButton";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { useOsType } from "@/hooks/useOsType";
import {
  canRemove,
  nameToStore,
  newEndKey,
  removeEndKey,
  updateEndKey,
  type FormatShortcut,
} from "@/lib/endKeys";
import { holdKeyDefaultName } from "@/lib/startKeys";

/** The two lists of this shape: end shortcuts, and hold shortcuts, which
 * end a recording when released (qa 2026-11-26_1810). */
export type EndKeysKind = "end" | "hold";

/** What differs between the two lists: their texts, the ids of new
 * entries and how the backend registers an entry for its capture. */
const KINDS: Record<
  EndKeysKind,
  {
    add: string;
    addPressKeys: string;
    lastOne: string;
    shortcutHint: string;
    idPrefix: string;
    capturePrefix: string;
  }
> = {
  end: {
    add: "settings.endKeys.add",
    addPressKeys: "settings.endKeys.addPressKeys",
    lastOne: "settings.endKeys.lastOne",
    shortcutHint: "settings.endKeys.shortcutHint",
    idPrefix: "end",
    capturePrefix: "end_key",
  },
  hold: {
    add: "settings.holdKeys.add",
    addPressKeys: "settings.holdKeys.addPressKeys",
    lastOne: "settings.startKeys.lastOne",
    shortcutHint: "settings.holdKeys.shortcutHint",
    idPrefix: "hold",
    capturePrefix: "hold_key",
  },
};

interface EndKeysEditorProps {
  /** The entries, in their order. */
  value: EndKey[];
  /** The changed list. */
  onChange: (next: EndKey[]) => Promise<void> | void;
  /** End shortcuts (the default) or hold shortcuts. */
  kind?: EndKeysKind;
  /** Whether an entry may go; by default while another stays. Hold
   * shortcuts may go while a start or hold shortcut stays. */
  removable?: boolean;
  /** The grey lines under an entry: which other lists have its keys. */
  hintsOf?: (entry: EndKey) => string[];
  disabled?: boolean;
  /** Post-processing is off in the same scope: an entry with "Post-process"
   * says that it doesn't (`postProcessingOn`). */
  postProcessingOff?: boolean;
  /** No dictation can post-process at all (`postProcessingReachable`): a
   * hold shortcut with "Post-process" says that it isn't taken (qa
   * 2026-11-26_1713, question 4 a). */
  postProcessingUnreachable?: boolean;
}

/**
 * DanyWhere: the list of end shortcuts (end_keys.rs, qa 2026-11-22_2134)
 * or of hold shortcuts (qa 2026-11-26_1810), each with its name, its own
 * key combination, its "Append" field, and whether it post-processes and
 * sends context. Without a name of its own an end shortcut is called by its
 * combination, a hold shortcut "<keys> held". A controlled component: the
 * list comes in and a changed one goes out, so it can edit the defaults or
 * a profile's own list alike.
 */
export const EndKeysEditor: React.FC<EndKeysEditorProps> = ({
  value,
  onChange,
  kind = "end",
  removable,
  hintsOf,
  disabled = false,
  postProcessingOff = false,
  postProcessingUnreachable = false,
}) => {
  const { t } = useTranslation();
  const osType = useOsType();
  const texts = KINDS[kind];
  const format = useCallback<FormatShortcut>(
    (keys) => formatKeyCombination(keys, osType, t),
    [osType, t],
  );
  const followed = useCallback(
    (entry: EndKey) =>
      kind === "hold"
        ? holdKeyDefaultName(entry, format, (shortcut) =>
            t("settings.endKeys.held", { shortcut }),
          )
        : format(entry.shortcut),
    [kind, format, t],
  );
  const change = (id: string, part: Partial<EndKey>) =>
    onChange(updateEndKey(value, id, part));
  const mayGo = removable ?? canRemove(value);

  return (
    <div className="flex flex-col divide-y divide-line">
      {value.map((entry) => (
        <EndKeyRow
          key={entry.id}
          entry={entry}
          kind={kind}
          followed={followed(entry)}
          hints={hintsOf?.(entry) ?? []}
          removable={mayGo}
          onChange={(part) => change(entry.id, part)}
          onDelete={() => onChange(removeEndKey(value, entry.id))}
          disabled={disabled}
          postProcessingOff={postProcessingOff}
          postProcessingUnreachable={postProcessingUnreachable}
        />
      ))}
      <div className="px-4 py-3">
        <AddKeyButton
          captureId={(now) => `${texts.capturePrefix}:${texts.idPrefix}_${now}`}
          label={t(texts.add)}
          pressKeys={t(texts.addPressKeys)}
          onAdd={(shortcut, now) =>
            onChange([
              ...value,
              newEndKey(value, shortcut, now, texts.idPrefix),
            ])
          }
          disabled={disabled}
        />
      </div>
    </div>
  );
};

interface EndKeyRowProps {
  entry: EndKey;
  kind: EndKeysKind;
  /** The name it follows without one of its own. */
  followed: string;
  /** Grey lines under it, e.g. that a start shortcut has its keys. */
  hints: string[];
  /** It isn't the last one, so it may go. */
  removable: boolean;
  onChange: (part: Partial<EndKey>) => Promise<void> | void;
  onDelete: () => Promise<void> | void;
  disabled: boolean;
  postProcessingOff: boolean;
  postProcessingUnreachable: boolean;
}

const EndKeyRow: React.FC<EndKeyRowProps> = ({
  entry,
  kind,
  followed,
  hints,
  removable,
  onChange,
  onDelete,
  disabled,
  postProcessingOff,
  postProcessingUnreachable,
}) => {
  const { t } = useTranslation();
  const texts = KINDS[kind];
  const shownName = entry.name.trim() !== "" ? entry.name : followed;
  // The field shows the own name or the one it follows as plain text; it
  // is stored when the field loses focus, as nothing when it is the
  // followed one or blank, so the name follows the combination.
  const [name, setName] = useState(shownName);
  useEffect(() => setName(shownName), [shownName]);
  const nameId = `${kind}-key-name-${entry.id}`;

  const storeName = () => {
    const next = nameToStore(name, followed);
    setName(next === "" ? followed : next);
    if (next !== entry.name) void onChange({ name: next });
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
          captureId={`${texts.capturePrefix}:${entry.id}`}
          value={entry.shortcut}
          onChange={(shortcut) => onChange({ shortcut })}
          hint={t(texts.shortcutHint)}
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
              : t(texts.lastOne)
          }
          className="p-1 mb-1 rounded-md text-ink-3 hover:text-text hover:bg-logo-primary/10 disabled:opacity-40 disabled:cursor-not-allowed"
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
          value={entry.append_text}
          onChange={(append_text) => onChange({ append_text })}
          disabled={disabled}
        />
      </div>
      <div className="flex flex-wrap items-center gap-4 text-sm">
        <label className="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            className="accent-logo-primary"
            checked={entry.post_process}
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
            checked={entry.send_context}
            disabled={disabled || !entry.post_process}
            onChange={(event) =>
              void onChange({ send_context: event.target.checked })
            }
          />
          {t("settings.endKeys.sendContext")}
        </label>
      </div>
      {/* DanyWhere: a hold shortcut that post-processes is taken only while
          some dictation can post-process (qa 2026-11-26_1713, question
          4 a); otherwise, with post-processing off for this scope, no end
          post-processes (user, 2026-11-23, option a). Either way the box
          keeps its value for when it counts. */}
      {entry.post_process && postProcessingUnreachable ? (
        <p className="text-xs text-ink-3">{t("settings.holdKeys.notTaken")}</p>
      ) : (
        entry.post_process &&
        postProcessingOff && (
          <p className="text-xs text-ink-3">
            {t("settings.endKeys.postProcessOff")}
          </p>
        )
      )}
      {hints.map((hint) => (
        <p key={hint} className="text-xs text-ink-3">
          {hint}
        </p>
      ))}
    </div>
  );
};
