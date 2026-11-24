// The manual's screenshots: starts the real frontend in a plain browser.
// `app.html?lang=de&theme=dark` mocks the backend as a fresh macOS
// installation in that language and app theme, then loads `src/main.tsx`
// as the app's window would.
import { answerAsFreshMac, type Theme } from "./example-data";
import { installTauriMocks } from "./mocks/tauri";

const params = new URLSearchParams(window.location.search);
const language = params.get("lang") ?? "en";
const theme: Theme = params.get("theme") === "dark" ? "dark" : "light";

installTauriMocks(language);
answerAsFreshMac(language, theme);
// The theme main.tsx applies before the settings arrive (`getStoredTheme`).
try {
  localStorage.setItem("handy.theme", theme);
} catch {
  // Without storage the settings' theme follows a moment later.
}

// A still picture: no transitions, animations or blinking caret.
//
// And the app's glyphs as macOS's WebKit (the app's webview) draws them:
// Tailwind's font list names "Apple Color Emoji" before the system's own
// fallback, so Chromium draws a character the system font lacks, such as
// the key chips' "↩", as a colour emoji (a bitmap, even in the PDF), where
// WebKit takes a text font's outline. Limited to the emoji planes, the
// emoji font no longer takes such characters, and Chromium falls back as
// WebKit does.
const still = document.createElement("style");
still.textContent = `
*, *::before, *::after {
  transition: none !important;
  animation: none !important;
  caret-color: transparent !important;
}
@font-face {
  font-family: "Apple Color Emoji";
  src: local("Apple Color Emoji"), local("AppleColorEmoji");
  unicode-range: U+1F000-1FFFF;
}
`;
document.head.append(still);

await import("../../../src/main.tsx");
