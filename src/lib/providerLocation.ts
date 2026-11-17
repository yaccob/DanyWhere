/**
 * DanyWhere: where a post-processing provider runs, as the settings show it
 * next to the provider. "local" is this computer: Apple Intelligence, or an
 * address at `localhost` or a loopback address (`127.0.0.0/8`, `::1`).
 * Everything else, a server in the local network included, is "cloud": the
 * dictation leaves the computer. The backend decides the same way whether to
 * warm a model up (`src-tauri/src/model_warmup.rs`).
 */
export type ProviderLocation = "local" | "cloud";

export const APPLE_INTELLIGENCE_PROVIDER_ID = "apple_intelligence";

export function isLocalBaseUrl(baseUrl: string): boolean {
  let url: URL;
  try {
    url = new URL(baseUrl.trim());
  } catch {
    return false;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return false;
  // URL normalizes the host: lower case, IPv4 in dotted form, IPv6 in
  // brackets and shortened (`[::1]`).
  const host = url.hostname.replace(/\.$/, "");
  return (
    host === "localhost" ||
    host === "[::1]" ||
    /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(host)
  );
}

export function providerLocation(provider: {
  id: string;
  base_url: string;
}): ProviderLocation {
  if (provider.id === APPLE_INTELLIGENCE_PROVIDER_ID) return "local";
  return isLocalBaseUrl(provider.base_url) ? "local" : "cloud";
}
