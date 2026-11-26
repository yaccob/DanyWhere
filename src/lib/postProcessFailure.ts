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

/** Whether `detail` is a failure's reason, which the explanation shows, so
 * it isn't shown again as it is. A reason this app doesn't know isn't shown
 * either: it is a code, not a text. */
export const isPostProcessFailureDetail = (detail?: string | null): boolean =>
  parse(detail) !== null;
