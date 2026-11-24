import React from "react";
import { LabelPill } from "./LabelPill";

/** The overlay states whose pill says where the dictation's text went. */
export type ReadyNoticeState = "clipboard" | "ready" | "not_ready";

type Translate = (key: string, options?: { keys: string }) => string;

interface ReadyNoticeProps {
  state: ReadyNoticeState;
  /** The platform's paste shortcut, ⌘V or Ctrl+V. */
  pasteKeys: string;
  /** The "Insert Last Dictation" service's shortcut, e.g. ⌃⇧V; null: none. */
  insertKeys: string | null;
  t: Translate;
}

// Clipboard notice: the text was put on the clipboard instead of pasted.
// Ready notice: the text waits for the "Insert Last Dictation" service.
// Not ready: that service was called while a dictation was still running.
export const ReadyNotice: React.FC<ReadyNoticeProps> = ({
  state,
  pasteKeys,
  insertKeys,
  t,
}) => {
  const label =
    state === "clipboard"
      ? t("overlay.clipboard", { keys: pasteKeys })
      : state === "not_ready"
        ? t("overlay.notReady")
        : insertKeys
          ? t("overlay.ready", { keys: insertKeys })
          : t("overlay.readyNoShortcut");
  return <LabelPill label={label} />;
};
