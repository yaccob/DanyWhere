import { BRAND_NAME } from "@/lib/constants/branding";

// Placeholder wordmark: the product name set in the app font. A designed
// wordmark replaces this component later; callers only pass a width (the
// height follows from the viewBox) and an optional className.
const DanyWhereWordmark = ({
  width,
  height,
  className,
}: {
  width?: number;
  height?: number;
  className?: string;
}) => (
  <svg
    width={width}
    height={height}
    className={["text-text", className].filter(Boolean).join(" ")}
    viewBox="0 0 220 48"
    role="img"
    aria-label={BRAND_NAME}
    xmlns="http://www.w3.org/2000/svg"
  >
    {/* textLength pins the rendered width, whatever font the system picks. */}
    <text
      x="110"
      y="36"
      textAnchor="middle"
      textLength="212"
      lengthAdjust="spacing"
      fontSize="38"
      fontWeight="600"
      fill="currentColor"
    >
      {BRAND_NAME}
    </text>
  </svg>
);

export default DanyWhereWordmark;
