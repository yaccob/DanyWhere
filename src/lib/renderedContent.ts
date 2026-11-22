/**
 * DanyWhere: whether a component drew anything. Some settings draw nothing
 * on a device (a microphone's channel with one channel, the typing tool on
 * macOS, the active list without lists); the profile wrapper around them
 * then draws no state line either (qa 2026-11-22_0107, error 3).
 */

/** A DOM node as far as `showsAnything` needs it. */
export interface NodeLike {
  nodeType: number;
  textContent: string | null;
}

const ELEMENT_NODE = 1;
const TEXT_NODE = 3;

/** Whether a profile wrapper shows its lines: one around a setting only
 * while the setting shows anything; one without a setting always, since it
 * stands for a control drawn elsewhere (the speech model's wrapper next to
 * the model table). */
export const wrapperShows = (
  hasSetting: boolean,
  settingShows: boolean,
): boolean => !hasSetting || settingShows;

/** Whether rendered `nodes` show anything: an element, or text that isn't
 * only white space. A component that returns `null` renders no node. */
export const showsAnything = (nodes: ArrayLike<NodeLike>): boolean =>
  Array.from(nodes).some(
    (node) =>
      node.nodeType === ELEMENT_NODE ||
      (node.nodeType === TEXT_NODE && (node.textContent ?? "").trim() !== ""),
  );
