export type PermissionPlatform = "macos" | "windows" | "other";

export interface PermissionGrants {
  accessibility: boolean;
  microphone: boolean;
}

/**
 * Whether the app can proceed past the permission screen. On macOS the user
 * may run without Accessibility (shortcuts via the Tauri backend, transcript
 * left on the clipboard), but never without the microphone.
 */
export const permissionsSatisfied = (
  platform: PermissionPlatform,
  grants: PermissionGrants,
  accessibilitySkipped: boolean,
): boolean => {
  if (platform === "macos") {
    return (grants.accessibility || accessibilitySkipped) && grants.microphone;
  }
  if (platform === "windows") {
    return grants.microphone;
  }
  return true;
};
