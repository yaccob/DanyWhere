/**
 * DanyWhere: Escape ends a pointer drag without dropping (History's version
 * chips, the prompt library's handles). It is heard on the window, in the
 * capture phase: WebKit gives a clicked button no keyboard focus, so a key
 * handler on the dragged element never hears it (qa 2026-11-22_0107,
 * error 4). The drag owns Escape while it runs, so nothing else (a dialog,
 * a menu) closes on the same press. `followPointer` hears the pointer on the
 * window too, for a list whose rows move while one is dragged.
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

/** What a pointer drag does with the pointer's moves and its end. */
export interface PointerDragHandlers {
  onMove: (clientY: number) => void;
  onEnd: () => void;
  onCancel: () => void;
}

/** Follows the pointer on `target` (the window) while a drag runs; returns
 * the function that stops listening. A list that shows the new order while
 * dragging moves DOM nodes, and moving the dragged row's own node drops the
 * handle's pointer capture, so its pointerup never comes (qa
 * 2026-11-22_0233, point 10); the window gets every move and release. */
export const followPointer = (
  target: Pick<EventTarget, "addEventListener" | "removeEventListener">,
  handlers: PointerDragHandlers,
): (() => void) => {
  const onMove = (event: Event) =>
    handlers.onMove((event as PointerEvent).clientY);
  const onUp = () => handlers.onEnd();
  const onCancel = () => handlers.onCancel();
  target.addEventListener("pointermove", onMove);
  target.addEventListener("pointerup", onUp);
  target.addEventListener("pointercancel", onCancel);
  return () => {
    target.removeEventListener("pointermove", onMove);
    target.removeEventListener("pointerup", onUp);
    target.removeEventListener("pointercancel", onCancel);
  };
};
