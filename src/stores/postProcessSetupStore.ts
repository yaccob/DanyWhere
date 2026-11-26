// DanyWhere: the post-processing setup's state, shared by the page "Post
// Process", onboarding's screen and the notices' next steps (sidecar qa
// 2026-11-26_2250, 4.5): what the backend says about the setup, the answers
// being given, a sign-in through the browser while it waits, and the sample.
import { create } from "zustand";
import {
  commands,
  events,
  type FailureDto,
  type PostProcessSignInFinished,
  type SampleResult,
  type SetupAnswers,
  type SetupStatus,
  type SignInIntent,
  type SignInProvider,
} from "@/bindings";
import i18n from "@/i18n";
import {
  sanitize,
  suggest,
  type Machine,
  type SetupAnswers as GivenAnswers,
} from "@/lib/postProcessSetup/questions";
import { useSettingsStore } from "./settingsStore";

const TEXTS = "settings.postProcessing.setup";

/** The sample's sentence in the app's language. */
const sampleText = () => i18n.t(`${TEXTS}.sample.text`);

/** What the browser shows after the sign-in's callback, in the app's
 * language. */
const callbackPage = () => ({
  success: i18n.t(`${TEXTS}.signIn.browserSuccess`),
  failure: i18n.t(`${TEXTS}.signIn.browserFailure`),
});

export interface SignInState {
  provider: SignInProvider;
  /** The way it is for (`signInIntentOf`), kept for "Try again". */
  intent: SignInIntent | null;
  /** Starting: the backend opens the browser; waiting: for its callback;
   * failed: `error` says why. */
  phase: "starting" | "waiting" | "failed";
  error: string | null;
}

export interface SampleState {
  phase: "running" | "done" | "failed";
  result: SampleResult | null;
  failure: FailureDto | null;
}

interface PostProcessSetupState {
  /** The backend's status; null until it answers (or when it can't). */
  status: SetupStatus | null;
  /** The answers being given, as the settings keep them; null until they
   * are read from the settings. */
  answers: SetupAnswers | null;
  /** "Later" put the setup off. */
  skipped: boolean;
  /** The questions show although a way is set up or put off ("Answer the
   * questions again", a notice's "Set up" or "Change setup"). */
  editing: boolean;
  signIn: SignInState | null;
  sample: SampleState | null;
  /** "I've topped up", and OpenRouter still has the free tier. */
  notToppedUp: boolean;
  /** The listener of `post-process-sign-in-finished` is registered. */
  listening: boolean;

  /** Reads the answers and "Later" from the settings, once. */
  hydrate: (kept: { answers?: SetupAnswers | null; skipped?: boolean }) => void;
  /** Asks the backend for the status. */
  loadStatus: () => Promise<void>;
  /** Asks the backend for the status and reloads the settings. */
  refresh: () => Promise<void>;
  listen: () => Promise<void>;
  /** A new answer: drops what no longer applies and keeps the answers. */
  answer: (next: GivenAnswers, machine: Machine, inert?: boolean) => void;
  /** "Later". */
  putOff: (inert?: boolean) => Promise<void>;
  openQuestions: () => void;
  closeQuestions: () => void;
  startSignIn: (
    provider: SignInProvider,
    intent?: SignInIntent | null,
  ) => Promise<void>;
  reopenSignIn: () => Promise<void>;
  cancelSignIn: () => Promise<void>;
  /** A way is set up: keeps it as finished, reloads, tries the sample. */
  finishWay: () => Promise<void>;
  runSample: () => Promise<void>;
  /** "I've topped up": OpenRouter's key info again. */
  checkTopUp: () => Promise<void>;
  /** Forgets the way in use: a sign-in, or the key of a guided way. */
  disconnect: () => Promise<void>;
}

const EMPTY: SetupAnswers = {
  texts_may_go: null,
  subscription: null,
  next_step: null,
};

const save = (
  answers: SetupAnswers | null,
  finished: boolean,
  skipped: boolean,
) => commands.setPostProcessSetupAnswers(answers, finished, skipped);

export const usePostProcessSetupStore = create<PostProcessSetupState>()(
  (set, get) => ({
    status: null,
    answers: null,
    skipped: false,
    editing: false,
    signIn: null,
    sample: null,
    notToppedUp: false,
    listening: false,

    hydrate: (kept) => {
      if (get().answers !== null) return;
      set({
        answers: { ...EMPTY, ...(kept.answers ?? {}) },
        skipped: kept.skipped ?? false,
      });
    },

    loadStatus: async () => {
      const status = await commands.getPostProcessSetupStatus();
      set({ status: status ?? null });
    },

    refresh: async () => {
      const status = await commands.getPostProcessSetupStatus();
      set({ status: status ?? null });
      await useSettingsStore.getState().refreshSettings();
    },

    listen: async () => {
      if (get().listening) return;
      set({ listening: true });
      await events.postProcessSignInFinished.listen(({ payload }) => {
        void signInFinished(payload);
      });
    },

    answer: (next, machine, inert = false) => {
      const answers = sanitize(next, machine);
      set({ answers, skipped: false });
      if (!inert) void save(answers, false, false);
    },

    putOff: async (inert = false) => {
      set({ skipped: true, editing: false });
      if (!inert) await save(get().answers ?? EMPTY, false, true);
    },

    openQuestions: () => set({ editing: true, skipped: false }),
    closeQuestions: () => set({ editing: false }),

    startSignIn: async (provider, intent = null) => {
      set({ signIn: { provider, intent, phase: "starting", error: null } });
      const started = await commands.startPostProcessSignIn(
        provider,
        callbackPage(),
        intent,
      );
      const now = get().signIn;
      // Cancelled or finished meanwhile.
      if (!now || now.provider !== provider || now.phase !== "starting") return;
      set({
        signIn:
          started.status === "ok"
            ? { provider, intent, phase: "waiting", error: null }
            : { provider, intent, phase: "failed", error: started.error },
      });
    },

    reopenSignIn: async () => {
      await commands.reopenPostProcessSignIn();
    },

    cancelSignIn: async () => {
      set({ signIn: null });
      await commands.cancelPostProcessSignIn();
    },

    finishWay: async () => {
      set({ editing: false, skipped: false, notToppedUp: false });
      await save(get().answers ?? EMPTY, true, false);
      await get().refresh();
      await get().runSample();
    },

    runSample: async () => {
      set({ sample: { phase: "running", result: null, failure: null } });
      const outcome = await commands.runPostProcessSample(sampleText());
      set({
        sample:
          outcome.status === "ok"
            ? { phase: "done", result: outcome.data, failure: null }
            : { phase: "failed", result: null, failure: outcome.error },
      });
    },

    checkTopUp: async () => {
      const outcome = await commands.refreshOpenrouterConnection();
      await get().refresh();
      set({
        notToppedUp: outcome.status === "ok" && outcome.data.free_tier,
      });
    },

    disconnect: async () => {
      const status = get().status;
      if (!status) return;
      const way = status.way;
      if (way?.kind === "chatgpt_plan") {
        await commands.disconnectPostProcessSignIn("chatgpt");
      } else if (way?.kind === "openrouter") {
        await commands.disconnectPostProcessSignIn("openrouter");
      } else {
        // A pasted key: an empty key removes it; the rest stays. A server
        // of one's own and Apple Intelligence have nothing to disconnect.
        const provider =
          way?.kind === "guided_key" ? way.provider : status.provider_id;
        if (
          !provider ||
          way?.kind === "own_server" ||
          way?.kind === "on_device"
        )
          return;
        await commands.applyPostProcessChoice({
          provider_id: provider,
          model: null,
          api_key: "",
          effort: null,
        });
      }
      set({ sample: null, notToppedUp: false });
      await get().refresh();
    },
  }),
);

/** How a sign-in ended, from the backend's event. */
const signInFinished = async ({
  provider,
  ok,
  error,
}: PostProcessSignInFinished) => {
  const { signIn, finishWay } = usePostProcessSetupStore.getState();
  if (!signIn || signIn.provider !== provider) return;
  if (ok) {
    usePostProcessSetupStore.setState({ signIn: null });
    await finishWay();
  } else if (error === "cancelled") {
    usePostProcessSetupStore.setState({ signIn: null });
  } else {
    usePostProcessSetupStore.setState({
      signIn: { ...signIn, phase: "failed", error },
    });
  }
};

/** Any machine: the ways through a sign-in don't depend on its memory. */
const ANY_MACHINE: Machine = {
  memoryBytes: Number.MAX_SAFE_INTEGER,
  platform: "macos",
};

/**
 * The way of the setup a sign-in is for, from the answers (sidecar qa
 * 2026-11-26_2250, D, H1 and M1): OpenRouter's free models, or OpenRouter
 * only with credit, which then sends nothing until the account has credit.
 * Null when the answers lead elsewhere, e.g. to ChatGPT, or OpenRouter was
 * chosen under "All providers …".
 */
export const signInIntentOf = (
  answers: GivenAnswers | null | undefined,
): SignInIntent | null => {
  switch (answers ? suggest(answers, ANY_MACHINE)?.kind : undefined) {
    case "openrouter_free":
      return "free";
    case "openrouter_credit":
      return "credit";
    default:
      return null;
  }
};
