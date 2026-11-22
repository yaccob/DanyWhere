/**
 * DanyWhere: a pointer drag follows the pointer on the window (History's
 * version chips, the prompt library's handles). Escape, which ends a drag
 * without dropping, is a layer of `escapeLayers` (`cancelOnEscape`).
 */

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
