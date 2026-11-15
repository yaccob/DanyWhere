import { useEffect, useState } from "react";

/** How long a confirmation next to a button stays. */
const SHOWN_MS = 5000;

/**
 * A short confirmation shown next to a button, e.g. "Exported". Toasts are
 * for failures only (`notify`), so a success is shown here instead.
 */
export const useTransientStatus = () => {
  const [status, setStatus] = useState<string | null>(null);
  useEffect(() => {
    if (status === null) return;
    const timer = window.setTimeout(() => setStatus(null), SHOWN_MS);
    return () => window.clearTimeout(timer);
  }, [status]);
  return [status, setStatus] as const;
};
