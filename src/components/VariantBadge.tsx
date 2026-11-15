import React from "react";
import { useBuildVariant } from "../hooks/useBuildVariant";

interface VariantBadgeProps {
  className?: string;
}

/**
 * DanyWhere: names a test build ("DEV", "NOAX") in its colour, so that it
 * can't be taken for the release. The release renders nothing.
 */
const VariantBadge: React.FC<VariantBadgeProps> = ({ className = "" }) => {
  const variant = useBuildVariant();
  if (!variant) return null;

  return (
    <span
      className={`rounded-full px-2 py-0.5 font-bold uppercase tracking-wider leading-none ${className}`}
      style={{
        backgroundColor: variant.background,
        color: variant.foreground,
      }}
    >
      {variant.label}
    </span>
  );
};

export default VariantBadge;
