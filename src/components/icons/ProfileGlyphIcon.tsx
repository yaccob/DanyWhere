import React from "react";

/**
 * DanyWhere: the overlay's glyph for choosing the profile by application: a
 * window while it is on (the application in front chooses), the tray's
 * padlock while it is off. Colored by `currentColor`.
 */
export const ProfileGlyphIcon: React.FC<{
  glyph: "window" | "lock";
  className?: string;
}> = ({ glyph, className }) =>
  glyph === "window" ? (
    <svg className={className} viewBox="0 0 14 14" aria-hidden="true">
      <g fill="none" stroke="currentColor" strokeWidth="1.4">
        <rect x="0.7" y="1.7" width="12.6" height="10.6" rx="2" />
        <line x1="0.7" y1="5" x2="13.3" y2="5" />
      </g>
      <circle cx="7" cy="8.6" r="2" fill="currentColor" />
    </svg>
  ) : (
    <svg className={className} viewBox="0 0 14 14" aria-hidden="true">
      <path
        d="M4.5 6.5V4.5a2.5 2.5 0 0 1 5 0v2"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
      />
      <rect x="2" y="6.5" width="10" height="7" rx="1.5" fill="currentColor" />
    </svg>
  );
