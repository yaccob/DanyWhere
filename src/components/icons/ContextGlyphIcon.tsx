import React from "react";
import { MessageSquare, MessageSquareOff } from "lucide-react";
import type { ContextGlyph } from "@/lib/overlayProfile";

/**
 * DanyWhere: the overlay's speech bubble for "Send Context"
 * (`send_context.rs`), crossed out while it is off. Drawn 14 × 14 by CSS,
 * with a stroke as heavy as the profile glyphs'; colored by `currentColor`.
 */
export const ContextGlyphIcon: React.FC<{
  glyph: ContextGlyph;
  className?: string;
}> = ({ glyph, className }) => {
  const Icon = glyph === "context" ? MessageSquare : MessageSquareOff;
  return <Icon className={className} strokeWidth={2.4} aria-hidden="true" />;
};
