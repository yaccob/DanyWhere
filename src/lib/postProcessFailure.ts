// DanyWhere: why a post-processing failed (user, 2026-11-23: the notice
// names the actual reason, so the user knows what to change). The backend
// sends the reason as the notice's detail, a small JSON object
// (src-tauri/src/post_process_failure.rs); this turns it into a sentence
// after the notice's explanation.
import { providerName, type Translate } from "./displayNames";

/** The notice's detail as the backend sends it. */
interface Failure {
  reason: string;
  providerId?: unknown;
  provider?: unknown;
  model?: unknown;
  seconds?: unknown;
  status?: unknown;
  error?: unknown;
}

const parse = (detail?: string | null): Failure | null => {
  if (!detail) return null;
  try {
    const value: unknown = JSON.parse(detail);
    if (
      typeof value === "object" &&
      value !== null &&
      !Array.isArray(value) &&
      typeof (value as { reason?: unknown }).reason === "string"
    ) {
      return value as Failure;
    }
  } catch {
    // A raw error, not a reason.
  }
  return null;
};

const text = (value: unknown): string | null =>
  typeof value === "string" && value.trim() !== "" ? value : null;
const count = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

const KEY = "errors.postProcessFailure";

/** The sentence for `failure`, or null when its reason isn't known here or
 * a value its sentence needs is missing. */
const reasonSentence = (t: Translate, failure: Failure): string | null => {
  const label = text(failure.provider);
  const provider =
    label === null
      ? null
      : providerName({ id: text(failure.providerId) ?? "", label }, t);
  const model = text(failure.model);
  const withProvider = (key: string, options: Record<string, unknown> = {}) =>
    provider === null ? null : t(`${KEY}.${key}`, { provider, ...options });

  switch (failure.reason) {
    case "no-provider":
      return t(`${KEY}.noProvider`);
    case "no-model":
      return withProvider("noModel");
    case "no-prompt":
      return t(`${KEY}.noPrompt`);
    case "no-api-key":
      return withProvider("noApiKey");
    case "key-refused":
      return withProvider("keyRefused");
    case "no-credit":
      return withProvider("noCredit");
    case "model-not-found":
      return model === null ? null : withProvider("modelNotFound", { model });
    case "unreachable":
      return withProvider("unreachable");
    case "timeout": {
      const seconds = count(failure.seconds);
      return seconds === null ? null : withProvider("timeout", { seconds });
    }
    case "status": {
      const status = count(failure.status);
      return status === null ? null : withProvider("status", { status });
    }
    case "empty-reply":
      return withProvider("emptyReply");
    case "truncated":
      return t(`${KEY}.truncated`);
    case "unavailable":
      return withProvider("unavailable");
    // DanyWhere: where a way of the setup ends (qa 2026-11-26_2250, 4.3).
    case "free-busy":
      return withProvider("freeBusy");
    case "free-daily-limit":
      return withProvider("freeDailyLimit");
    case "key-limit":
      return withProvider("keyLimit");
    case "plan-limit":
      return withProvider("planLimit");
    case "plan-not-eligible":
      return withProvider("planNotEligible");
    case "sign-in-expired":
      return withProvider("signInExpired");
    // DanyWhere: ChatGPT without a session, and a setup never finished
    // (qa 2026-11-26_2250, D, M2 and M7).
    case "no-sign-in":
      return withProvider("noSignIn");
    case "not-set-up":
      return t(`${KEY}.notSetUp`);
    case "rate-limited":
      return withProvider("rateLimited");
    case "other": {
      const error = text(failure.error);
      return error === null ? null : t(`${KEY}.other`, { error });
    }
    default:
      return null;
  }
};

/** `base` (the notice's explanation), followed by the failure's reason when
 * `detail` names one this app knows; otherwise `base` alone. */
export const withPostProcessFailureReason = (
  t: Translate,
  base: string,
  detail?: string | null,
): string => {
  const failure = parse(detail);
  const reason = failure === null ? null : reasonSentence(t, failure);
  return reason === null ? base : `${base} ${reason}`;
};

/** The title a failure's notice has instead of its code's, if any: before
 * the setup was ever finished, "Post-Processing Not Set Up" (qa
 * 2026-11-26_2250, D, M7). A translation key. */
export const postProcessFailureTitle = (
  detail?: string | null,
): string | null =>
  parse(detail)?.reason === "not-set-up" ? `${KEY}.notSetUpTitle` : null;

/** Whether `detail` is a failure's reason, which the explanation shows, so
 * it isn't shown again as it is. A reason this app doesn't know isn't shown
 * either: it is a code, not a text. */
export const isPostProcessFailureDetail = (detail?: string | null): boolean =>
  parse(detail) !== null;

/** The sentence for a command's failure (`FailureDto`: a reason's name and
 * the notice's detail, the JSON object with the reason's values) at
 * `provider`, whose name the caller shows elsewhere too. Where the reason
 * has no sentence, its error, or a detail that isn't JSON as it is; never
 * the JSON itself (sidecar qa 2026-11-26_2250, D, M4). */
export const failureSentence = (
  t: Translate,
  failure: { reason: string; detail: string | null },
  provider: { id: string; label: string },
): string => {
  const values = parse(failure.detail);
  const raw = values === null ? failure.detail : null;
  const error = text(values?.error) ?? raw ?? failure.reason;
  return (
    reasonSentence(t, {
      ...values,
      reason: values?.reason ?? failure.reason,
      providerId: provider.id || values?.providerId,
      provider: provider.label || values?.provider,
      error,
    }) ?? t(`${KEY}.other`, { error })
  );
};

/**
 * DanyWhere: the next step a failure's notice offers (qa 2026-11-26_1840,
 * section 5: when a way ends, a targeted notice instead of all the
 * questions; 2026-11-26_2250, 4.5).
 * - `set_up`: the setup's questions (nothing set up yet)
 * - `open`: a page of the provider's account, `label` names the button
 * - `change_setup`: the questions again (the plan doesn't count any more)
 * - `sign_in_again`: a new sign-in with the provider
 */
export type FailureAction =
  | { kind: "set_up" }
  | {
      kind: "open";
      label: "topUp" | "credit" | "changeLimit" | "manageUsage";
      url: string;
    }
  | { kind: "change_setup" }
  | { kind: "sign_in_again"; provider: "openrouter" | "chatgpt" };

/** The pages a failure's next step may open, by provider id. */
export interface FailurePages {
  usage?: string;
  credit?: string;
  keys?: string;
}

/** The next step for the failure in `detail`, or none: the reason has no
 * step, or the provider has no page for it. `pagesOf` gives a provider's
 * pages (`lib/postProcessSetup/guides.ts`). */
export const postProcessFailureAction = (
  detail: string | null | undefined,
  pagesOf: (providerId: string) => FailurePages,
): FailureAction | null => {
  const failure = parse(detail);
  if (failure === null) return null;
  const providerId = text(failure.providerId) ?? "";
  const pages = pagesOf(providerId);
  const open = (
    label: "topUp" | "credit" | "changeLimit" | "manageUsage",
    url: string | undefined,
  ): FailureAction | null => (url ? { kind: "open", label, url } : null);
  switch (failure.reason) {
    case "no-provider":
    case "no-model":
    case "no-api-key":
    case "not-set-up":
      return { kind: "set_up" };
    case "no-sign-in":
      return providerId === "chatgpt"
        ? { kind: "sign_in_again", provider: "chatgpt" }
        : { kind: "set_up" };
    case "no-credit":
      return open(
        providerId === "openrouter" ? "topUp" : "credit",
        pages.credit,
      );
    case "free-daily-limit":
      return open("topUp", pages.credit);
    case "key-limit":
      // Only OpenRouter limits a key's spending; elsewhere the limit is the
      // organization's or the project's, on its usage page.
      return providerId === "openrouter"
        ? open("changeLimit", pages.keys)
        : open("manageUsage", pages.usage);
    case "plan-limit":
      return open("manageUsage", pages.usage);
    case "plan-not-eligible":
      return { kind: "change_setup" };
    case "sign-in-expired":
      return providerId === "openrouter" || providerId === "chatgpt"
        ? { kind: "sign_in_again", provider: providerId }
        : { kind: "set_up" };
    default:
      return null;
  }
};
