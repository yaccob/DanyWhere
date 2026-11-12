// DanyWhere: what History shows about context from earlier dictations
// (`dictation_context.rs`): the window an entry was dictated in (the
// criterion context is chosen by, so the user sees what to correct), the
// entries that went along as its context, and whether it went out as context
// itself.
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { AppWindow, ChevronDown, ChevronRight, Send, X } from "lucide-react";
import { commands, type HistoryEntry } from "@/bindings";
import { formatDateTime } from "@/utils/dateFormat";
import { shownText } from "../history/postProcessEntry";
import {
  firstWords,
  windowFilterOf,
  type WindowFilter,
} from "./dictationContext";

/** How many words of an earlier entry the context list shows. */
const PREVIEW_WORDS = 8;

/** Shows the History of one window, optionally scrolled to an entry. */
export type ShowWindow = (filter: WindowFilter, focusId?: number) => void;

/** The window an entry was dictated in, on a line of its own so a long
 * title stays readable; a click shows only that window's entries. */
export const EntryWindowLine: React.FC<{
  entry: HistoryEntry;
  onShowWindow: ShowWindow;
}> = ({ entry, onShowWindow }) => {
  const { t } = useTranslation();
  const filter = windowFilterOf(entry.window_context);
  if (!filter) return null;
  return (
    <button
      type="button"
      onClick={() => onShowWindow(filter)}
      title={`${filter.label}\n${t("settings.history.showWindowEntries")}`}
      className="flex items-start gap-1 w-full text-start text-xs text-ink-3 hover:text-logo-primary cursor-pointer"
    >
      <AppWindow width={12} height={12} className="shrink-0 mt-0.5" />
      <span className="line-clamp-2 break-words">{filter.label}</span>
    </button>
  );
};

/** Under the text: the entries that went along as context (to open and
 * jump to), and whether this entry went out as context of a later one. */
export const EntryContextInfo: React.FC<{
  entry: HistoryEntry;
  onShowWindow: ShowWindow;
}> = ({ entry, onShowWindow }) => {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const [contextEntries, setContextEntries] = useState<HistoryEntry[] | null>(
    null,
  );
  const context = entry.window_context;
  const ids = context?.context_entry_ids ?? [];
  const requested = context?.context_requested ?? false;
  const sentAt = context?.sent_as_context_at ?? null;
  if (!requested && sentAt === null) return null;

  const toggle = async () => {
    const next = !open;
    setOpen(next);
    if (next && contextEntries === null) {
      try {
        const result = await commands.getHistoryEntriesByIds(ids);
        setContextEntries(result.status === "ok" ? result.data : []);
      } catch (error) {
        console.error("Failed to load the context entries:", error);
        setContextEntries([]);
      }
    }
  };

  const ownWindow = windowFilterOf(context);

  return (
    <div className="flex flex-col gap-1 w-full text-xs text-ink-3">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        {requested &&
          (ids.length > 0 ? (
            <button
              type="button"
              onClick={() => void toggle()}
              aria-expanded={open}
              className="flex items-center gap-1 font-medium hover:text-logo-primary cursor-pointer"
            >
              {open ? (
                <ChevronDown width={12} height={12} />
              ) : (
                <ChevronRight width={12} height={12} />
              )}
              {t("settings.history.contextUsed", { count: ids.length })}
            </button>
          ) : (
            <span>{t("settings.history.contextUsed", { count: 0 })}</span>
          ))}
        {sentAt !== null && (
          <span
            className="flex items-center gap-1"
            title={t("settings.history.contextSentTooltip", {
              date: formatDateTime(String(sentAt), i18n.language),
            })}
          >
            <Send width={12} height={12} />
            {t("settings.history.contextSent")}
          </span>
        )}
      </div>
      {open && (
        <ul className="flex flex-col gap-1 ps-4">
          {contextEntries === null ? (
            <li>…</li>
          ) : (
            contextEntries.map((earlier) => {
              const filter =
                windowFilterOf(earlier.window_context) ?? ownWindow;
              return (
                <li key={earlier.id}>
                  <button
                    type="button"
                    disabled={!filter}
                    onClick={() => filter && onShowWindow(filter, earlier.id)}
                    title={t("settings.history.contextEntryJump")}
                    className="text-start hover:text-logo-primary cursor-pointer disabled:cursor-default"
                  >
                    <span className="font-medium">
                      {formatDateTime(String(earlier.timestamp), i18n.language)}
                    </span>{" "}
                    {firstWords(shownText(earlier, false), PREVIEW_WORDS)}
                  </button>
                </li>
              );
            })
          )}
        </ul>
      )}
    </div>
  );
};

/** Above the list while it shows one window: which one, and the way back. */
export const WindowFilterChip: React.FC<{
  filter: WindowFilter;
  count: number;
  onClear: () => void;
}> = ({ filter, count, onClear }) => {
  const { t } = useTranslation();
  return (
    <div className="px-4 flex flex-wrap items-center gap-2">
      <span
        className="flex items-start gap-1 min-w-0 max-w-full rounded-md border border-line px-2 py-1 text-xs text-ink-2"
        title={filter.label}
      >
        <AppWindow width={12} height={12} className="shrink-0 mt-0.5" />
        <span className="break-words">
          {t("settings.history.windowFilter", {
            window: filter.label,
            count,
          })}
        </span>
      </span>
      <button
        type="button"
        onClick={onClear}
        className="flex items-center gap-1 rounded-md border border-logo-primary px-2 py-1 text-xs font-medium text-logo-primary hover:bg-logo-primary/10 cursor-pointer"
      >
        <X width={12} height={12} />
        {t("settings.history.showAll")}
      </button>
    </div>
  );
};
