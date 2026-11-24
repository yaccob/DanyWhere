import React from "react";
import ReactDOM from "react-dom/client";
import { platform } from "@tauri-apps/plugin-os";
import App from "./App";
import { installCompatShims } from "./lib/compat";
import {
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "./lib/utils/theme";
import { loadUserTheme } from "./lib/loadUserTheme";

// Initialize i18n
import "./i18n";

import { useModelStore } from "./stores/modelStore";
import { useNoticeStore } from "./stores/noticeStore";
import { commands } from "./bindings";
import { useTranscriptionJobsStore } from "./stores/transcriptionJobsStore";

const startApp = () => {
  installCompatShims();

  // Set platform before render so CSS can scope per-platform (e.g. scrollbar styles)
  document.documentElement.dataset.platform = platform();

  // Apply the last-known theme synchronously before render to avoid a flash of
  // the wrong palette, then reconcile with the persisted setting once it loads.
  applyTheme(getStoredTheme());
  syncThemeFromSettings();
  // DanyWhere: the user's theme file, after the built-in styles.
  loadUserTheme();

  // Initialize model store (loads models and sets up event listeners)
  useModelStore.getState().initialize();
  useTranscriptionJobsStore.getState().initialize();
  // DanyWhere: once notices listen, warn about prompts that still have old
  // parts, so the warning also pops up (src-tauri/src/prompt_frame.rs).
  useNoticeStore
    .getState()
    .initialize()
    .then(() => commands.checkPromptsForOldParts());

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
};

// DanyWhere: the settings window shows the user manual in a frame
// (`ManualPage`). For a file the build lacks, the app's origin answers with
// this page; in a frame the app doesn't start a second time, and the manual's
// page moves on to another page (or says the build has no manual).
if (window.self === window.top) {
  startApp();
}
