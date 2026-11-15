import { useEffect, useState } from "react";
import { getName } from "@tauri-apps/api/app";
import { buildVariant, type BuildVariant } from "../lib/variant";

// DanyWhere: the product name doesn't change while the app runs; ask once.
let pending: Promise<BuildVariant | null> | null = null;

/** The mark of this build (Dev, NoAX), or `null` for the release. */
export function useBuildVariant(): BuildVariant | null {
  const [variant, setVariant] = useState<BuildVariant | null>(null);

  useEffect(() => {
    let alive = true;
    pending ??= getName()
      .then(buildVariant)
      .catch((error) => {
        console.error("Failed to get the app's name:", error);
        return null;
      });
    pending.then((found) => {
      if (alive) setVariant(found);
    });
    return () => {
      alive = false;
    };
  }, []);

  return variant;
}
