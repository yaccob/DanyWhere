// The manual's screenshots (`bun run manual:screenshots`): Playwright with a
// config of its own, apart from the app's `playwright.config.ts`. It serves
// the frontend with `vite.config.ts` here (backend mocked) and runs
// `screenshots.spec.ts`, which writes the pictures to
// `docs/manual/public/screenshots/`.
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";

const ROOT = fileURLToPath(new URL("../../..", import.meta.url));
const PORT = 14372;

export default defineConfig({
  testDir: ".",
  testMatch: "screenshots.spec.ts",
  // One browser, one picture after the other: the pictures don't depend on
  // each other, but each shot is quick and the server is shared.
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: "list",
  timeout: 120_000,
  outputDir: fileURLToPath(
    new URL(
      "../../../node_modules/.manual-screenshots-results",
      import.meta.url,
    ),
  ),
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    browserName: "chromium",
    ...devices["Desktop Chrome"],
    // Sharp previews (MANUAL_SCREENSHOTS_PREVIEW); the SVGs don't depend on it.
    deviceScaleFactor: 2,
  },
  webServer: {
    command: "bunx vite --config docs/manual/screenshots/vite.config.ts",
    cwd: ROOT,
    url: `http://127.0.0.1:${PORT}/docs/manual/screenshots/app.html`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
