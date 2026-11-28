import type { TFunction } from "i18next";
import { platform } from "@tauri-apps/plugin-os";
import type { NoticeCode } from "@/bindings";
import {
  isPostProcessFailureDetail,
  postProcessFailureAction,
  postProcessFailureTitle,
  withPostProcessFailureReason,
  type FailureAction,
} from "./postProcessFailure";
import { providerPages } from "./postProcessSetup/guides";

interface NoticeDisplay {
  title: string;
  // DanyWhere: a title of its own for some details (see `noticeTitle`).
  titleFor?: (detail?: string | null) => string | null;
  // Without one, the Live overlay shows the notice's detail instead.
  explanation?: (t: TFunction, detail?: string | null) => string;
  // The explanation already names the detail, so it isn't shown again.
  // DanyWhere: or, as a function, for the details it says so of.
  detailInExplanation?: boolean | ((detail?: string | null) => boolean);
  // DanyWhere: the next step the notice offers for its detail, as a button.
  action?: (detail?: string | null) => FailureAction | null;
}

// DanyWhere: a failed post-processing's next step (qa 2026-11-26_2250, 4.5).
const postProcessAction = (detail?: string | null) =>
  postProcessFailureAction(detail, providerPages);

// See "Error notices" in AGENTS.md for how to add a code.
const NOTICE_DISPLAY: Record<NoticeCode, NoticeDisplay> = {
  TranscriptionFailed: {
    title: "errors.transcriptionFailedTitle",
  },
  PasteFailed: {
    title: "errors.pasteFailedTitle",
    explanation: (t) => t("errors.pasteFailed"),
  },
  PasteBlockedElevated: {
    title: "errors.pasteBlockedElevatedTitle",
    explanation: (t) => t("errors.pasteBlockedElevated"),
  },
  NoMicrophone: {
    title: "errors.noInputDeviceTitle",
    explanation: (t) => t("errors.noInputDevice"),
  },
  MicrophoneSilent: {
    title: "errors.microphoneSilentTitle",
    explanation: (t) => t("errors.microphoneSilent"),
  },
  MicPermissionDenied: {
    title: "errors.micPermissionDeniedTitle",
    explanation: (t) =>
      t(`errors.micPermissionDenied.${platform()}`, {
        defaultValue: t("errors.micPermissionDenied.generic"),
      }),
  },
  RecordingFailed: {
    title: "errors.recordingFailedTitle",
  },
  ModelLoadFailed: {
    title: "errors.modelLoadFailedTitle",
    explanation: (t) => t("errors.modelLoadFailed"),
  },
  NoModelSelected: {
    title: "errors.noModelSelectedTitle",
    explanation: (t) => t("errors.noModelSelected"),
  },
  // DanyWhere: the three post-processing failures name their reason, which
  // comes as the detail (src/lib/postProcessFailure.ts); another detail,
  // such as a History job's raw error, shows as before.
  PostProcessFailed: {
    title: "errors.postProcessFailedTitle",
    titleFor: postProcessFailureTitle,
    explanation: (t, detail) =>
      withPostProcessFailureReason(t, t("errors.postProcessFailed"), detail),
    detailInExplanation: isPostProcessFailureDetail,
    action: postProcessAction,
  },
  FileTranscriptionFailed: {
    title: "errors.fileTranscriptionFailedTitle",
  },
  FilePostProcessFailed: {
    title: "errors.filePostProcessFailedTitle",
    titleFor: postProcessFailureTitle,
    explanation: (t, detail) =>
      withPostProcessFailureReason(
        t,
        t("errors.filePostProcessFailed"),
        detail,
      ),
    detailInExplanation: isPostProcessFailureDetail,
    action: postProcessAction,
  },
  HistoryPostProcessFailed: {
    title: "errors.historyPostProcessFailedTitle",
    titleFor: postProcessFailureTitle,
    explanation: (t, detail) =>
      withPostProcessFailureReason(
        t,
        t("errors.historyPostProcessFailed"),
        detail,
      ),
    detailInExplanation: isPostProcessFailureDetail,
    action: postProcessAction,
  },
  // DanyWhere: the detail is the app that missed the dictation shortcut.
  AppMissedShortcut: {
    title: "errors.appMissedShortcutTitle",
    explanation: (t, detail) =>
      t("errors.appMissedShortcut", { app: detail ?? "" }),
    detailInExplanation: true,
  },
  // DanyWhere: the detail names the prompts (src-tauri/src/prompt_frame.rs).
  OutdatedPrompts: {
    title: "errors.outdatedPromptsTitle",
    // DanyWhere: and that such a prompt runs alone while it is active
    // (src-tauri/src/prompt_library.rs).
    explanation: (t, detail) =>
      `${t("errors.outdatedPrompts", { prompts: detail ?? "" })} ${t("errors.outdatedPromptsAlone")}`,
    detailInExplanation: true,
  },
  // DanyWhere: the detail is the post-processing model that refused the
  // profile's effort (src-tauri/src/post_process_effort.rs).
  PostProcessEffortRefused: {
    title: "errors.postProcessEffortRefusedTitle",
    explanation: (t, detail) =>
      t("errors.postProcessEffortRefused", { model: detail ?? "" }),
    detailInExplanation: true,
  },
  // DanyWhere: the detail is the raw error; the text before the instruction
  // waits for Paste Last Transcript (src-tauri/src/dictation_flow/mod.rs).
  InstructionTranscriptionFailed: {
    title: "errors.instructionTranscriptionFailedTitle",
    explanation: (t) => t("errors.instructionTranscriptionFailed"),
  },
};

export const noticeDisplay = (code: NoticeCode) => NOTICE_DISPLAY[code];

/** The translation key of a notice's title: its code's, or one of its own
 * for the detail (DanyWhere: a post-processing that was never set up). The
 * tray keeps the code's title, which Rust has. */
export const noticeTitle = (code: NoticeCode, detail?: string | null) => {
  const display = NOTICE_DISPLAY[code];
  return display.titleFor?.(detail) ?? display.title;
};

/** The detail a notice shows under its explanation: none when the
 * explanation already names it. */
export const separateDetail = (
  code: NoticeCode,
  detail?: string | null,
): string | null => {
  const { detailInExplanation } = NOTICE_DISPLAY[code];
  const named =
    typeof detailInExplanation === "function"
      ? detailInExplanation(detail)
      : detailInExplanation === true;
  return named ? null : (detail ?? null);
};
