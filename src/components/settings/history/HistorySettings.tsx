import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { readFile } from "@tauri-apps/plugin-fs";
import {
  Check,
  Copy,
  FileAudio,
  FilePlus,
  FolderOpen,
  RotateCcw,
  Star,
  Trash2,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../../hooks/useSettings";
import { useOsType } from "../../../hooks/useOsType";
import { notify } from "@/stores/noticeStore";
import {
  commands,
  events,
  type HistoryEntry,
  type HistorySearchQuery,
  type HistoryUpdatePayload,
  type Result,
  type SaveVersionMode,
} from "@/bindings";
import {
  useTranscriptionJobsStore,
  type ActiveJobStatus,
} from "@/stores/transcriptionJobsStore";
import { formatDateTime } from "@/utils/dateFormat";
import { AudioPlayer, AudioPlayerGroup } from "../../ui/AudioPlayer";
import { Button } from "../../ui/Button";
import { copyToClipboard } from "./clipboard";
import {
  canRunChosenPrompt,
  postProcessAvailability,
  retranscribeMode,
  retranscribeTitleKey,
  type PostProcessAvailability,
  type ReprocessMode,
} from "./postProcessEntry";
import {
  pickAndTranscribeFiles,
  useFileReadsStore,
  type FileRead,
} from "./transcribeFiles";
// DanyWhere: the window a dictation was made in, and its context.
import {
  inWindow,
  type WindowFilter,
} from "../dictation-context/dictationContext";
import {
  EntryContextInfo,
  EntryWindowLine,
  WindowFilterChip,
  type ShowWindow,
} from "../dictation-context/EntryContext";
// DanyWhere: searching History.
import {
  EMPTY_SEARCH,
  SEARCH_DELAY_MS,
  invalidField,
  isSearchActive,
  matchRanges,
  mergeSearchUpdate,
  searchQuery,
  textMatcher,
  windowMatcher,
  type SearchFields,
  type TextMatcher,
} from "./historySearch";
import { Highlighted, HistorySearchBar } from "./HistorySearchBar";
// DanyWhere: the text in a box that scrolls and can be resized.
import { HistoryTextBox } from "./HistoryTextBox";
// DanyWhere: the star's tooltip says it keeps the recording.
import { keepRecordingTooltip } from "./keepRecording";
// DanyWhere: the versions of an entry: what shows, the track, ✨.
import { PostProcessButton } from "./PostProcessButton";
// DanyWhere (H6, strand E2): the wand's menu with every prompt by name.
import { promptMenuItems, type PromptMenuItem } from "./promptMenu";
import { VersionTrack } from "./VersionTrack";
import {
  defaultSaveMode,
  hasText,
  indexAfterDelete,
  indexAfterMove,
  isSaveShortcut,
  matchingVersions,
  saveModes,
  shownVersion,
  topIndex,
  versionText,
} from "./versions";

const IconButton: React.FC<{
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
  title: string;
  disabled?: boolean;
  /** Greyed out and inert like `disabled`, but still shows its tooltip,
   * which says why. */
  unavailable?: boolean;
  active?: boolean;
  children: React.ReactNode;
}> = ({ onClick, title, disabled, unavailable, active, children }) => (
  <button
    onClick={unavailable ? undefined : onClick}
    disabled={disabled}
    aria-disabled={unavailable || undefined}
    className={`p-1.5 rounded-md flex items-center justify-center transition-colors disabled:cursor-not-allowed disabled:text-text/20 ${
      unavailable
        ? "cursor-not-allowed text-text/20"
        : active
          ? "cursor-pointer text-logo-primary hover:text-logo-primary"
          : "cursor-pointer text-ink-3 hover:text-logo-primary"
    }`}
    title={title}
  >
    {children}
  </button>
);

const PAGE_SIZE = 30;

// The pulse of an entry's text while it's being transcribed.
const TRANSCRIBE_PULSE = {
  animation: "transcribe-pulse 3s ease-in-out infinite",
};

const TranscribePulseKeyframes: React.FC = () => (
  <style>{`
    @keyframes transcribe-pulse {
      0%, 100% { color: var(--color-ink-3); }
      50% { color: var(--color-text); }
    }
  `}</style>
);

// A dropped or picked file while its audio is read, before it's an entry.
const FileReadRow: React.FC<{ read: FileRead }> = ({ read }) => {
  const { t } = useTranslation();
  return (
    <div className="px-4 py-2 pb-5 flex flex-col gap-3">
      <span
        className="flex items-center gap-1 min-w-0 h-7 text-sm text-ink-3"
        title={read.name}
      >
        <FileAudio width={14} height={14} className="shrink-0" />
        <span className="truncate">{read.name}</span>
      </span>
      <p className="italic text-sm w-full" style={TRANSCRIBE_PULSE}>
        <TranscribePulseKeyframes />
        {t("settings.history.readingFile")}
      </p>
    </div>
  );
};

interface HeaderButtonProps {
  onClick: () => void;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
}

const HeaderButton: React.FC<HeaderButtonProps> = ({
  onClick,
  label,
  icon: Icon,
}) => (
  <Button
    onClick={onClick}
    variant="secondary"
    size="sm"
    className="flex items-center gap-2"
    title={label}
  >
    <Icon className="w-4 h-4" />
    <span>{label}</span>
  </Button>
);

export const HistorySettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const postProcess = postProcessAvailability(settings);
  // DanyWhere (H6): the prompts in the order of the profile in use, whose
  // active prompts a click on the wand itself runs.
  const promptItems = useMemo(
    () =>
      promptMenuItems(
        settings?.post_process_prompts ?? [],
        settings?.post_process_prompt_order ?? [],
      ),
    [settings?.post_process_prompts, settings?.post_process_prompt_order],
  );
  const promptMenuUsable = canRunChosenPrompt(settings);
  const jobs = useTranscriptionJobsStore((state) => state.jobs);
  const reads = useFileReadsStore((state) => state.reads);
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const entriesRef = useRef<HistoryEntry[]>([]);
  const loadingRef = useRef(false);
  // DanyWhere: the History of one window (the criterion context is chosen
  // by), and an entry to scroll to once it shows.
  const [windowFilter, setWindowFilter] = useState<WindowFilter | null>(null);
  const [windowEntries, setWindowEntries] = useState<HistoryEntry[]>([]);
  const [scrollToId, setScrollToId] = useState<number | null>(null);
  const windowFilterRef = useRef<WindowFilter | null>(null);
  useEffect(() => {
    windowFilterRef.current = windowFilter;
  }, [windowFilter]);

  const showWindow: ShowWindow = useCallback(async (filter, focusId) => {
    try {
      const result = await commands.getHistoryEntriesInWindow(
        filter.app_id,
        filter.title,
      );
      setWindowEntries(result.status === "ok" ? result.data : []);
    } catch (error) {
      console.error("Failed to load the window's entries:", error);
      setWindowEntries([]);
    }
    setWindowFilter(filter);
    setScrollToId(focusId ?? null);
    // DanyWhere: a jump to one entry ends the search, which might hide it.
    if (focusId !== undefined) {
      setSearchFields(EMPTY_SEARCH);
      setSearch(EMPTY_SEARCH);
    }
  }, []);

  useEffect(() => {
    if (scrollToId === null) return;
    const element = document.getElementById(`history-entry-${scrollToId}`);
    if (element) {
      element.scrollIntoView({ behavior: "smooth", block: "center" });
      setScrollToId(null);
    }
  }, [scrollToId, windowEntries, entries]);

  // DanyWhere: the search: what's typed, what it settled on after a pause,
  // and the pages of entries that match it in the whole history (within the
  // window filter, if one is set).
  const [searchFields, setSearchFields] = useState<SearchFields>(EMPTY_SEARCH);
  const [search, setSearch] = useState<SearchFields>(EMPTY_SEARCH);
  const [searchEntries, setSearchEntries] = useState<HistoryEntry[]>([]);
  const [searchHasMore, setSearchHasMore] = useState(false);
  const [searchLoading, setSearchLoading] = useState(false);
  const [searchInvalid, setSearchInvalid] = useState<"text" | "window" | null>(
    null,
  );
  const searchActive = isSearchActive(search);
  const query = useMemo(
    () => (searchActive ? searchQuery(search, windowFilter) : null),
    [searchActive, search, windowFilter],
  );
  const matcher = useMemo(
    () => (searchActive ? textMatcher(search) : null),
    [searchActive, search],
  );
  const windowHighlight = useMemo(
    () => (searchActive ? windowMatcher(search) : null),
    [searchActive, search],
  );
  const queryRef = useRef<HistorySearchQuery | null>(null);
  const searchEntriesRef = useRef<HistoryEntry[]>([]);
  const searchHasMoreRef = useRef(false);
  const searchLoadingRef = useRef(false);
  // Counts the queries, so a late answer to an earlier one is dropped.
  const searchGeneration = useRef(0);
  // Live updates are checked one after another, in the order they came.
  const searchUpdates = useRef<Promise<void>>(Promise.resolve());
  useEffect(() => {
    searchEntriesRef.current = searchEntries;
  }, [searchEntries]);
  useEffect(() => {
    searchHasMoreRef.current = searchHasMore;
  }, [searchHasMore]);

  useEffect(() => {
    const timer = setTimeout(() => setSearch(searchFields), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [searchFields]);

  const loadSearchPage = useCallback(
    async (pageQuery: HistorySearchQuery, cursor?: number) => {
      const isFirstPage = cursor === undefined;
      if (!isFirstPage && searchLoadingRef.current) return;
      const generation = searchGeneration.current;
      searchLoadingRef.current = true;
      if (isFirstPage) setSearchLoading(true);
      try {
        const result = await commands.searchHistoryEntries(
          pageQuery,
          cursor ?? null,
          PAGE_SIZE,
        );
        if (generation !== searchGeneration.current) return;
        if (result.status === "ok") {
          const { entries: found, has_more } = result.data;
          setSearchInvalid(null);
          setSearchEntries((prev) => {
            if (isFirstPage) return found;
            const shown = new Set(prev.map((entry) => entry.id));
            return [...prev, ...found.filter((entry) => !shown.has(entry.id))];
          });
          setSearchHasMore(has_more);
        } else {
          // An invalid pattern searches nothing; the last results stay.
          const field = invalidField(result.error);
          setSearchInvalid(field);
          if (field === null) {
            console.error("Failed to search the history:", result.error);
          }
        }
      } catch (error) {
        console.error("Failed to search the history:", error);
      } finally {
        if (generation === searchGeneration.current) {
          setSearchLoading(false);
          searchLoadingRef.current = false;
        }
      }
    },
    [],
  );

  useEffect(() => {
    searchGeneration.current += 1;
    queryRef.current = query;
    searchLoadingRef.current = false;
    if (!query) {
      setSearchEntries([]);
      setSearchHasMore(false);
      setSearchInvalid(null);
      setSearchLoading(false);
      return;
    }
    void loadSearchPage(query);
  }, [query, loadSearchPage]);

  // A new or changed entry joins the results only if it matches.
  const updateSearch = useCallback((entry: HistoryEntry) => {
    const updateQuery = queryRef.current;
    if (!updateQuery) return;
    const generation = searchGeneration.current;
    searchUpdates.current = searchUpdates.current.then(async () => {
      const shown = searchEntriesRef.current.some((e) => e.id === entry.id);
      let matches = shown;
      if (!shown) {
        try {
          const result = await commands.historyEntryMatchesSearch(
            entry.id,
            updateQuery,
          );
          matches = result.status === "ok" && result.data;
        } catch (error) {
          console.error("Failed to check an entry against the search:", error);
        }
      }
      if (generation !== searchGeneration.current) return;
      setSearchEntries((prev) => {
        const next = mergeSearchUpdate(
          prev,
          entry,
          matches,
          searchHasMoreRef.current,
        );
        searchEntriesRef.current = next;
        return next;
      });
    });
  }, []);

  // DanyWhere: a change to an entry reaches every list.
  const updateEntries = (change: (list: HistoryEntry[]) => HistoryEntry[]) => {
    setEntries(change);
    setWindowEntries(change);
    setSearchEntries(change);
  };

  // Keep ref in sync for use in IntersectionObserver callback
  useEffect(() => {
    entriesRef.current = entries;
  }, [entries]);

  const loadPage = useCallback(async (cursor?: number) => {
    const isFirstPage = cursor === undefined;
    if (!isFirstPage && loadingRef.current) return;
    loadingRef.current = true;

    if (isFirstPage) setLoading(true);

    try {
      const result = await commands.getHistoryEntries(
        cursor ?? null,
        PAGE_SIZE,
      );
      if (result.status === "ok") {
        const { entries: newEntries, has_more } = result.data;
        setEntries((prev) => {
          if (isFirstPage) return newEntries;
          const existingIds = new Set(prev.map((entry) => entry.id));
          return [
            ...prev,
            ...newEntries.filter((entry) => !existingIds.has(entry.id)),
          ];
        });
        setHasMore(has_more);
      }
    } catch (error) {
      console.error("Failed to load history entries:", error);
    } finally {
      setLoading(false);
      loadingRef.current = false;
    }
  }, []);

  // Initial load
  useEffect(() => {
    loadPage();
  }, [loadPage]);

  // Infinite scroll via IntersectionObserver
  // DanyWhere: a search pages through its own results; the observer starts
  // again after each page, so a short page that leaves the end in view
  // loads the next.
  const searchCount = searchEntries.length;
  useEffect(() => {
    if (loading) return;

    const sentinel = sentinelRef.current;
    const more = query ? searchHasMore && !searchLoading : hasMore;
    if (!sentinel || !more) return;

    const observer = new IntersectionObserver(
      (observerEntries) => {
        const first = observerEntries[0];
        if (first.isIntersecting) {
          const pageQuery = queryRef.current;
          const list = pageQuery
            ? searchEntriesRef.current
            : entriesRef.current;
          const lastEntry = list[list.length - 1];
          if (lastEntry && pageQuery) {
            void loadSearchPage(pageQuery, lastEntry.id);
          } else if (lastEntry) {
            loadPage(lastEntry.id);
          }
        }
      },
      { threshold: 0 },
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [
    loading,
    hasMore,
    loadPage,
    query,
    searchHasMore,
    searchLoading,
    searchCount,
    loadSearchPage,
  ]);

  // Listen for new entries added from the transcription pipeline
  useEffect(() => {
    const unlisten = events.historyUpdatePayload.listen((event) => {
      const payload: HistoryUpdatePayload = event.payload;
      if (payload.action === "added") {
        setEntries((prev) => [
          payload.entry,
          ...prev.filter((entry) => entry.id !== payload.entry.id),
        ]);
        // DanyWhere: a new dictation in the shown window shows there too.
        const filter = windowFilterRef.current;
        if (filter && inWindow(payload.entry.window_context, filter)) {
          setWindowEntries((prev) => [
            payload.entry,
            ...prev.filter((entry) => entry.id !== payload.entry.id),
          ]);
        }
      } else if (payload.action === "updated") {
        setEntries((prev) =>
          prev.map((e) => (e.id === payload.entry.id ? payload.entry : e)),
        );
        // DanyWhere: the window's list too; a dictation whose window was
        // stored only now joins it.
        const filter = windowFilterRef.current;
        setWindowEntries((prev) => {
          if (prev.some((e) => e.id === payload.entry.id)) {
            return prev.map((e) =>
              e.id === payload.entry.id ? payload.entry : e,
            );
          }
          if (filter && inWindow(payload.entry.window_context, filter)) {
            return [payload.entry, ...prev].sort(
              (a, b) => b.timestamp - a.timestamp || b.id - a.id,
            );
          }
          return prev;
        });
      }
      // DanyWhere: and the search's results, if it matches.
      if (payload.action === "added" || payload.action === "updated") {
        updateSearch(payload.entry);
      }
      // "deleted" and "toggled" are handled by optimistic updates only,
      // so we intentionally ignore them here to avoid double-mutation.
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [updateSearch]);

  const toggleSaved = async (id: number) => {
    // Optimistic update
    updateEntries((prev) =>
      prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
    );
    try {
      const result = await commands.toggleHistoryEntrySaved(id);
      if (result.status !== "ok") {
        // Revert on failure
        updateEntries((prev) =>
          prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
        );
      }
    } catch (error) {
      console.error("Failed to toggle saved status:", error);
      // Revert on failure
      updateEntries((prev) =>
        prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
      );
    }
  };

  // Recordings are Opus, which older webviews can't play: the backend hands
  // over a WAV (decoded if need be), played from memory on every OS.
  const getAudioUrl = useCallback(async (fileName: string) => {
    try {
      const result = await commands.getRecordingPlaybackPath(fileName);
      if (result.status === "ok") {
        const fileData = await readFile(result.data);
        const blob = new Blob([fileData], { type: "audio/wav" });
        return URL.createObjectURL(blob);
      }
      return null;
    } catch (error) {
      console.error("Failed to load the recording:", error);
      return null;
    }
  }, []);

  const deleteAudioEntry = async (id: number) => {
    // Optimistically remove
    updateEntries((prev) => prev.filter((e) => e.id !== id));
    try {
      const result = await commands.deleteHistoryEntry(id);
      if (result.status !== "ok") {
        throw new Error(String(result.error));
      }
    } catch (error) {
      console.error("Failed to delete entry:", error);
      loadPage();
      // DanyWhere: the search's results too.
      if (queryRef.current) void loadSearchPage(queryRef.current);
      throw error;
    }
  };

  const transcribeFile = async () => {
    try {
      await pickAndTranscribeFiles();
    } catch (error) {
      console.error("Failed to pick a file to transcribe:", error);
      notify({ title: t("settings.history.transcribeFailed") });
    }
  };

  const openRecordingsFolder = async () => {
    try {
      const result = await commands.openRecordingsFolder();
      if (result.status !== "ok") {
        throw new Error(String(result.error));
      }
    } catch (error) {
      console.error("Failed to open recordings folder:", error);
    }
  };

  let content: React.ReactNode;
  // DanyWhere: what the search found, else one window's entries while
  // filtered.
  const shownEntries = query
    ? searchEntries
    : windowFilter
      ? windowEntries
      : entries;

  if (loading || (query && searchLoading && searchEntries.length === 0)) {
    content = (
      <div className="px-4 py-3 text-center text-ink-2">
        {t("settings.history.loading")}
      </div>
    );
  } else if (shownEntries.length === 0 && reads.length === 0) {
    content = (
      <div className="px-4 py-3 text-center text-ink-2">
        {query
          ? searchInvalid
            ? t("settings.history.search.invalidPattern")
            : t("settings.history.search.noMatches")
          : t("settings.history.empty")}
      </div>
    );
  } else {
    content = (
      <>
        <AudioPlayerGroup>
          <div className="divide-y divide-line">
            {reads.map((read) => (
              <FileReadRow key={`read-${read.id}`} read={read} />
            ))}
            {shownEntries.map((entry, index) => (
              <HistoryEntryComponent
                key={entry.id}
                entry={entry}
                newest={index === 0}
                onShowWindow={showWindow}
                highlight={matcher}
                windowHighlight={windowHighlight}
                job={jobs[entry.id]}
                onToggleSaved={() => toggleSaved(entry.id)}
                onCopyText={(text) => copyToClipboard(text)}
                getAudioUrl={getAudioUrl}
                deleteAudio={deleteAudioEntry}
                postProcess={postProcess}
                // DanyWhere: with the profile's active prompts, or the one
                // chosen in the wand's menu (`prompt_library`).
                reprocess={(id, mode, version, promptIds) =>
                  commands.reprocessHistoryEntry(id, mode, version, promptIds)
                }
                promptItems={promptItems}
                promptMenuUsable={promptMenuUsable}
                cancelTranscription={commands.cancelHistoryEntryTranscription}
                saveVersion={commands.saveHistoryVersion}
                moveVersion={commands.moveHistoryVersion}
                deleteVersion={commands.deleteHistoryVersion}
              />
            ))}
          </div>
        </AudioPlayerGroup>
        {/* Sentinel for infinite scroll (DanyWhere: not while one window
            shows, which is loaded whole, unless a search pages it) */}
        {(query || !windowFilter) && <div ref={sentinelRef} className="h-1" />}
      </>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="space-y-2">
        <div className="px-4 flex items-center justify-between">
          <div>
            <h2 className="text-xs font-medium text-ink-3 uppercase tracking-wide">
              {t("settings.history.title")}
            </h2>
          </div>
          <div className="flex items-center gap-2">
            <HeaderButton
              onClick={transcribeFile}
              label={t("settings.history.transcribeFile")}
              icon={FilePlus}
            />
            <HeaderButton
              onClick={openRecordingsFolder}
              label={t("settings.history.openFolder")}
              icon={FolderOpen}
            />
          </div>
        </div>
        {/* DanyWhere: searching History. */}
        <HistorySearchBar
          fields={searchFields}
          onChange={setSearchFields}
          invalid={searchInvalid}
        />
        {windowFilter && (
          <WindowFilterChip
            filter={windowFilter}
            count={windowEntries.length}
            onClear={() => {
              setWindowFilter(null);
              setWindowEntries([]);
            }}
          />
        )}
        <div className="bg-background border border-line rounded-lg overflow-visible">
          {content}
        </div>
      </div>
    </div>
  );
};

interface HistoryEntryProps {
  entry: HistoryEntry;
  // DanyWhere: the first entry of the list shown, whose text box has a
  // height of its own.
  newest: boolean;
  // DanyWhere: shows the History of one window.
  onShowWindow: ShowWindow;
  // DanyWhere: what the search highlights in the text, and in the window
  // line and the file name.
  highlight: TextMatcher | null;
  windowHighlight: TextMatcher | null;
  job?: ActiveJobStatus;
  onToggleSaved: () => void;
  onCopyText: (text: string) => Promise<boolean>;
  getAudioUrl: (fileName: string) => Promise<string | null>;
  deleteAudio: (id: number) => Promise<void>;
  postProcess: PostProcessAvailability;
  // DanyWhere: `version` is the version a post-processing starts from
  // (null: the top); `promptIds` the prompts to run instead of the active
  // ones (null), from the wand's menu.
  reprocess: (
    id: number,
    mode: ReprocessMode,
    version: number | null,
    promptIds: string[] | null,
  ) => Promise<void>;
  // DanyWhere (H6): the wand's menu.
  promptItems: PromptMenuItem[];
  promptMenuUsable: boolean;
  cancelTranscription: (id: number) => Promise<void>;
  // DanyWhere: the entry's versions (`history_versions`).
  saveVersion: (
    id: number,
    base: number,
    text: string,
    mode: SaveVersionMode,
  ) => Promise<Result<HistoryEntry, string>>;
  moveVersion: (
    id: number,
    from: number,
    to: number,
  ) => Promise<Result<HistoryEntry, string>>;
  deleteVersion: (
    id: number,
    index: number,
  ) => Promise<Result<HistoryEntry, string>>;
}

const VERSIONS = "settings.history.versions";

const HistoryEntryComponent: React.FC<HistoryEntryProps> = ({
  entry,
  newest,
  onShowWindow,
  highlight,
  windowHighlight,
  job,
  onToggleSaved,
  onCopyText,
  getAudioUrl,
  deleteAudio,
  postProcess,
  reprocess,
  promptItems,
  promptMenuUsable,
  cancelTranscription,
  saveVersion,
  moveVersion,
  deleteVersion,
}) => {
  const { t, i18n } = useTranslation();
  const osType = useOsType();
  const [showCopied, setShowCopied] = useState(false);
  // Pressed cancel: a running job stops after its current chunk.
  const [cancelling, setCancelling] = useState(false);
  // DanyWhere: the version the user chose on the track (null: the top), and
  // the search during which they chose it; during that search the entry
  // keeps their choice instead of showing the version that matches.
  const [chosen, setChosen] = useState<number | null>(null);
  const [switchedIn, setSwitchedIn] = useState<TextMatcher | null>(null);
  // DanyWhere: the version being edited, and the text typed so far.
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);

  // Queued, or being transcribed or post-processed in the background.
  const retrying = job !== undefined;
  useEffect(() => {
    if (!retrying) setCancelling(false);
  }, [retrying]);
  // DanyWhere: the entry's result, the top version, whichever made it: the
  // transcription may be deleted (`history_versions`).
  const hasTranscription = hasText(entry);
  // DanyWhere: the version shown; during a search the one that matches.
  const shown = shownVersion(
    entry,
    highlight,
    chosen,
    highlight !== null && switchedIn === highlight,
  );
  const text = versionText(entry, shown);
  const matching = useMemo(
    () => matchingVersions(entry, highlight),
    [entry, highlight],
  );
  // DanyWhere: where the search matches the shown text and the file name.
  const highlights = useMemo(
    () => matchRanges(text, highlight),
    [text, highlight],
  );
  const fileHighlights = useMemo(
    () =>
      entry.source_name ? matchRanges(entry.source_name, windowHighlight) : [],
    [entry.source_name, windowHighlight],
  );

  /** Shows version `index` (null: the top) as the user's choice. */
  const show = (index: number | null) => {
    setChosen(index);
    setSwitchedIn(highlight);
  };

  const handleLoadAudio = useCallback(
    () => getAudioUrl(entry.file_name),
    [getAudioUrl, entry.file_name],
  );

  const handleCopyText = async () => {
    if (!hasTranscription) {
      return;
    }

    const copied = await onCopyText(text);
    if (!copied) {
      notify({ title: t("settings.history.copyError") });
      return;
    }

    setShowCopied(true);
    setTimeout(() => setShowCopied(false), 2000);
  };

  // DanyWhere: an edit of a version below the top is saved on top as a new
  // version; nothing goes, so nothing asks.
  const startEditing = (base: number) => {
    if (retrying || editing !== null || saving) return;
    setDraft(versionText(entry, base));
    setEditing(base);
  };

  const saveModesOfEdit = editing === null ? [] : saveModes(entry, editing);
  const defaultMode = editing === null ? null : defaultSaveMode(entry, editing);

  const saveEdit = async (mode: SaveVersionMode) => {
    if (editing === null || saving || draft.trim().length === 0) return;
    // Unchanged: nothing to save, and nothing above it goes.
    if (draft.trim() === versionText(entry, editing).trim()) {
      setEditing(null);
      return;
    }
    setSaving(true);
    try {
      const result = await saveVersion(entry.id, editing, draft, mode);
      if (result.status !== "ok") throw new Error(String(result.error));
      setEditing(null);
      // What was saved is the top now.
      show(null);
    } catch (error) {
      console.error("Failed to save the version:", error);
      notify({ title: t(`${VERSIONS}.saveError`) });
    } finally {
      setSaving(false);
    }
  };

  const handleEditKeyDown = (
    event: React.KeyboardEvent<HTMLTextAreaElement>,
  ) => {
    if (event.key === "Escape") {
      event.preventDefault();
      setEditing(null);
    } else if (isSaveShortcut(event) && defaultMode !== null) {
      event.preventDefault();
      void saveEdit(defaultMode);
    }
  };

  // DanyWhere: moving or deleting a version, without asking. The version
  // shown stays in view: it follows a move, and when it is the one deleted
  // the entry shows its top (`follow` null).
  const changeVersions = async (
    change: () => Promise<Result<HistoryEntry, string>>,
    follow: number | null,
    topAfter: number,
  ) => {
    setSaving(true);
    try {
      const result = await change();
      if (result.status !== "ok") throw new Error(String(result.error));
      show(follow === null || follow === topAfter ? null : follow);
    } catch (error) {
      console.error("Failed to move or delete a version:", error);
      notify({ title: t(`${VERSIONS}.changeError`) });
    } finally {
      setSaving(false);
    }
  };

  const moveVersionTo = (from: number, to: number) =>
    void changeVersions(
      () => moveVersion(entry.id, from, to),
      indexAfterMove(shown, from, to),
      topIndex(entry),
    );

  const deleteVersionAt = (index: number) =>
    void changeVersions(
      () => deleteVersion(entry.id, index),
      indexAfterDelete(shown, index),
      topIndex(entry) - 1,
    );

  // DanyWhere: ✨ post-processes the current version, the one shown; the
  // result goes on top, where the entry then shows it. Every version stays.
  const runPostProcess = () => {
    void reprocess(entry.id, "post_process", shown, null);
    show(null);
  };

  // DanyWhere (H6): one prompt of the library, chosen by name, on the
  // version shown; its result goes on top too.
  const runPrompt = (promptId: string) => {
    void reprocess(entry.id, "post_process", shown, [promptId]);
    show(null);
  };

  // DanyWhere: the new transcription goes on top; every version stays.
  const runRetranscribe = (mode: ReprocessMode) => {
    void reprocess(entry.id, mode, null, null);
    show(null);
  };

  const handleDeleteEntry = async () => {
    try {
      await deleteAudio(entry.id);
    } catch (error) {
      console.error("Failed to delete entry:", error);
      notify({ title: t("settings.history.deleteError") });
    }
  };

  const formattedDate = formatDateTime(String(entry.timestamp), i18n.language);
  const saveKeys = osType === "macos" ? "⌘↩" : "Ctrl+Enter";

  let progressText = t("settings.history.transcribing");
  if (job?.state === "queued") {
    progressText = t("settings.history.waiting");
  } else if (job?.state === "post_processing") {
    progressText = t("settings.history.postProcessing");
  } else if (job?.state === "running" && job.percent > 0) {
    progressText = t("settings.history.transcribingPercent", {
      percent: job.percent,
    });
  }

  return (
    <div
      id={`history-entry-${entry.id}`}
      className="px-4 py-2 pb-5 flex flex-col gap-3"
    >
      <div className="flex justify-between items-center gap-3">
        <div className="flex items-center gap-2 min-w-0">
          <p className="text-sm font-medium shrink-0">{formattedDate}</p>
          {entry.source_name && (
            <span
              className="flex items-center gap-1 min-w-0 text-sm text-ink-3"
              title={entry.source_name}
            >
              <FileAudio width={14} height={14} className="shrink-0" />
              <span className="truncate">
                <Highlighted text={entry.source_name} ranges={fileHighlights} />
              </span>
            </span>
          )}
        </div>
        <div className="flex items-center shrink-0">
          <IconButton
            onClick={handleCopyText}
            disabled={!hasTranscription || retrying}
            title={t("settings.history.copyToClipboard")}
          >
            {showCopied ? (
              <Check width={16} height={16} />
            ) : (
              <Copy width={16} height={16} />
            )}
          </IconButton>
          <IconButton
            onClick={onToggleSaved}
            active={entry.saved}
            title={keepRecordingTooltip(entry.saved, (key) => t(key))}
          >
            <Star
              width={16}
              height={16}
              fill={entry.saved ? "currentColor" : "none"}
            />
          </IconButton>
          {retrying ? (
            <IconButton
              onClick={() => {
                setCancelling(true);
                void cancelTranscription(entry.id);
              }}
              disabled={cancelling}
              title={t("settings.history.cancelTranscription")}
            >
              <X width={16} height={16} />
            </IconButton>
          ) : (
            <>
              {/* DanyWhere: ✨ on the version shown. */}
              {hasTranscription && (
                <PostProcessButton
                  availability={postProcess}
                  onRun={runPostProcess}
                  prompts={promptItems}
                  onRunPrompt={runPrompt}
                  menuUsable={promptMenuUsable}
                  disabled={editing !== null || saving}
                />
              )}
              {!entry.recording_removed && (
                <IconButton
                  onClick={(event) =>
                    runRetranscribe(
                      retranscribeMode(event.shiftKey, postProcess),
                    )
                  }
                  disabled={editing !== null || saving}
                  title={t(
                    retranscribeTitleKey(
                      entry.post_process_requested,
                      postProcess,
                    ),
                  )}
                >
                  <RotateCcw width={16} height={16} />
                </IconButton>
              )}
            </>
          )}
          <IconButton
            onClick={handleDeleteEntry}
            title={t("settings.history.delete")}
          >
            <Trash2 width={16} height={16} />
          </IconButton>
        </div>
      </div>

      {/* DanyWhere: the window it was dictated in; shows that window. */}
      <EntryWindowLine
        entry={entry}
        onShowWindow={onShowWindow}
        highlight={windowHighlight}
      />

      <div className="flex flex-col items-start gap-1 pb-2">
        {/* DanyWhere: the text in a box that scrolls and can be resized; a
            click into it, or its hidden edit button (Tab, then Enter or
            Space), edits the version shown (no pencil since the user found
            it superfluous, 2026-11-16). */}
        {hasTranscription && !retrying && (
          <HistoryTextBox
            text={text}
            highlights={highlights}
            newest={newest}
            editing={editing !== null}
            draft={draft}
            onDraftChange={setDraft}
            onKeyDown={handleEditKeyDown}
            onActivate={
              editing === null && !saving
                ? () => startEditing(shown)
                : undefined
            }
            editLabel={t("settings.history.edit")}
            className="italic"
          />
        )}
        {(retrying || !hasTranscription) && (
          <p
            className={`italic text-sm w-full ${retrying ? "" : "text-ink-3"}`}
            style={retrying ? TRANSCRIBE_PULSE : undefined}
          >
            {retrying && <TranscribePulseKeyframes />}
            {retrying
              ? progressText
              : entry.source_name
                ? // Cancelled, interrupted by a quit, or failed.
                  t("settings.history.notTranscribed")
                : t("settings.history.transcriptionFailed")}
          </p>
        )}
        {/* DanyWhere: the saves the version allows; the default one is
            highlighted and on ⌘↩. */}
        {editing !== null && !retrying && (
          <div className="flex flex-wrap items-center gap-2 pt-1">
            {saveModesOfEdit.map((mode) => {
              const label =
                mode === "replace"
                  ? t(`${VERSIONS}.save`)
                  : t(`${VERSIONS}.saveAsNew`);
              const hint =
                mode === "replace"
                  ? t(`${VERSIONS}.saveHint`)
                  : t(`${VERSIONS}.saveAsNewHint`);
              return (
                <Button
                  key={mode}
                  size="sm"
                  variant={mode === defaultMode ? "primary" : "secondary"}
                  onClick={() => void saveEdit(mode)}
                  disabled={saving || draft.trim().length === 0}
                  title={
                    mode === defaultMode
                      ? t(`${VERSIONS}.withShortcut`, {
                          hint,
                          keys: saveKeys,
                        })
                      : hint
                  }
                >
                  {label}
                </Button>
              );
            })}
            <Button
              size="sm"
              variant="secondary"
              onClick={() => setEditing(null)}
            >
              {t(`${VERSIONS}.cancel`)}
            </Button>
          </div>
        )}
        {/* DanyWhere: the versions, when there is more than one. */}
        {hasTranscription && !retrying && editing === null && (
          <VersionTrack
            entry={entry}
            shown={shown}
            matching={matching}
            onShow={show}
            onMove={moveVersionTo}
            onDelete={deleteVersionAt}
            disabled={saving}
          />
        )}
        {/* DanyWhere: its context, and whether it went out as context. */}
        {!retrying && editing === null && (
          <EntryContextInfo entry={entry} onShowWindow={onShowWindow} />
        )}
      </div>

      {entry.recording_removed ? (
        <p className="text-xs text-ink-3">
          {t("settings.history.recordingRemoved")}
        </p>
      ) : (
        <AudioPlayer onLoadRequest={handleLoadAudio} className="w-full" />
      )}
    </div>
  );
};
