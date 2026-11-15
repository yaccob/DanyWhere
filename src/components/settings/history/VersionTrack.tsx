// DanyWhere: the track under a History entry with more than one version:
// Transcription → ✨ → ✎ …, one chip per version, bottom to top. A click
// shows that version; below the top it offers "Go Back to This Version".
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronRight,
  Pencil,
  Undo2,
  WandSparkles,
} from "lucide-react";
import type { HistoryEntry, HistoryVersion, VersionStep } from "@/bindings";
import { formatDateTime } from "@/utils/dateFormat";
import { confirmLoss } from "./confirmLoss";
import {
  describeStep,
  topIndex,
  versionMark,
  versionsLostByGoingBack,
} from "./versions";

const KEYS = "settings.history.versions";

interface VersionTrackProps {
  entry: Pick<HistoryEntry, "versions">;
  /** The version the entry shows (`shownVersion`). */
  shown: number;
  /** The versions the search matches (`matchingVersions`). */
  matching: number[];
  /** The user chose a version to see. */
  onShow: (index: number) => void;
  /** The user agreed to delete the versions above `keep`. */
  onGoBack: (keep: number) => void;
  /** While a job runs: the stack can't change, so no going back. */
  disabled?: boolean;
}

/** What a chip shows: the word "Transcription", or ✨ and ✎ as icons. */
const ChipMark: React.FC<{ version: HistoryVersion }> = ({ version }) => {
  const { t } = useTranslation();
  const mark = versionMark(version);
  if (mark === "transcription") return <>{t(`${KEYS}.transcription`)}</>;
  return (
    <span className="flex items-center gap-0.5">
      {(mark === "ai" || mark === "ai_user") && (
        <WandSparkles width={12} height={12} aria-hidden="true" />
      )}
      {(mark === "user" || mark === "ai_user") && (
        <Pencil width={12} height={12} aria-hidden="true" />
      )}
    </span>
  );
};

export const VersionTrack: React.FC<VersionTrackProps> = ({
  entry,
  shown,
  matching,
  onShow,
  onGoBack,
  disabled = false,
}) => {
  const { t, i18n } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const count = entry.versions.length;
  if (count <= 1) return null;

  const top = topIndex(entry);
  const belowTop = shown < top;
  // Showing a version below the top keeps the track open: the user sees
  // which one it is and the way back.
  const open = expanded || belowTop;

  const stepLine = (step: VersionStep) => {
    const { key, prompt, atSeconds } = describeStep(step);
    const name = t(`${KEYS}.step.${key}`, { prompt });
    return atSeconds === null
      ? name
      : t(`${KEYS}.step.withTime`, {
          step: name,
          time: formatDateTime(String(atSeconds), i18n.language),
        });
  };

  const chipTitle = (version: HistoryVersion, index: number) =>
    [
      ...version.steps.map(stepLine),
      ...(matching.includes(index) ? [t(`${KEYS}.searchMatch`)] : []),
      index === shown ? t(`${KEYS}.shown`) : t(`${KEYS}.showVersion`),
    ].join("\n");

  const goBack = async () => {
    const lost = versionsLostByGoingBack(entry, shown);
    if (lost === 0) return;
    const agreed = await confirmLoss({
      title: t(`${KEYS}.confirmGoBackTitle`),
      message: t(`${KEYS}.confirmGoBackMessage`, { count: lost }),
      okLabel: t(`${KEYS}.confirmGoBack`),
      cancelLabel: t(`${KEYS}.cancel`),
    });
    if (agreed) onGoBack(shown);
  };

  const summary = t(`${KEYS}.summary`, { count });

  return (
    <div className="flex flex-col gap-1 w-full text-xs text-ink-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        {belowTop ? (
          <span className="flex items-center gap-1 font-medium">
            <ChevronDown width={12} height={12} aria-hidden="true" />
            {summary}
          </span>
        ) : (
          <button
            type="button"
            onClick={() => setExpanded(!expanded)}
            aria-expanded={open}
            className="flex items-center gap-1 font-medium hover:text-logo-primary cursor-pointer"
          >
            {open ? (
              <ChevronDown width={12} height={12} aria-hidden="true" />
            ) : (
              <ChevronRight
                width={12}
                height={12}
                aria-hidden="true"
                className="rtl:rotate-180"
              />
            )}
            {summary}
          </button>
        )}
        {open && (
          <ol className="flex flex-wrap items-center gap-1">
            {entry.versions.map((version, index) => {
              const isShown = index === shown;
              const matches = matching.includes(index);
              return (
                <li key={index} className="flex items-center gap-1">
                  {index > 0 && (
                    <ChevronRight
                      width={10}
                      height={10}
                      aria-hidden="true"
                      className="shrink-0 rtl:rotate-180"
                    />
                  )}
                  <button
                    type="button"
                    onClick={() => onShow(index)}
                    aria-pressed={isShown}
                    title={chipTitle(version, index)}
                    className={`flex items-center rounded-md border px-1.5 py-0.5 cursor-pointer transition-colors ${
                      isShown
                        ? "border-logo-primary text-logo-primary font-medium"
                        : "border-line hover:text-logo-primary hover:border-logo-primary"
                    } ${matches ? "bg-logo-primary/25" : ""}`}
                  >
                    <ChipMark version={version} />
                  </button>
                </li>
              );
            })}
          </ol>
        )}
        {belowTop && (
          <button
            type="button"
            onClick={() => void goBack()}
            disabled={disabled}
            title={t(`${KEYS}.goBackHint`)}
            className="flex items-center gap-1 font-medium hover:text-logo-primary cursor-pointer disabled:cursor-not-allowed disabled:text-text/20"
          >
            <Undo2 width={12} height={12} aria-hidden="true" />
            {t(`${KEYS}.goBack`)}
          </button>
        )}
      </div>
    </div>
  );
};
