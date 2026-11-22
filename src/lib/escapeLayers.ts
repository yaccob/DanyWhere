/**
 * DanyWhere: Escape closes what is open in front, such as a dropdown's list,
 * a menu or a drag. It is heard on the window, in the capture phase: WebKit
 * gives a clicked button no keyboard focus, so a key handler on the button
 * or on the dragged element never hears it (qa 2026-11-22_0107, error 4; qa
 * 2026-11-22_0233, the dropdowns). What is open forms a stack per window and
 * only the newest layer closes. The press goes no further, so a dialog or a
 * menu behind the layer stays open until the next Escape.
 */
import { isComposingKeyEvent } from "./utils/keyboard";

type Target = Pick<EventTarget, "addEventListener" | "removeEventListener">;

interface Layer {
  onCancel: () => void;
}

interface Stack {
  layers: Layer[];
  onKey: (event: Event) => void;
}

const stacks = new WeakMap<Target, Stack>();

const forget = (target: Target, stack: Stack) => {
  if (stack.layers.length > 0) return;
  target.removeEventListener("keydown", stack.onKey, true);
  stacks.delete(target);
};

/** Opens a layer on `target` (the window) that `onCancel` closes on
 * Escape; returns the function that removes it. Only the newest layer
 * hears the press, and it is removed before `onCancel` runs, so the next
 * Escape goes to the layer behind it, or to the page once none is left.
 * An Escape that ends an input method's composition is left to the input. */
export const cancelOnEscape = (
  target: Target,
  onCancel: () => void,
): (() => void) => {
  let stack = stacks.get(target);
  if (!stack) {
    const created: Stack = {
      layers: [],
      onKey: (event: Event) => {
        const key = event as KeyboardEvent;
        if (key.key !== "Escape" || isComposingKeyEvent(key)) return;
        const top = created.layers.pop();
        if (!top) return;
        event.preventDefault();
        // Not the document (a dialog), and not a window listener added
        // after this one; one added before asks `escapeLayerOpen`.
        event.stopPropagation();
        event.stopImmediatePropagation();
        forget(target, created);
        top.onCancel();
      },
    };
    target.addEventListener("keydown", created.onKey, true);
    stacks.set(target, created);
    stack = created;
  }
  const layer: Layer = { onCancel };
  const own = stack;
  own.layers.push(layer);
  return () => {
    const at = own.layers.indexOf(layer);
    if (at < 0) return;
    own.layers.splice(at, 1);
    forget(target, own);
  };
};

/** Whether a layer is open on `target`: a window listener for Escape that
 * was added before the layers' own (and so hears the press first) leaves
 * it to them. */
export const escapeLayerOpen = (target: Target): boolean =>
  (stacks.get(target)?.layers.length ?? 0) > 0;
