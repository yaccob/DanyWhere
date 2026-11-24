import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSettings } from "@/hooks/useSettings";
import { manualCandidates, manualLink } from "@/lib/manual";
import { applyManualTheme } from "@/lib/utils/theme";

interface ManualPageProps {
  /** The page to show, by its slug (`MANUAL_PAGES`); its home page if
   * missing. */
  page?: string;
}

/** Whether `doc` is a page of the manual, not the app's own index.html,
 * which the app's origin answers for a file the build lacks. */
const isManualPage = (doc: Document): boolean =>
  doc.querySelector('meta[name="generator"][content^="Starlight"]') !== null;

/** What a click on a link in the manual's page `view` does
 * (`manualLink`): a page of the manual opens in the frame, even when the
 * link asks for a window of its own; a website or e-mail address opens in
 * the default browser or mail program; anything else does nothing. */
const followLink = (event: MouseEvent, view: Window): void => {
  // Middle click opens a link, other buttons (the context menu) don't.
  if (event.type === "auxclick" && event.button !== 1) return;
  const target = event.target as Element | null;
  const anchor =
    typeof target?.closest === "function" ? target.closest("a[href]") : null;
  if (!anchor) return;
  let url: string;
  try {
    url = new URL(anchor.getAttribute("href") ?? "", view.document.baseURI)
      .href;
  } catch {
    event.preventDefault();
    return;
  }
  const link = manualLink(url, window.location.href);
  if (link === "manual") {
    const opensIn = anchor.getAttribute("target") ?? "";
    const elsewhere =
      event.type === "auxclick" ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      (opensIn !== "" && opensIn !== "_self");
    if (elsewhere) {
      event.preventDefault();
      view.location.assign(url);
    }
    return;
  }
  event.preventDefault();
  if (link === "external") {
    openUrl(url).catch((error) =>
      console.warn(`Failed to open ${url} from the manual:`, error),
    );
  }
};

/**
 * DanyWhere: the user manual as a page of the settings window, like History
 * and About: a frame across the whole content area with the manual's page
 * in the interface's language (`manualUrl`). The manual is served from the
 * app's own origin, so the page can reach into the frame: it decides each
 * click on a link there (`followLink`) and brings the page on screen into a
 * new Application Theme. A page the build lacks gives way to its language's
 * home page, then the English one (`manualCandidates`); a build without the
 * manual says so.
 */
export const ManualPage: React.FC<ManualPageProps> = ({ page }) => {
  const { t, i18n } = useTranslation();
  const { settings } = useSettings();
  const theme = settings?.theme ?? "system";
  const frame = useRef<HTMLIFrameElement>(null);

  const candidates = manualCandidates(i18n.language, page);
  const asked = candidates.join(" ");
  // Which of the candidates the frame shows; a new page or language starts
  // over with the first.
  const [tried, setTried] = useState({ asked, index: 0 });
  const index = tried.asked === asked ? tried.index : 0;

  useEffect(() => {
    const doc = frame.current?.contentDocument;
    if (doc && isManualPage(doc)) applyManualTheme(doc, theme);
  }, [theme]);

  const onLoad = () => {
    const view = frame.current?.contentWindow;
    const doc = frame.current?.contentDocument;
    // Another origin's page can't be reached; links never lead to one.
    if (!view || !doc || view.location.protocol === "about:") return;
    if (!isManualPage(doc)) {
      setTried({ asked, index: index + 1 });
      return;
    }
    const follow = (event: MouseEvent) => followLink(event, view);
    doc.addEventListener("click", follow, true);
    doc.addEventListener("auxclick", follow, true);
  };

  if (index >= candidates.length) {
    return (
      <p className="max-w-3xl w-full mx-auto p-4 text-sm text-ink-3">
        {t("manual.unavailable")}
      </p>
    );
  }
  // The content area's whole width and the height left in it (below a
  // warning, if any); the column it sits in has no margin (`edgeToEdge`).
  return (
    <iframe
      ref={frame}
      src={candidates[index]}
      title={t("sidebar.manual")}
      onLoad={onLoad}
      className="block min-h-0 w-full flex-1 border-0"
    />
  );
};
