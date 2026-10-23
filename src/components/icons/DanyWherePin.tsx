import { useId } from "react";

// DanyWhere glyph: a D-shaped location pin around a microphone with grille
// slots. Same geometry as the menu bar icon (generated in the sidecar by
// tools/branding/make_assets.py); drawn in currentColor like the lucide icons
// next to it in the sidebar.
const DanyWherePin = ({
  width,
  height,
  className,
}: {
  width?: number | string;
  height?: number | string;
  className?: string;
}) => {
  const maskId = useId();
  return (
    <svg
      width={width || 24}
      height={height || 24}
      viewBox="147 134 760 760"
      className={className}
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <mask id={maskId} maskUnits="userSpaceOnUse">
        <rect x="147" y="134" width="760" height="760" fill="white" />
        {[328, 366, 404].map((y) => (
          <rect
            key={y}
            x="490"
            y={y}
            width="68"
            height="22"
            rx="11"
            fill="black"
          />
        ))}
      </mask>
      <g
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path
          d="M292 190 H512 A250 250 0 0 1 762 440 C762 620 572 760 512 840 C452 760 292 640 292 440 Z"
          strokeWidth="84"
        />
        <path d="M406 434 A118 118 0 0 0 642 434" strokeWidth="52" />
        <path d="M524 552 V630 M456 630 H592" strokeWidth="52" />
      </g>
      <rect
        x="462"
        y="300"
        width="124"
        height="196"
        rx="62"
        fill="currentColor"
        mask={`url(#${maskId})`}
      />
    </svg>
  );
};

export default DanyWherePin;
