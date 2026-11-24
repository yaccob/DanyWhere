// The manual's screenshots: the app's own Vite config, serving the real
// frontend in a plain browser with the backend mocked. `src/bindings.ts` is
// swapped for `mocks/bindings.ts` however a module imports it (`@/bindings`,
// `./bindings`, `../../bindings`), and the page `app.html` installs Tauri's
// IPC mocks before it loads `src/main.tsx`. Nothing of this reaches the app's
// own build.
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, mergeConfig, type Plugin, type UserConfig } from "vite";
import appConfig from "../../../vite.config";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const ROOT = resolve(HERE, "../../..");
const BINDINGS = resolve(ROOT, "src/bindings.ts");
const BINDINGS_MOCK = resolve(HERE, "mocks/bindings.ts");

/** The port the screenshot pipeline serves on (not the app's 1420). */
export const SCREENSHOT_PORT = 14372;

/** Every import that resolves to `src/bindings.ts` gets the mock instead;
 * `src/bindings.ts?raw` (the mock reads the command list from it) stays. */
const mockBindings = (): Plugin => ({
  name: "manual-screenshots:mock-bindings",
  enforce: "pre",
  async resolveId(source, importer, options) {
    if (!importer || importer === BINDINGS_MOCK) return null;
    const resolved = await this.resolve(source, importer, {
      ...options,
      skipSelf: true,
    });
    return resolved?.id === BINDINGS ? BINDINGS_MOCK : null;
  },
});

export default defineConfig(async (env) => {
  const base = (await (typeof appConfig === "function"
    ? appConfig(env)
    : appConfig)) as UserConfig;
  return mergeConfig(base, {
    root: ROOT,
    cacheDir: resolve(ROOT, "node_modules/.vite-manual-screenshots"),
    plugins: [mockBindings()],
    server: {
      port: SCREENSHOT_PORT,
      strictPort: true,
      host: "127.0.0.1",
      hmr: false,
    },
  } satisfies UserConfig);
});
