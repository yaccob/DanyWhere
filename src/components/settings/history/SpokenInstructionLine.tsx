// DanyWhere: the instruction spoken for a dictation's post-processing,
// under the entry's text in History, with its audio while it is kept
// (sidecar qa/2026-11-26_1720_nachbearbeitung-gesprochene-anweisung.md, F6).
// It belongs to the entry, not to its text: the copy button, the versions
// and the context of later dictations leave it out.
import React, { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { AudioPlayer } from "../../ui/AudioPlayer";
import { Highlighted } from "./HistorySearchBar";
import { matchRanges, type TextMatcher } from "./historySearch";

export interface SpokenInstructionLineProps {
  /** The instruction as heard; `null`/absent: the dictation had none. */
  instruction: string | null | undefined;
  /** The file of its audio in the recordings folder, if kept. */
  audioFile: string | null | undefined;
  /** The storage limit removed the entry's recordings. */
  recordingRemoved: boolean;
  /** The search's text, marked where it matches. */
  highlight: TextMatcher | null;
  /** A playable URL of a recording file (History's `getAudioUrl`). */
  getAudioUrl: (fileName: string) => Promise<string | null>;
}

export const SpokenInstructionLine: React.FC<SpokenInstructionLineProps> = ({
  instruction,
  audioFile,
  recordingRemoved,
  highlight,
  getAudioUrl,
}) => {
  const { t } = useTranslation();
  const text = instruction?.trim() ?? "";
  const ranges = useMemo(() => matchRanges(text, highlight), [text, highlight]);
  const playable = audioFile && !recordingRemoved ? audioFile : null;
  const loadAudio = useCallback(
    () => (playable ? getAudioUrl(playable) : Promise.resolve(null)),
    [getAudioUrl, playable],
  );

  if (instruction === null || instruction === undefined) return null;

  return (
    <div className="w-full flex flex-col gap-1">
      <p className="text-sm">
        <span className="font-medium text-ink-2">
          {t("settings.history.spokenInstruction.label")}
        </span>{" "}
        {text ? (
          <span className="italic">
            <Highlighted text={text} ranges={ranges} />
          </span>
        ) : (
          <span className="italic text-ink-3">
            {t("settings.history.spokenInstruction.nothingHeard")}
          </span>
        )}
      </p>
      {playable && (
        <div
          role="group"
          aria-label={t("settings.history.spokenInstruction.audio")}
        >
          <AudioPlayer onLoadRequest={loadAudio} className="w-full" />
        </div>
      )}
    </div>
  );
};
