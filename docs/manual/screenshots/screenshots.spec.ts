// The manual's screenshots, as vector pictures: each shot renders the real
// settings window (backend mocked, `boot.ts`) per language and app theme in
// headless Chromium, prints it as a PDF at the window's size with screen
// styles, and cuts the shot's part out of it as SVG with poppler's
// `pdftocairo`. Text becomes glyph outlines, so it stays sharp at any zoom
// and needs no font on the reader's side.
//
//   bun run manual:screenshots
//
// writes docs/manual/public/screenshots/<lang>/<name>.<theme>.svg. With
// MANUAL_SCREENSHOTS_PREVIEW=<folder> it also writes, per picture, the SVG
// rasterized (`rsvg-convert`) and a plain browser screenshot of the same
// part, to compare the two.
//
// Needs Playwright's Chromium (`bunx playwright install --only-shell
// chromium`) and poppler (`pdftocairo`); type check with
// `bunx tsc --noEmit -p docs/manual/screenshots/tsconfig.json`.
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";

const ROOT = fileURLToPath(new URL("../../..", import.meta.url));
const OUT = join(ROOT, "docs/manual/public/screenshots");
const PREVIEW = process.env.MANUAL_SCREENSHOTS_PREVIEW;

const LANGUAGES = ["en", "de", "es", "fr"] as const;
const THEMES = ["light", "dark"] as const;

/** The settings window as DanyWhere opens it (`MAIN_WINDOW_SIZE` in
 * src-tauri/src/utils.rs); its height grows so nothing scrolls. */
const WINDOW = { width: 720, height: 600 };
/** Room around a shot's part, in CSS pixels, so its border and shadow
 * don't touch the picture's edge. */
const MARGIN = 12;
/** CSS pixels to PDF points. */
const PT_PER_PX = 72 / 96;

interface Shot {
  /** The file name, without language and theme. */
  name: string;
  /** The settings group shown, by its title's translation key. */
  groupTitleKey: string;
}

/** Defaults › General is where the window opens (`START_LOCATION`). */
const SHOTS: Shot[] = [
  { name: "end-shortcuts", groupTitleKey: "settings.endKeys.title" },
];

/** A translation as the app shows it: the locale's text, the product named
 * DanyWhere (`src/i18n/branding.ts`), English where the locale lacks it. */
const translation = (language: string, key: string): string => {
  const read = (code: string) =>
    JSON.parse(
      readFileSync(
        join(ROOT, `src/i18n/locales/${code}/translation.json`),
        "utf8",
      ),
    ) as Record<string, unknown>;
  const lookup = (tree: Record<string, unknown>) =>
    key
      .split(".")
      .reduce<unknown>(
        (node, part) => (node as Record<string, unknown> | undefined)?.[part],
        tree,
      );
  const text = lookup(read(language)) ?? lookup(read("en"));
  if (typeof text !== "string") throw new Error(`no text for ${key}`);
  return text.replace(/\bHandy(?=s?\b)/g, "DanyWhere");
};

/** Opens the window in `language` and `theme` and waits until it shows the
 * settings in that language, theme and fonts. */
const openWindow = async (
  page: Page,
  language: string,
  theme: string,
): Promise<void> => {
  await page.setViewportSize(WINDOW);
  await page.emulateMedia({ media: "screen", colorScheme: theme as "light" });
  await page.goto(
    `/docs/manual/screenshots/app.html?lang=${language}&theme=${theme}`,
  );
  await page.waitForFunction(
    ([lang, mode]) =>
      document.documentElement.lang === lang &&
      document.documentElement.dataset.theme === mode &&
      document.querySelector('[id^="end-key-name-"]') !== null,
    [language, theme],
  );
  await page.waitForLoadState("networkidle");
  await page.evaluate(() => document.fonts.ready);
  // Let the window grow until its settings no longer scroll, so the PDF
  // (one page, the window's size) holds all of them. The scrollbars are
  // overlays, as on macOS, so the width of the content stays the same.
  for (let round = 0; round < 5; round += 1) {
    const overflow = await page.evaluate(async () => {
      await new Promise((done) =>
        requestAnimationFrame(() => requestAnimationFrame(done)),
      );
      const scroller = [...document.querySelectorAll<HTMLElement>("div")].find(
        (element) =>
          getComputedStyle(element).overflowY === "auto" &&
          element.scrollHeight > element.clientHeight,
      );
      return scroller ? scroller.scrollHeight - scroller.clientHeight : 0;
    });
    if (overflow === 0) return;
    const size = page.viewportSize() ?? WINDOW;
    await page.setViewportSize({
      width: size.width,
      height: size.height + overflow,
    });
  }
  throw new Error("the settings still scroll");
};

/** Leaves only the settings group titled `title` visible, with the window's
 * background, so the PDF holds nothing else; the layout stays as it is.
 * Returns the group's box in CSS pixels. */
const showOnlyGroup = (page: Page, title: string) =>
  page.evaluate((wanted) => {
    const heading = [...document.querySelectorAll("h2")].find(
      (element) => element.textContent?.trim() === wanted,
    );
    // SettingsGroup: the title's block, then the card, in one element.
    const group = heading?.parentElement?.parentElement;
    if (!group) throw new Error(`no settings group "${wanted}"`);
    // Everything beside the group and beside each of its ancestors goes
    // invisible; the group's own elements keep their visibility.
    for (let node: Element = group; node.parentElement; ) {
      const parent: Element = node.parentElement;
      for (const sibling of parent.children) {
        if (sibling !== node && sibling instanceof HTMLElement) {
          sibling.style.setProperty("visibility", "hidden", "important");
        }
      }
      if (parent === document.body) break;
      node = parent;
    }
    const { x, y, width, height } = group.getBoundingClientRect();
    return { x, y, width, height };
  }, title);

/** pdftocairo's SVG, smaller and drawn the same: numbers to three decimals
 * (a thousandth of a point), and each run of neighbouring glyph groups with
 * the same colour as one group (cairo wraps every glyph in its own). */
const tidySvg = (svg: string): string => {
  let tidy = svg.replace(/(\d\.\d{3})\d+/g, "$1");
  const neighbours =
    /(<g (fill="[^"]+" fill-opacity="[^"]+")>\n(?:<use [^>]*\/>\n)+)<\/g>\n<g \2>\n/g;
  for (let before = ""; before !== tidy; ) {
    before = tidy;
    tidy = tidy.replace(neighbours, "$1");
  }
  return tidy;
};

for (const language of LANGUAGES) {
  for (const theme of THEMES) {
    test(`${language} ${theme}`, async ({ page }) => {
      // An error in the app would leave a picture of a half-drawn window.
      const errors: string[] = [];
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) => {
        if (message.type() === "error") errors.push(message.text());
      });
      for (const shot of SHOTS) {
        await openWindow(page, language, theme);
        const viewport = page.viewportSize();
        if (!viewport) throw new Error("no viewport");
        const title = translation(language, shot.groupTitleKey);
        const preview =
          PREVIEW && join(PREVIEW, `${shot.name}.${language}.${theme}`);
        if (preview) {
          // The window as the browser shows it, before anything is hidden.
          mkdirSync(PREVIEW as string, { recursive: true });
          await page.screenshot({ path: `${preview}.window.png` });
        }
        const box = await showOnlyGroup(page, title);
        // The part with its margin, in whole points (pdftocairo's unit).
        const left = Math.floor((box.x - MARGIN) * PT_PER_PX);
        const top = Math.floor((box.y - MARGIN) * PT_PER_PX);
        const right = Math.ceil((box.x + box.width + MARGIN) * PT_PER_PX);
        const bottom = Math.ceil((box.y + box.height + MARGIN) * PT_PER_PX);
        const [width, height] = [right - left, bottom - top];

        const work = mkdtempSync(join(tmpdir(), "manual-screenshots-"));
        try {
          const pdf = join(work, "window.pdf");
          const cut = join(work, "cut.svg");
          await page.pdf({
            path: pdf,
            width: `${viewport.width}px`,
            height: `${viewport.height}px`,
            margin: { top: "0", right: "0", bottom: "0", left: "0" },
            printBackground: true,
            scale: 1,
            pageRanges: "1",
          });
          // `-nocenter` puts the cut at the top left of a page its size.
          execFileSync("pdftocairo", [
            "-svg",
            ...["-f", "1", "-l", "1"],
            ...["-x", String(left), "-y", String(top)],
            ...["-W", String(width), "-H", String(height)],
            ...["-paperw", String(width), "-paperh", String(height)],
            "-nocenter",
            pdf,
            cut,
          ]);
          const svg = tidySvg(readFileSync(cut, "utf8"));
          expect(svg).toContain("<svg");
          // Vector only: a bitmap (a colour emoji, an image in the UI)
          // would blur when zoomed.
          expect(svg).not.toContain("<image");
          const dir = join(OUT, language);
          mkdirSync(dir, { recursive: true });
          const file = join(dir, `${shot.name}.${theme}.svg`);
          writeFileSync(file, svg);

          if (preview) {
            // The same part as the browser shows it, and the SVG drawn by
            // another renderer, both at twice the size.
            await page.screenshot({
              path: `${preview}.screen.png`,
              clip: {
                x: left / PT_PER_PX,
                y: top / PT_PER_PX,
                width: width / PT_PER_PX,
                height: height / PT_PER_PX,
              },
            });
            execFileSync("rsvg-convert", [
              ...["-z", "2", "-o", `${preview}.svg.png`],
              file,
            ]);
          }
        } finally {
          rmSync(work, { recursive: true, force: true });
        }
      }
      expect(errors).toEqual([]);
    });
  }
}
