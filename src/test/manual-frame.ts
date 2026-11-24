// DanyWhere: the user manual's pages for the frame of the manual page
// (`ManualPage`). happy-dom loads no frame by default; the preload
// (`src/test/setup.ts`) lets it load pages under /manual/ through its fetch
// interceptor, answered from here. A path no test serves is a plain page of
// the manual; `withoutManual` makes the build lack it, as the Nix package
// does. A frame never reaches the network: whatever else it asks for is
// not found. A link can't open a window of its own or move the test's own
// window: happy-dom only sets the address then, and `resetManualPages`
// (after every test) puts it back, with nothing served any more.

/** The part of happy-dom's browser settings this needs. */
interface FrameSettings {
  enableIframePageLoading: boolean;
  navigation: { disableChildPageNavigation: boolean };
  fetch: {
    interceptor: {
      beforeAsyncRequest?: (options: {
        request: Request;
        window: unknown;
      }) => Promise<Response | void>;
    } | null;
  };
}

interface HappyDom {
  settings: FrameSettings;
  setURL: (url: string) => void;
}

const happyDom = (): HappyDom =>
  (window as unknown as { happyDOM: HappyDom }).happyDOM;

/** The test window's address, as `src/test/dom.ts` registers it. */
const TEST_URL = "http://localhost/";

/** A page of the manual as Starlight builds it, as far as the app reads
 * it: the generator tags, the theme menu, and `body`. */
export const manualPage = (body = "<h1>Manual</h1>"): string =>
  `<!doctype html><html lang="en" data-theme="dark"><head>` +
  `<meta name="generator" content="Astro v7.4.13">` +
  `<meta name="generator" content="Starlight v0.43.1">` +
  `<title>DanyWhere Manual</title></head><body>` +
  `<starlight-theme-select><select>` +
  `<option value="dark">Dark</option><option value="light">Light</option>` +
  `<option value="auto">Auto</option></select></starlight-theme-select>` +
  `${body}</body></html>`;

/** What the app's origin answers for a file the build lacks: the app's own
 * index.html (Tauri, and Vite's dev server). */
export const APP_INDEX =
  '<!doctype html><html lang="en"><head><title>DanyWhere</title></head>' +
  '<body><div id="root"></div></body></html>';

const served = new Map<string, string>();
const requested: string[] = [];
let missing = false;

/** Serves `pages` (by path, e.g. `/manual/de/`) from now on. */
export const serveManualPages = (pages: Record<string, string>): void => {
  for (const [path, html] of Object.entries(pages)) served.set(path, html);
};

/** From now on the build has no manual: every path no test serves gets the
 * app's own index.html. */
export const withoutManual = (): void => {
  missing = true;
};

/** The paths under /manual/ a frame loaded, in order. */
export const manualRequests = (): readonly string[] => requested;

const html = (body: string, status = 200) =>
  new Response(body, {
    status,
    headers: { "content-type": "text/html; charset=utf-8" },
  });

export const installManualPages = (): void => {
  const { settings } = happyDom();
  settings.enableIframePageLoading = true;
  settings.navigation.disableChildPageNavigation = true;
  settings.fetch.interceptor = {
    beforeAsyncRequest: async ({ request, window: requester }) => {
      const { pathname } = new URL(request.url);
      const manual = pathname === "/manual" || pathname.startsWith("/manual/");
      // The app's own requests go on as before.
      if (!manual && requester === window) return;
      if (!manual) return html("", 404);
      requested.push(pathname);
      return html(served.get(pathname) ?? (missing ? APP_INDEX : manualPage()));
    },
  };
};

export const resetManualPages = (): void => {
  served.clear();
  requested.length = 0;
  missing = false;
  if (window.location.href !== TEST_URL) happyDom().setURL(TEST_URL);
};
