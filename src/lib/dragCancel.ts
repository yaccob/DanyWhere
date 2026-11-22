/**
 * DanyWhere: Escape ends a pointer drag without dropping (History's version
 * chips, the prompt library's handles). It is heard on the window, in the
 * capture phase: WebKit gives a clicked button no keyboard focus, so a key
 * handler on the dragged element never hears it (qa 2026-11-22_0107,
 * error 4). The drag owns Escape while it runs, so nothing else (a dialog,
 * a menu) closes on the same press.
 */

/** Listens for Escape on `target` (the window) while a drag runs, and
 * calls `onCancel`; returns the function that stops listening. */
export const cancelOnEscape = (
  target: Pick<EventTarget, "addEventListener" | "removeEventListener">,
  onCancel: () => void,
): (() => void) => {
  const onKey = (event: Event) => {
    if ((event as KeyboardEvent).key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    onCancel();
  };
  target.addEventListener("keydown", onKey, true);
  return () => target.removeEventListener("keydown", onKey, true);
};
