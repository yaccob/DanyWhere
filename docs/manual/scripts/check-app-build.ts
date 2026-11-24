// Checks the app build of the manual (public/manual/) the way the app serves
// it, in headless Chromium:
// - a static server that resolves paths like Tauri 2's asset protocol
//   (tauri 2.11 src/manager/mod.rs get_asset: the path as is, then
//   <path>.html, then <path>/index.html, else the app's index.html; and
//   tauri-utils 2.9 src/mime_type.rs for the content type);
// - every page loads without console errors, and every link, script,
//   stylesheet and image on it resolves to a file of the build;
// - no figure is missing its file, and each theme shows its own variant;
// - a click on a figure opens it full size, Escape closes it;
// - Pagefind's search finds the test page by a word only it contains.
//
// Run after `bun run build:manual`:
//   bun docs/manual/scripts/check-app-build.ts [screenshot folder]
// With a folder it also saves PNG screenshots of the test page there.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { chromium, type Page } from "@playwright/test";

const repoDir = join(import.meta.dir, "..", "..", "..");
const buildDir = join(repoDir, "public", "manual");
const BASE = "/manual";
const TEST_PAGE = `${BASE}/dev/components/`;
const SEARCH_WORD = "zymurgy";
const screenshotDir = process.argv[2];

if (!existsSync(join(buildDir, "dev", "components", "index.html"))) {
  console.error(`no app build in ${buildDir}; run \`bun run build:manual\``);
  process.exit(2);
}

// ---------------------------------------------------------------- server --

/** Tauri's lookup, limited to the manual: undefined where Tauri would fall
 *  back to the app's own index.html. */
function resolveLikeTauri(pathname: string): string | undefined {
  let path = pathname.endsWith("/") ? pathname.slice(0, -1) : pathname;
  path = decodeURIComponent(path).replace(/^\//, "");
  if (!path.startsWith("manual")) return undefined;
  const rel = path.slice("manual".length).replace(/^\//, "");
  const candidates =
    rel === ""
      ? ["", ".html", "/index.html"]
      : [rel, `${rel}.html`, `${rel}/index.html`];
  for (const candidate of candidates) {
    const file = join(buildDir, candidate.replace(/^\//, ""));
    if (existsSync(file) && statSync(file).isFile()) return file;
  }
  return undefined;
}

/** tauri-utils MimeType::parse: sniff the content first (except for SVG),
 *  else the extension, else HTML for an unknown extension. */
function mimeLikeTauri(file: string, bytes: Uint8Array): string {
  if (!file.endsWith(".svg")) {
    if (bytes[0] === 0x1f && bytes[1] === 0x8b) return "application/gzip";
    if (
      bytes[0] === 0x00 &&
      bytes[1] === 0x61 &&
      bytes[2] === 0x73 &&
      bytes[3] === 0x6d
    )
      return "application/wasm";
    if (bytes[0] === 0x89 && bytes[1] === 0x50) return "image/png";
    if (String.fromCharCode(...bytes.slice(0, 4)) === "wOF2")
      return "font/woff2";
  }
  const ext = file.includes(".") ? file.split(".").pop()! : undefined;
  const byExt: Record<string, string> = {
    bin: "application/octet-stream",
    css: "text/css",
    csv: "text/csv",
    html: "text/html",
    ico: "image/vnd.microsoft.icon",
    js: "text/javascript",
    mjs: "text/javascript",
    json: "application/json",
    jsonld: "application/ld+json",
    mp4: "video/mp4",
    rtf: "application/rtf",
    svg: "image/svg+xml",
    txt: "text/plain",
  };
  if (ext === undefined) return "application/octet-stream";
  return byExt[ext] ?? "text/html";
}

const fallbacks: string[] = [];
const server = Bun.serve({
  port: 0,
  hostname: "127.0.0.1",
  fetch(request) {
    const { pathname } = new URL(request.url);
    const file = resolveLikeTauri(pathname);
    if (!file) {
      fallbacks.push(pathname);
      return new Response("Tauri would serve the app's index.html here", {
        status: 404,
        headers: { "x-tauri-fallback": "1" },
      });
    }
    const bytes = readFileSync(file);
    return new Response(bytes, {
      headers: { "content-type": mimeLikeTauri(file, bytes) },
    });
  },
});
const origin = `http://127.0.0.1:${server.port}`;

// ---------------------------------------------------------------- checks --

let failures = 0;
const fail = (message: string) => {
  failures++;
  console.log(`  FAIL ${message}`);
};
const pass = (message: string) => console.log(`  ok   ${message}`);

function htmlPages(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return htmlPages(path);
    return entry.name === "index.html" ? [path] : [];
  });
}

const pages = htmlPages(buildDir)
  .map((file) =>
    `${BASE}/${relative(buildDir, file).split(sep).slice(0, -1).join("/")}/`.replace(
      /\/\/$/,
      "/",
    ),
  )
  .sort();

const browser = await chromium.launch();

async function openPage(colorScheme: "light" | "dark"): Promise<Page> {
  const context = await browser.newContext({
    colorScheme,
    viewport: { width: 1100, height: 800 },
    deviceScaleFactor: 2,
  });
  return context.newPage();
}

console.log(`Pages (${pages.length}) and the URLs they use`);
{
  const page = await openPage("light");
  const errors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  const checked = new Set<string>();
  for (const path of pages) {
    errors.length = 0;
    const response = await page.goto(origin + path, {
      waitUntil: "networkidle",
    });
    if (!response || response.status() !== 200) {
      fail(`${path} answers ${response?.status()}`);
      continue;
    }
    const urls: string[] = await page.$$eval(
      "a[href], link[href], script[src], img[src]",
      (elements) =>
        elements.map(
          (e) => (e as HTMLAnchorElement).href || (e as HTMLImageElement).src,
        ),
    );
    const local = [...new Set(urls)]
      .map((u) => new URL(u))
      .filter((u) => u.origin === origin)
      .map((u) => u.pathname);
    const broken = local.filter((p) => !resolveLikeTauri(p));
    local.forEach((p) => checked.add(p));
    const missing = await page.locator("[data-dw-missing]").count();
    if (broken.length) fail(`${path}: unresolved ${broken.join(", ")}`);
    else if (errors.length)
      fail(`${path}: console errors: ${errors.join(" | ")}`);
    else if (missing) fail(`${path}: ${missing} figure(s) without a file`);
    else pass(`${path} (${local.length} local URLs)`);
  }
  console.log(`  ${checked.size} distinct local URLs checked`);
  await page.context().close();
}

console.log("Figures follow the theme; a click opens one full size");
for (const scheme of ["light", "dark"] as const) {
  const page = await openPage(scheme);
  await page.goto(origin + TEST_PAGE, { waitUntil: "networkidle" });
  const theme = await page.evaluate(
    () => document.documentElement.dataset.theme,
  );
  if (theme !== scheme) fail(`${scheme}: data-theme is ${theme}`);
  for (const figure of ["diagrams", "screenshots"]) {
    const visible = page.locator(`figure.dw-figure--${figure} img:visible`);
    const count = await visible.count();
    const src = count === 1 ? await visible.getAttribute("src") : null;
    const loaded =
      count === 1 &&
      (await visible.evaluate(
        (img: HTMLImageElement) => img.complete && img.naturalWidth > 0,
      ));
    if (count === 1 && src?.endsWith(`.${scheme}.svg`) && loaded)
      pass(`${scheme}: ${figure} shows ${src}`);
    else
      fail(
        `${scheme}: ${figure} shows ${count} image(s), ${src}, loaded ${loaded}`,
      );
  }
  if (screenshotDir) {
    await page.screenshot({
      path: join(screenshotDir, `test-page-${scheme}.png`),
      fullPage: true,
    });
  }
  await page.locator("figure.dw-figure--diagrams a:visible").click();
  const dialog = page.locator("dialog.dw-zoom");
  const open = await dialog.evaluate((d: HTMLDialogElement) => d.open);
  const zoomSrc = await dialog.locator("img").getAttribute("src");
  if (open && zoomSrc?.endsWith(`sample.${scheme}.svg`))
    pass(`${scheme}: click opens ${zoomSrc}`);
  else fail(`${scheme}: click: open ${open}, ${zoomSrc}`);
  if (screenshotDir) {
    await page.screenshot({
      path: join(screenshotDir, `test-page-zoom-${scheme}.png`),
    });
  }
  await page.keyboard.press("Escape");
  if (await dialog.evaluate((d: HTMLDialogElement) => d.open))
    fail(`${scheme}: Escape leaves the dialog open`);
  else pass(`${scheme}: Escape closes it`);
  await page.context().close();
}

console.log(`Search for "${SEARCH_WORD}"`);
{
  const page = await openPage("light");
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(origin + TEST_PAGE, { waitUntil: "networkidle" });
  await page.locator("site-search button[data-open-modal]").click();
  await page.locator("site-search dialog input").fill(SEARCH_WORD);
  const result = page
    .locator("site-search dialog a", { hasText: "Components test page" })
    .first();
  try {
    await result.waitFor({ timeout: 10000 });
    const href = await result.getAttribute("href");
    if (
      href &&
      new URL(href, origin).pathname === TEST_PAGE &&
      resolveLikeTauri(new URL(href, origin).pathname)
    ) {
      pass(`found the test page: ${href}`);
    } else fail(`result links to ${href}`);
  } catch {
    fail(`no result for ${SEARCH_WORD}; page errors: ${errors.join(" | ")}`);
  }
  if (screenshotDir) {
    await page.screenshot({
      path: join(screenshotDir, "test-page-search-light.png"),
    });
  }
  await page.context().close();
}

await browser.close();
server.stop(true);

if (fallbacks.length) {
  console.log(
    `Requests Tauri would answer with the app's index.html: ${[...new Set(fallbacks)].join(", ")}`,
  );
}
console.log(failures ? `${failures} check(s) failed` : "All checks passed");
process.exit(failures ? 1 : 0);
