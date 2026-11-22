// DanyWhere: a profile wrapper that a jump from the page "Profile" goes to
// scrolls into view and lights up briefly (qa 2026-11-22_1603, B5). The
// rules are in `lib/settingJump.ts`.
import { useEffect, useRef, useState, type RefObject } from "react";
import {
  HIGHLIGHT_MS,
  isJumpTarget,
  scrollBehavior,
  scrollBlock,
} from "@/lib/settingJump";
import { prefersReducedMotion } from "@/lib/utils/motion";
import { useSettingJump } from "./settingJumpStore";

/** The height of the area that scrolls `element`: its nearest scrolling
 * ancestor (the settings window's content), else the window. */
const viewportHeight = (element: HTMLElement): number => {
  for (let node = element.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node);
    if (overflowY === "auto" || overflowY === "scroll") {
      return node.clientHeight;
    }
  }
  return window.innerHeight;
};

/**
 * Whether the wrapper `ref` (of `fields`, or their entry `entry`, in profile
 * `profileId`) is lit after a jump to it. While a jump to it waits and the
 * wrapper shows, it takes the jump in the next frame, when the page is laid
 * out and the window has scrolled the new page to its top; then it scrolls
 * itself into view and stays lit for `HIGHLIGHT_MS`.
 */
export const useJumpHighlight = (
  ref: RefObject<HTMLElement | null>,
  profileId: string,
  fields: readonly string[],
  entry: string | undefined,
  shows: boolean,
): boolean => {
  const jump = useSettingJump((s) => s.jump);
  const [litAt, setLitAt] = useState<number | null>(null);
  const mine =
    jump !== null && shows && isJumpTarget(jump, profileId, fields, entry);
  // The fields as of the last render, for the frame that takes the jump.
  const wrapper = useRef({ fields, entry });
  wrapper.current = { fields, entry };

  useEffect(() => {
    if (!mine) return;
    const frame = requestAnimationFrame(() => {
      const element = ref.current;
      // Hidden after all (it draws nothing on this device): not the target.
      if (!element || element.getClientRects().length === 0) return;
      const { fields: current, entry: currentEntry } = wrapper.current;
      if (!useSettingJump.getState().claim(profileId, current, currentEntry)) {
        return;
      }
      element.scrollIntoView({
        block: scrollBlock(element.offsetHeight, viewportHeight(element)),
        behavior: scrollBehavior(prefersReducedMotion()),
      });
      setLitAt(Date.now());
    });
    return () => cancelAnimationFrame(frame);
  }, [mine, jump, profileId, ref]);

  useEffect(() => {
    if (litAt === null) return;
    const timer = window.setTimeout(() => setLitAt(null), HIGHLIGHT_MS);
    return () => window.clearTimeout(timer);
  }, [litAt]);

  return litAt !== null;
};
