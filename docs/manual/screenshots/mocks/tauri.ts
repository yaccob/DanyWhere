// The manual's screenshots: the Tauri side in a plain browser, as the
// component tests' `src/test/tauri-mock.ts` sets it up. The plugins the app
// imports stay real; what they ask the backend goes to Tauri's `mockIPC`,
// which answers as a Mac with every permission granted and nothing set up
// would, else with nothing. `@tauri-apps/plugin-os` reads the system from a
// global: macOS on Apple silicon.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

/** What a plugin asks, answered where "nothing" would be wrong. */
const ANSWERS: Record<string, (args: unknown) => unknown> = {
  "plugin:os|locale": () => "en-US",
  "plugin:app|version": () => "0.0.0",
  "plugin:app|name": () => "DanyWhere",
  "plugin:macos-permissions|check_accessibility_permission": () => true,
  "plugin:macos-permissions|check_microphone_permission": () => true,
  "plugin:macos-permissions|check_input_monitoring_permission": () => true,
  "plugin:autostart|is_enabled": () => false,
};

export const installTauriMocks = (locale: string): void => {
  const answers: Record<string, (args: unknown) => unknown> = {
    ...ANSWERS,
    "plugin:os|locale": () => locale,
  };
  mockIPC((cmd, args) => answers[cmd]?.(args) ?? null, {
    shouldMockEvents: true,
  });
  mockWindows("main");
  (window as unknown as Record<string, unknown>).__TAURI_OS_PLUGIN_INTERNALS__ =
    {
      os_type: "macos",
      platform: "macos",
      family: "unix",
      version: "26.0.0",
      arch: "aarch64",
      eol: "\n",
      exe_extension: "",
    };
};
