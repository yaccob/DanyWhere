import React, { useLayoutEffect, useRef, useState } from "react";
import { labelPill } from "@/lib/overlayProfile";

/**
 * The compact pill with the dot and one label: where the dictation's text
 * went, or which word list is on. Its width comes from the label's measured
 * width (`labelPill`), like the pills that show a profile.
 */
export const LabelPill: React.FC<{ label: string }> = ({ label }) => {
  const probeRef = useRef<HTMLSpanElement>(null);
  const [labelWidth, setLabelWidth] = useState<number>();
  useLayoutEffect(() => {
    const probe = probeRef.current;
    if (probe) setLabelWidth(Math.ceil(probe.getBoundingClientRect().width));
  }, [label]);
  return (
    <>
      <span ref={probeRef} className="sprobe" aria-hidden="true">
        {label}
      </span>
      <div
        className="scard compact"
        style={
          labelWidth === undefined
            ? undefined
            : { width: labelPill(labelWidth) }
        }
      >
        <div className="sbase">
          <div className="sbase-l">
            <span className="sdot ready" />
          </div>
          <span className="swork-label">{label}</span>
          <div className="sbase-r" />
        </div>
      </div>
    </>
  );
};
