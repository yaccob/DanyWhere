import type { TFunction } from "i18next";
import { platform } from "@tauri-apps/plugin-os";
import type { NoticeCode } from "@/bindings";
import {
  isPostProcessFailureDetail,
  withPostProcessFailureReason,
} from "./postProcessFailure";

interface NoticeDisplay {
  title: string;
  // Without one, the Live overlay shows the notice's detail instead.
  explanation?: (t: TFunction, detail?: string | null) => string;
  // The explanation already names the detail, so it isn't shown again.
  // DanyWhere: or, as a function, for the details it says so of.
  detailInExplanation?: boolean | ((detail?: string | null) => boolean);
}

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
    explanation: (t, detail) =>
      withPostProcessFailureReason(t, t("errors.postProcessFailed"), detail),
    detailInExplanation: isPostProcessFailureDetail,
  },
  FileTranscriptionFailed: {
    title: "errors.fileTranscriptionFailedTitle",
  },
  FilePostProcessFailed: {
    title: "errors.filePostProcessFailedTitle",
    explanation: (t, detail) =>
      withPostProcessFailureReason(
        t,
        t("errors.filePostProcessFailed"),
        detail,
      ),
    detailInExplanation: isPostProcessFailureDetail,
  },
  HistoryPostProcessFailed: {
    title: "errors.historyPostProcessFailedTitle",
    explanation: (t, detail) =>
      withPostProcessFailureReason(
        t,
        t("errors.historyPostProcessFailed"),
        detail,
      ),
    detailInExplanation: isPostProcessFailureDetail,
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
