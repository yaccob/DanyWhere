// DanyWhere: puts the user's theme file (theme.css next to the settings,
// src-tauri/src/theme_file.rs) into a webview as its last stylesheet, so it
// overrides the built-in palette and any other rule without a rebuild.
// loadUserTheme.ts wires this to the backend; this file is the DOM logic.

/** The id of the `<style>` element that holds the theme file's CSS. */
export const USER_THEME_ID = "danywhere-user-theme";

/**
 * Puts the theme file's CSS into the document as its last stylesheet,
 * replacing what an earlier call put there; `null` (no file, or one that
 * can't be read) leaves only the built-in styles.
 */
export function applyUserTheme(doc: Document, css: string | null): void {
  const existing = doc.getElementById(USER_THEME_ID);
  if (css === null) {
    existing?.remove();
    return;
  }
  const sheet = existing ?? doc.createElement("style");
  sheet.id = USER_THEME_ID;
  if (sheet.textContent !== css) sheet.textContent = css;
  if (doc.head.lastElementChild !== sheet) doc.head.appendChild(sheet);
}

/**
 * Moves the theme file's sheet back to the end of `<head>` when a sheet was
 * added after it (Vite adds a lazy chunk's CSS there), so a built-in rule
 * never wins over the user's by coming later. Returns whether it moved it.
 */
export function keepUserThemeLast(doc: Document): boolean {
  const sheet = doc.getElementById(USER_THEME_ID);
  if (!sheet || doc.head.lastElementChild === sheet) return false;
  doc.head.appendChild(sheet);
  return true;
}
