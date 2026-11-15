// DanyWhere: the Post Process page loads the provider's model list by itself.
// The list lives only in memory (settingsStore's postProcessModelOptions), so
// after every app start the model dropdown held just the selected model until
// the user clicked the refresh button.

const APPLE_PROVIDER_ID = "apple_intelligence";

/**
 * Providers whose model list loads without a key: OpenRouter answers
 * GET /models without one (OpenAI and Groq answer 401). Keep in line with
 * `model_list_needs_api_key` in `shortcut/mod.rs`.
 */
const PUBLIC_MODEL_LIST_PROVIDERS = new Set<string>(["openrouter"]);

/** The model lists loaded in this app session, per provider. */
export type ModelOptions = Record<string, string[]>;

/** Whether a provider's list was loaded in this app session (an empty list counts). */
export const modelsLoadedFor = (
  options: ModelOptions,
  providerId: string,
): boolean => options[providerId] !== undefined;

/**
 * The lists after a provider's key or the provider changed: its list is gone,
 * so the dropdown doesn't show models from before, and it counts as not
 * loaded, so the page loads it again by itself. Handy set it to an empty
 * list, which counts as loaded, so a new key needed the refresh button.
 */
export const withoutModelOptions = (
  options: ModelOptions,
  providerId: string,
): ModelOptions => {
  const rest = { ...options };
  delete rest[providerId];
  return rest;
};

/**
 * Whether a provider can be asked for its models: the custom provider needs a
 * base URL (a local server may need no key), every other provider a key.
 * Apple Intelligence has a fixed model.
 */
export const isProviderReadyForModelList = (
  providerId: string,
  baseUrl: string,
  apiKey: string,
): boolean => {
  if (providerId === APPLE_PROVIDER_ID) return false;
  if (PUBLIC_MODEL_LIST_PROVIDERS.has(providerId)) return true;
  return providerId === "custom" ? baseUrl.trim() !== "" : apiKey.trim() !== "";
};

export interface ModelAutoFetchState {
  providerId: string;
  baseUrl: string;
  apiKey: string;
  /** The list was loaded in this app session (an empty list counts). */
  modelsLoaded: boolean;
  isFetching: boolean;
  /** This provider was already asked with this key and base URL. */
  alreadyTried: boolean;
}

/**
 * Load the list when the page shows a provider that is set up and whose list
 * has not been loaded or tried yet. A failed attempt is not repeated for the
 * same settings (the refresh button still is), so a wrong key does not cause
 * a request after every render.
 */
export const shouldAutoFetchModels = (state: ModelAutoFetchState): boolean =>
  isProviderReadyForModelList(state.providerId, state.baseUrl, state.apiKey) &&
  !state.modelsLoaded &&
  !state.isFetching &&
  !state.alreadyTried;

/** Identifies one attempt: the provider with its current base URL and key. */
export const modelFetchAttemptKey = (
  providerId: string,
  baseUrl: string,
  apiKey: string,
): string => JSON.stringify([providerId, baseUrl.trim(), apiKey.trim()]);

/**
 * Once an attempt's list has arrived it is no longer "tried": after a
 * provider switch clears the list, coming back loads it again. Only a failed
 * attempt (its list never arrives) stays tried, so it isn't repeated.
 */
export const forgetLoadedAttempt = (
  tried: Set<string>,
  attempt: string,
  modelsLoaded: boolean,
): void => {
  if (modelsLoaded) tried.delete(attempt);
};
