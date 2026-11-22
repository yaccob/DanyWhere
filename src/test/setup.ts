// DanyWhere: the preload of the component tests (`bunfig.toml`, `bun test`).
// It gives every test file a DOM, replaces `src/bindings.ts` and
// `src/i18n/index.ts` before any component loads, answers the Tauri plugins,
// and after every test unmounts what was rendered and resets the stand-ins,
// the stores and the language. How to write a test: AGENTS.md, "Component
// tests".
import "./dom";
import { afterEach, mock } from "bun:test";
import * as bindings from "./bindings-mock";
import * as testI18n from "./i18n";

mock.module("@/bindings", () => ({
  commands: bindings.commands,
  events: bindings.events,
}));
mock.module("@/i18n", () => testI18n);
// Modules that use Vite's `import.meta.glob`, which bun doesn't have, get a
// stand-in here (`src/i18n/index.ts` above). The release notes: none.
mock.module("@/components/whats-new/releaseNotes", () => ({
  findReleaseNoteToShow: () => null,
  findLatestReleaseNote: () => null,
}));

// Loaded only now, so they see the stand-ins above.
const { cleanup } = await import("@testing-library/react");
const { installTauriMocks, resetTauriMocks } = await import("./tauri-mock");
const { resetStores } = await import("./stores");

installTauriMocks();

afterEach(async () => {
  cleanup();
  bindings.resetBindingsMock();
  resetTauriMocks();
  resetStores();
  await testI18n.resetTestLanguage();
  document.body.innerHTML = "";
});
