// DanyWhere: the Tauri side of the component tests, under the plugins that
// components import directly (`@tauri-apps/plugin-os`, `…/api/event`, the
// opener, the dialog, …). The plugins stay real; what they ask the backend
// goes to this catch-all IPC handler (Tauri's own `mockIPC`) instead, which
// records the call and answers with what a test set with `mockIpc`, else
// with a default (nothing). `listen`/`emit` of `@tauri-apps/api/event` work
// in memory. `plugin-os` reads the system from a global, set here: macOS
// unless a test calls `setOsType`.
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";

export type TestOsType = "macos" | "windows" | "linux";

export interface IpcCall {
  cmd: string;
  args: unknown;
}

const overrides = new Map<string, (args: unknown) => unknown>();
const recorded: IpcCall[] = [];

/** What a plugin asks when a test sets nothing, as far as nothing would be
 * wrong. */
const DEFAULT_ANSWERS: Partial<Record<string, (args: unknown) => unknown>> = {
  "plugin:os|locale": () => "en-US",
  "plugin:app|version": () => "0.0.0-test",
  "plugin:app|name": () => "DanyWhere",
};

/** Every IPC call of a plugin since the last reset, in order. */
export const ipcCalls = (): readonly IpcCall[] => recorded;

/** Answers the plugin command `cmd` (e.g. `plugin:dialog|open`) with
 * `impl` until the next reset. */
export const mockIpc = (cmd: string, impl: (args: unknown) => unknown) => {
  overrides.set(cmd, impl);
};

let osType: TestOsType = "macos";

const osInternals = () => ({
  os_type: osType,
  platform: osType,
  family: osType === "windows" ? "windows" : "unix",
  version: "0.0.0-test",
  arch: "aarch64",
  eol: osType === "windows" ? "\r\n" : "\n",
  exe_extension: osType === "windows" ? "exe" : "",
});

/** The system `@tauri-apps/plugin-os` reports, until the next reset. */
export const setOsType = (next: TestOsType): void => {
  osType = next;
  (window as unknown as Record<string, unknown>).__TAURI_OS_PLUGIN_INTERNALS__ =
    osInternals();
};

/** Sends a Tauri event (`@tauri-apps/api/event`) to the listeners. */
export const emitTauriEvent = (event: string, payload?: unknown) =>
  emit(event, payload);

export const installTauriMocks = (): void => {
  mockIPC(
    (cmd, args) => {
      recorded.push({ cmd, args });
      const impl = overrides.get(cmd) ?? DEFAULT_ANSWERS[cmd];
      return impl ? impl(args) : null;
    },
    { shouldMockEvents: true },
  );
  mockWindows("main");
  setOsType(osType);
};

/** Back to macOS, no answers, no calls, no listeners. */
export const resetTauriMocks = (): void => {
  clearMocks();
  overrides.clear();
  recorded.length = 0;
  osType = "macos";
  installTauriMocks();
};
