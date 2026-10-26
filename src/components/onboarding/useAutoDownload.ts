import { useEffect, useRef, useState } from "react";
import { useModelStore } from "../../stores/modelStore";
import { autoDownloadCandidate } from "./autoDownload";

/**
 * DanyWhere: on a fresh install, starts downloading the default model as soon
 * as `active` is true (the permission step), at most once. Returns the id of
 * the model it started, for the model step to follow.
 */
export const useAutoDownload = (active: boolean): string | null => {
  const { models, downloadingModels, downloadModel } = useModelStore();
  const [started, setStarted] = useState<string | null>(null);
  const attempted = useRef(false);

  useEffect(() => {
    if (!active || attempted.current) return;
    // Models load asynchronously; until they do there is no candidate.
    const candidate = autoDownloadCandidate(models, downloadingModels);
    if (!candidate) return;
    attempted.current = true;
    setStarted(candidate);
    void downloadModel(candidate);
  }, [active, models, downloadingModels, downloadModel]);

  return started;
};
