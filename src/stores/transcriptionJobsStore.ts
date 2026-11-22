import { create } from "zustand";
import { events, type TranscriptionJobStatus } from "@/bindings";
import i18n from "@/i18n";
import { notify } from "@/stores/noticeStore";

export type ActiveJobStatus = Extract<
  TranscriptionJobStatus,
  { state: "queued" } | { state: "running" } | { state: "post_processing" }
>;

// Background transcriptions (files, and History entries transcribed or
// post-processed again), keyed by History entry id. Only unfinished jobs are
// kept.
interface TranscriptionJobsStore {
  jobs: Record<number, ActiveJobStatus>;
  initialized: boolean;
  initialize: () => Promise<void>;
}

export const useTranscriptionJobsStore = create<TranscriptionJobsStore>()(
  (set, get) => ({
    jobs: {},
    initialized: false,

    initialize: async () => {
      if (get().initialized) return;
      set({ initialized: true });

      await events.transcriptionJob.listen(({ payload }) => {
        const { entry_id: entryId, status } = payload;
        set((state) => {
          const jobs = { ...state.jobs };
          if (
            status.state === "queued" ||
            status.state === "running" ||
            status.state === "post_processing"
          ) {
            jobs[entryId] = status;
          } else {
            delete jobs[entryId];
          }
          return { jobs };
        });
        if (status.state === "failed") {
          console.error(`Transcription of entry ${entryId} failed:`, status);
        }
        // DanyWhere: ✨ came back with the top version's text, so the entry
        // has no new version; without this nothing visible would happen.
        if (status.state === "unchanged") {
          notify({
            title: i18n.t("settings.history.versions.unchanged"),
            level: "warning",
          });
        }
      });
    },
  }),
);

const hasJobs = (state: TranscriptionJobsStore) =>
  Object.keys(state.jobs).length > 0;

// While a background transcription runs, the model can't be switched,
// unloaded or deleted, so one transcript is never written by two models.
export const useModelsLocked = () => useTranscriptionJobsStore(hasJobs);

// The same, read once outside React.
export const areModelsLocked = () =>
  hasJobs(useTranscriptionJobsStore.getState());
