import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { readFile } from "@tauri-apps/plugin-fs";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  Check,
  Copy,
  FileAudio,
  FilePlus,
  FolderOpen,
  Pencil,
  RotateCcw,
  Star,
  Trash2,
  WandSparkles,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../../hooks/useSettings";
import { notify } from "@/stores/noticeStore";
import {
  commands,
  events,
  type CorrectionPart,
  type HistoryEntry,
  type HistoryUpdatePayload,
  type Result,
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
  correctionsLostBy,
  isCorrected,
  isSaveShortcut,
  shownPart,
} from "./corrections";
import {
  postProcessAvailability,
  postProcessedText,
  retranscribeMode,
  shownText,
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

// Long text folds to this many lines until "Show more".
const FOLDED_LINES = 4;

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
  }, []);

  useEffect(() => {
    if (scrollToId === null) return;
    const element = document.getElementById(`history-entry-${scrollToId}`);
    if (element) {
      element.scrollIntoView({ behavior: "smooth", block: "center" });
      setScrollToId(null);
    }
  }, [scrollToId, windowEntries, entries]);

  // DanyWhere: a change to an entry reaches both lists.
  const updateEntries = (change: (list: HistoryEntry[]) => HistoryEntry[]) => {
    setEntries(change);
    setWindowEntries(change);
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
  useEffect(() => {
    if (loading) return;

    const sentinel = sentinelRef.current;
    if (!sentinel || !hasMore) return;

    const observer = new IntersectionObserver(
      (observerEntries) => {
        const first = observerEntries[0];
        if (first.isIntersecting) {
          const lastEntry = entriesRef.current[entriesRef.current.length - 1];
          if (lastEntry) {
            loadPage(lastEntry.id);
          }
        }
      },
      { threshold: 0 },
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [loading, hasMore, loadPage]);

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
      // "deleted" and "toggled" are handled by optimistic updates only,
      // so we intentionally ignore them here to avoid double-mutation.
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

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
  // DanyWhere: one window's entries while filtered.
  const shownEntries = windowFilter ? windowEntries : entries;

  if (loading) {
    content = (
      <div className="px-4 py-3 text-center text-ink-2">
        {t("settings.history.loading")}
      </div>
    );
  } else if (shownEntries.length === 0 && reads.length === 0) {
    content = (
      <div className="px-4 py-3 text-center text-ink-2">
        {t("settings.history.empty")}
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
            {shownEntries.map((entry) => (
              <HistoryEntryComponent
                key={entry.id}
                entry={entry}
                onShowWindow={showWindow}
                job={jobs[entry.id]}
                onToggleSaved={() => toggleSaved(entry.id)}
                onCopyText={(text) => copyToClipboard(text)}
                getAudioUrl={getAudioUrl}
                deleteAudio={deleteAudioEntry}
                postProcess={postProcess}
                reprocess={commands.reprocessHistoryEntry}
                cancelTranscription={commands.cancelHistoryEntryTranscription}
                correct={commands.correctHistoryEntry}
                deletePostProcessing={commands.deleteHistoryEntryPostProcessing}
              />
            ))}
          </div>
        </AudioPlayerGroup>
        {/* Sentinel for infinite scroll (DanyWhere: not while one window
            shows, which is loaded whole) */}
        {!windowFilter && <div ref={sentinelRef} className="h-1" />}
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
  // DanyWhere: shows the History of one window.
  onShowWindow: ShowWindow;
  job?: ActiveJobStatus;
  onToggleSaved: () => void;
  onCopyText: (text: string) => Promise<boolean>;
  getAudioUrl: (fileName: string) => Promise<string | null>;
  deleteAudio: (id: number) => Promise<void>;
  postProcess: PostProcessAvailability;
  reprocess: (id: number, mode: ReprocessMode) => Promise<void>;
  cancelTranscription: (id: number) => Promise<void>;
  // DanyWhere: corrections in History.
  correct: (
    id: number,
    part: CorrectionPart,
    text: string | null,
  ) => Promise<Result<HistoryEntry, string>>;
  deletePostProcessing: (id: number) => Promise<Result<HistoryEntry, string>>;
}

const HistoryEntryComponent: React.FC<HistoryEntryProps> = ({
  entry,
  onShowWindow,
  job,
  onToggleSaved,
  onCopyText,
  getAudioUrl,
  deleteAudio,
  postProcess,
  reprocess,
  cancelTranscription,
  correct,
  deletePostProcessing,
}) => {
  const { t, i18n } = useTranslation();
  const [showCopied, setShowCopied] = useState(false);
  // Pressed cancel: a running job stops after its current chunk.
  const [cancelling, setCancelling] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  // A post-processed entry shows the post-processed text unless the
  // original is asked for.
  const [showOriginal, setShowOriginal] = useState(false);
  const textRef = useRef<HTMLParagraphElement>(null);
  // DanyWhere: the part being corrected, and the text typed so far.
  const [editing, setEditing] = useState<CorrectionPart | null>(null);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);

  // Queued, or being transcribed or post-processed in the background.
  const retrying = job !== undefined;
  useEffect(() => {
    if (!retrying) setCancelling(false);
  }, [retrying]);
  const hasTranscription = entry.transcription_text.trim().length > 0;
  const hasPostProcessed = postProcessedText(entry) !== null;
  const text = shownText(entry, showOriginal);
  const part = shownPart(entry, showOriginal);
  const corrected = isCorrected(entry, part);
  const folded = hasTranscription && !retrying && !expanded;

  // Offer "Show more" only when the folded text is actually cut off.
  useLayoutEffect(() => {
    const el = textRef.current;
    if (!el || !folded) return;
    const measure = () => setOverflows(el.scrollHeight > el.clientHeight + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
    // DanyWhere: `editing` remounts the paragraph after a correction.
  }, [folded, text, editing]);

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

  const startEditing = () => {
    setDraft(text);
    setEditing(part);
  };

  const saveCorrection = async () => {
    if (editing === null || draft.trim().length === 0) return;
    setSaving(true);
    try {
      const result = await correct(entry.id, editing, draft);
      if (result.status !== "ok") throw new Error(String(result.error));
      setEditing(null);
    } catch (error) {
      console.error("Failed to save the correction:", error);
      notify({ title: t("settings.history.correctError") });
    } finally {
      setSaving(false);
    }
  };

  const revertCorrection = async () => {
    try {
      const result = await correct(entry.id, part, null);
      if (result.status !== "ok") throw new Error(String(result.error));
    } catch (error) {
      console.error("Failed to undo the correction:", error);
      notify({ title: t("settings.history.correctError") });
    }
  };

  const handleDeletePostProcessing = async () => {
    try {
      const result = await deletePostProcessing(entry.id);
      if (result.status !== "ok") throw new Error(String(result.error));
      setShowOriginal(false);
    } catch (error) {
      console.error("Failed to delete the post-processing:", error);
      notify({ title: t("settings.history.deletePostProcessingError") });
    }
  };

  // Post-processing or transcribing again replaces corrections: ask first.
  const runReprocess = async (mode: ReprocessMode, message: string) => {
    if (correctionsLostBy(entry, mode)) {
      let replace = false;
      try {
        replace = await ask(message, {
          title: t("settings.history.confirmReplaceTitle"),
          kind: "warning",
          okLabel: t("settings.history.confirmReplace"),
          cancelLabel: t("settings.history.cancelCorrection"),
        });
      } catch (error) {
        console.error("Failed to ask before replacing a correction:", error);
      }
      if (!replace) return;
    }
    void reprocess(entry.id, mode);
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
              <span className="truncate">{entry.source_name}</span>
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
            onClick={startEditing}
            disabled={!hasTranscription || retrying || editing !== null}
            title={t("settings.history.edit")}
          >
            <Pencil width={16} height={16} />
          </IconButton>
          <IconButton
            onClick={onToggleSaved}
            active={entry.saved}
            title={
              entry.saved
                ? t("settings.history.unsave")
                : t("settings.history.save")
            }
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
              {hasTranscription && (
                <IconButton
                  onClick={() =>
                    void runReprocess(
                      "post_process",
                      t(
                        "settings.history.confirmPostProcessReplacesCorrection",
                      ),
                    )
                  }
                  unavailable={postProcess !== "ready"}
                  title={
                    postProcess === "ready"
                      ? t("settings.history.postProcess")
                      : postProcess === "off"
                        ? t("settings.history.postProcessOff")
                        : t("settings.history.postProcessNotSetUp")
                  }
                >
                  <WandSparkles width={16} height={16} />
                </IconButton>
              )}
              {!entry.recording_removed && (
                <IconButton
                  onClick={(event) =>
                    void runReprocess(
                      retranscribeMode(event.shiftKey, postProcess),
                      t(
                        "settings.history.confirmRetranscribeReplacesCorrection",
                      ),
                    )
                  }
                  title={
                    postProcess === "ready"
                      ? t("settings.history.retranscribeShift")
                      : t("settings.history.retranscribe")
                  }
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
      <EntryWindowLine entry={entry} onShowWindow={onShowWindow} />

      <div className="flex flex-col items-start gap-1 pb-2">
        {editing !== null && (
          <div className="w-full flex flex-col gap-2">
            <textarea
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.preventDefault();
                  setEditing(null);
                } else if (isSaveShortcut(event)) {
                  event.preventDefault();
                  void saveCorrection();
                }
              }}
              autoFocus
              rows={Math.min(12, Math.max(3, draft.split("\n").length + 1))}
              aria-label={t("settings.history.edit")}
              className="w-full italic text-sm text-text bg-background border border-line rounded-md p-2 resize-y focus:outline-none focus:border-logo-primary"
            />
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                onClick={() => void saveCorrection()}
                disabled={saving || draft.trim().length === 0}
              >
                {t("settings.history.saveCorrection")}
              </Button>
              <Button
                size="sm"
                variant="secondary"
                onClick={() => setEditing(null)}
              >
                {t("settings.history.cancelCorrection")}
              </Button>
            </div>
          </div>
        )}
        {editing === null && (
          <p
            ref={textRef}
            className={`italic text-sm w-full ${
              retrying
                ? ""
                : hasTranscription
                  ? "text-text select-text cursor-text whitespace-pre-wrap break-words"
                  : "text-ink-3"
            }`}
            style={
              retrying
                ? TRANSCRIBE_PULSE
                : folded
                  ? {
                      display: "-webkit-box",
                      WebkitLineClamp: FOLDED_LINES,
                      WebkitBoxOrient: "vertical",
                      overflow: "hidden",
                    }
                  : undefined
            }
          >
            {retrying && <TranscribePulseKeyframes />}
            {retrying
              ? progressText
              : hasTranscription
                ? text
                : entry.source_name
                  ? // Cancelled, interrupted by a quit, or failed.
                    t("settings.history.notTranscribed")
                  : t("settings.history.transcriptionFailed")}
          </p>
        )}
        {hasPostProcessed &&
          entry.post_processing_outdated &&
          part === "post_processing" &&
          !retrying &&
          editing === null && (
            <p className="text-xs text-ink-3">
              {t("settings.history.postProcessingOutdated")}
            </p>
          )}
        {hasTranscription &&
          !retrying &&
          editing === null &&
          (expanded || overflows || hasPostProcessed || corrected) && (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
              {(expanded || overflows) && (
                <button
                  type="button"
                  onClick={() => setExpanded(!expanded)}
                  className="text-xs font-medium text-ink-3 hover:text-logo-primary cursor-pointer"
                >
                  {expanded
                    ? t("settings.history.showLess")
                    : t("settings.history.showMore")}
                </button>
              )}
              {hasPostProcessed && (
                <button
                  type="button"
                  onClick={() => setShowOriginal(!showOriginal)}
                  className="text-xs font-medium text-ink-3 hover:text-logo-primary cursor-pointer"
                >
                  {showOriginal
                    ? t("settings.history.showPostProcessed")
                    : t("settings.history.showOriginal")}
                </button>
              )}
              {hasPostProcessed && (
                <button
                  type="button"
                  onClick={() => void handleDeletePostProcessing()}
                  className="text-xs font-medium text-ink-3 hover:text-logo-primary cursor-pointer"
                >
                  {t("settings.history.deletePostProcessing")}
                </button>
              )}
              {corrected && (
                <span className="flex items-center gap-2">
                  <span className="flex items-center gap-1 text-xs text-ink-3">
                    <Pencil width={12} height={12} />
                    {t("settings.history.corrected")}
                  </span>
                  <button
                    type="button"
                    onClick={() => void revertCorrection()}
                    className="text-xs font-medium text-ink-3 hover:text-logo-primary cursor-pointer"
                  >
                    {t("settings.history.revertCorrection")}
                  </button>
                </span>
              )}
            </div>
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
