// DanyWhere: marks the local test builds (Dev, NoAX) so that they can't be
// taken for the release (user, 2026-11-15: "momentan kann ich nicht recht
// unterscheiden, welche Version gerade aktiv ist"). The build gives each test
// build its own product name ("DanyWhere Dev", "DanyWhere NoAX"); the
// release's product name is "DanyWhere", so the release shows no mark.

/** The release's product name (`productName` in `tauri.conf.json`). */
export const RELEASE_PRODUCT_NAME = "DanyWhere";

/** A test build's mark, shown in the sidebar and the footer. */
export interface BuildVariant {
  /** What the product name adds to the release's, e.g. "Dev". */
  label: string;
  /** The mark's colour; the same hue as the build's tray dot. */
  background: string;
  /** Text colour that reads on `background`. */
  foreground: string;
}

type Colours = Pick<BuildVariant, "background" | "foreground">;

// Dev violet, NoAX orange: the tray dots the sidecar's make_assets.py draws
// (NoAX's #F28C28; Dev's dot is a lighter violet, #8B5CF6, that reads on a
// dark menu bar, the mark a darker one that white text reads on).
const COLOURS: Record<string, Colours> = {
  dev: { background: "#7C3AED", foreground: "#FFFFFF" },
  noax: { background: "#F28C28", foreground: "#1A1A1A" },
};
const OTHER: Colours = { background: "#6B7280", foreground: "#FFFFFF" };

/** The mark of a build, or `null` for the release. */
export function buildVariant(productName: string): BuildVariant | null {
  const prefix = `${RELEASE_PRODUCT_NAME} `;
  if (!productName.startsWith(prefix)) return null;
  const label = productName.slice(prefix.length).trim();
  if (!label) return null;
  return { label, ...(COLOURS[label.toLowerCase()] ?? OTHER) };
}
