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

installCompatShims();

// Set platform before render so CSS can scope per-platform (e.g. scrollbar styles)
document.documentElement.dataset.platform = platform();

// Apply the last-known theme synchronously before render to avoid a flash of
// the wrong palette, then reconcile with the persisted setting once it loads.
applyTheme(getStoredTheme());
syncThemeFromSettings();
// DanyWhere: the user's theme file, after the built-in styles.
loadUserTheme();

// Initialize i18n
import "./i18n";

// Initialize model store (loads models and sets up event listeners)
import { useModelStore } from "./stores/modelStore";
import { useNoticeStore } from "./stores/noticeStore";
import { commands } from "./bindings";
import { useTranscriptionJobsStore } from "./stores/transcriptionJobsStore";
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
