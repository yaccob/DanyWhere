import React from "react";
import { useTranslation } from "react-i18next";
import { useModelStore } from "../../stores/modelStore";
import { getTranslatedModelName } from "../../lib/utils/modelTranslation";
import { useChosenModel } from "./models/modelEntries";
import { wordListModelLine } from "./wordListModelLine";

/** DanyWhere: under the word lists' heading, whether the speech model of the
 * page's scope (the defaults' or a profile's) uses the word lists while it
 * recognizes speech, or leaves them to post-processing as known terms. */
export const WordListModelHint: React.FC = () => {
  const { t } = useTranslation();
  const models = useModelStore((s) => s.models);
  const chosen = useChosenModel();
  const info = models.find((m) => m.id === chosen);
  const line = wordListModelLine(
    info && {
      name: getTranslatedModelName(info, t),
      supports_vocabulary: info.supports_vocabulary,
    },
  );
  if (!line) return null;
  return (
    <p className="text-xs text-ink-3 mt-1">
      {t(line.key, { model: line.model })}
    </p>
  );
};
