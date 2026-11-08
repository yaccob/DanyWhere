// DanyWhere: the Post Process page loads the provider's model list by itself.
// The list lives only in memory (settingsStore's postProcessModelOptions), so
// after every app start the model dropdown held just the selected model until
// the user clicked the refresh button.

const APPLE_PROVIDER_ID = "apple_intelligence";

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
