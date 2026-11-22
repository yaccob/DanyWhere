// DanyWhere: whether an element shows anything, kept up to date while its
// children come and go (a microphone with several channels is chosen).
// Measured before the browser paints, so a wrapper around a setting that
// draws nothing never flashes its own lines. Give the returned ref to the
// element that holds the setting.
import { useLayoutEffect, useState } from "react";
import { showsAnything } from "@/lib/renderedContent";

export const useShowsAnything = (): [
  (element: HTMLElement | null) => void,
  boolean,
] => {
  const [element, setElement] = useState<HTMLElement | null>(null);
  const [shows, setShows] = useState(true);
  useLayoutEffect(() => {
    if (!element) return;
    const update = () => setShows(showsAnything(element.childNodes));
    update();
    const observer = new MutationObserver(update);
    observer.observe(element, { childList: true });
    return () => observer.disconnect();
  }, [element]);
  return [setElement, shows];
};
