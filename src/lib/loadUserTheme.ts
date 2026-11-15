import { listen } from "@tauri-apps/api/event";
import { commands } from "@/bindings";
import { applyUserTheme, keepUserThemeLast } from "./userTheme";

/** What theme_file.rs sends after the file was saved or deleted. */
const USER_THEME_CHANGED = "user-theme-changed";

/**
 * DanyWhere: loads the user's theme file into this webview and keeps it
 * current. The backend checks the file every two seconds and sends it when
 * it changed; a window that gets the focus asks for it right away, so a
 * change made in an editor shows as soon as the user switches back. A file
 * that is missing or can't be read leaves the built-in styles alone.
 */
export function loadUserTheme(): void {
  const reload = () =>
    commands
      .getUserTheme()
      .then((css) => applyUserTheme(document, css))
      .catch((error) => console.warn("Failed to load the theme file:", error));

  reload();
  listen<string | null>(USER_THEME_CHANGED, (event) =>
    applyUserTheme(document, event.payload),
  ).catch((error) =>
    console.warn("Failed to follow the theme file's changes:", error),
  );
  window.addEventListener("focus", reload);
  new MutationObserver(() => keepUserThemeLast(document)).observe(
    document.head,
    { childList: true },
  );
}
