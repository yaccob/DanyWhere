// DanyWhere: History asks before a step that deletes text the result
// doesn't keep: versions above the one being edited or gone back to, the
// user's edits under a redone post-processing, or the versions above a
// transcription that is replaced.
import { ask } from "@tauri-apps/plugin-dialog";

export type LossQuestion = {
  title: string;
  message: string;
  okLabel: string;
  cancelLabel: string;
};

/** Whether the user goes ahead; `false` when they cancel or the dialog
 * can't open, so nothing is deleted unasked. */
export const confirmLoss = async ({
  title,
  message,
  okLabel,
  cancelLabel,
}: LossQuestion): Promise<boolean> => {
  try {
    return await ask(message, {
      title,
      kind: "warning",
      okLabel,
      cancelLabel,
    });
  } catch (error) {
    console.error("Failed to ask before deleting versions:", error);
    return false;
  }
};
