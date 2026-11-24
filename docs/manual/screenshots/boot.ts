// The manual's screenshots: starts the real frontend in a plain browser.
// `app.html?lang=de&theme=dark` mocks the backend as a fresh macOS
// installation in that language and app theme (`&scene=profiles`: with the
// example profiles), then loads `src/main.tsx` as the app's window would.
//
// The pictures are drawn with open fonts that ship with the repository's
// dependencies instead of the system's: the glyph outlines end up in the
// published SVGs (Apple's licence doesn't allow passing on SF Pro), and the
// pictures look the same on any machine. Inter (OFL) stands in for the
// system font, in full (`inter-ui`, not Fontsource's subsets) since only the
// full font has the key symbols (⌃ ⌥ ⇧ ⌘ ↩); JetBrains Mono (OFL) stands in
// for the monospaced one. `screenshots.spec.ts` checks that no other font
// draws anything.
import "inter-ui/inter.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/600.css";
import "@fontsource/jetbrains-mono/700.css";
import { answerAsFreshMac, type Scene, type Theme } from "./example-data";
import { installTauriMocks } from "./mocks/tauri";

const params = new URLSearchParams(window.location.search);
const language = params.get("lang") ?? "en";
const theme: Theme = params.get("theme") === "dark" ? "dark" : "light";
const scene: Scene = params.get("scene") === "profiles" ? "profiles" : "fresh";

installTauriMocks(language);
answerAsFreshMac(language, theme, scene);
// The theme main.tsx applies before the settings arrive (`getStoredTheme`).
try {
  localStorage.setItem("handy.theme", theme);
} catch {
  // Without storage the settings' theme follows a moment later.
}

// The fonts above in place of the app's font lists (Tailwind's and
// App.css'), and a still picture: no transitions, animations or caret.
const style = document.createElement("style");
style.textContent = `
:root {
  --font-sans: "Inter";
  --font-mono: "JetBrains Mono", "Inter";
  --default-font-family: var(--font-sans);
  --default-mono-font-family: var(--font-mono);
  --font-ui: var(--font-sans);
  --font-display: var(--font-sans);
  --font-cjk-subset: var(--font-sans);
}
*, *::before, *::after {
  transition: none !important;
  animation: none !important;
  caret-color: transparent !important;
}
`;
document.head.append(style);

await import("../../../src/main.tsx");
