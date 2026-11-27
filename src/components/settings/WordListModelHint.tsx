import React from "react";
import { useTranslation } from "react-i18next";
import { useModelStore } from "../../stores/modelStore";
import { usePostProcessSetupStore } from "../../stores/postProcessSetupStore";
import { useSettings } from "../../hooks/useSettings";
import { getTranslatedModelName } from "../../lib/utils/modelTranslation";
import { providerName } from "../../lib/displayNames";
import { scopePage } from "../../lib/navigation";
import { postProcessingOn } from "../../lib/endKeys";
import { Button } from "../ui/Button";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { pickModel, useChosenModel, usePlace } from "./models/modelEntries";
import {
  useOpenLocation,
  useSettingsScope,
} from "./profiles/scope/SettingsScope";
import { wordListCheckSetup, wordListModelLine } from "./wordListModelLine";

/** The catalog model the word lists offer instead of one without
 * vocabulary: it takes them while it recognizes speech. */
export const VOCABULARY_MODEL_ID = "whisper-large-v3-turbo";

/** The speech model of the page's scope (the defaults' or a profile's),
 * what the word lists do with it, and whether the LLM check can run there. */
const useWordListModel = () => {
  const { t } = useTranslation();
  const models = useModelStore((s) => s.models);
  const chosen = useChosenModel();
  const { settings } = useSettings();
  const setup = wordListCheckSetup(settings);
  const info = models.find((m) => m.id === chosen);
  const line = wordListModelLine(
    info && {
      name: getTranslatedModelName(info, t),
      supports_vocabulary: info.supports_vocabulary,
    },
    setup.kind === "ready",
  );
  return { line, setup };
};

/** DanyWhere: under the word lists' heading, whether the speech model of the
 * page's scope uses the word lists while it recognizes speech, leaves them
 * to post-processing as known terms, or, without a provider for that,
 * leaves them without effect; for a model without vocabulary, the way to
 * one that has it and, without a provider, to setting one up (sidecar
 * qa/2026-11-24_0130_wortlisten-llm-pruefung.md, point 7). */
export const WordListModelHint: React.FC = () => {
  const { t } = useTranslation();
  const { line } = useWordListModel();
  if (!line) return null;
  return (
    <>
      <p className="text-xs text-ink-3 mt-1">
        {t(line.key, { model: line.model })}
      </p>
      {line.state !== "vocabulary" && (
        <WordListModelActions offerSetUp={line.state === "noEffect"} />
      )}
    </>
  );
};

/** "Set Up Post-Processing" (without a provider) and the switch to the
 * model that takes the word lists, when the app offers it. */
const WordListModelActions: React.FC<{ offerSetUp: boolean }> = ({
  offerSetUp,
}) => {
  const { t } = useTranslation();
  const scope = useSettingsScope();
  const openLocation = useOpenLocation();
  const { getSetting, updateSetting } = useSettings();
  const vocabularyModel = useModelStore((s) =>
    s.models.find((m) => m.id === VOCABULARY_MODEL_ID),
  );
  const place = usePlace(VOCABULARY_MODEL_ID);
  const percentage = useModelStore(
    (s) => s.downloadProgress[VOCABULARY_MODEL_ID]?.percentage ?? 0,
  );

  const showSetUp = offerSetUp && scope !== null && openLocation !== null;
  const showSwitch = vocabularyModel?.supports_vocabulary === true;
  if (!showSetUp && !showSwitch) return null;

  // Post Process shows only while the scope may post-process, so setting
  // it up leaves "never" there first, for the mode out of the box.
  const setUp = async () => {
    if (!scope || !openLocation) return;
    if (
      !postProcessingOn({ post_process_mode: getSetting("post_process_mode") })
    ) {
      await updateSetting("post_process_mode", "with_instruction_key");
    }
    // DanyWhere: the defaults' page opens on the setup's questions, also
    // when it was put off with "Later" (qa 2026-11-26_2250, 4.5). A
    // profile's page has no setup, so they aren't left open for the
    // defaults' (D, N15).
    if (scope.kind === "defaults") {
      usePostProcessSetupStore.getState().openQuestions();
    }
    openLocation(scopePage(scope, "postprocessing"));
  };

  const switchLabel = (): string => {
    switch (place) {
      case "downloading":
        return t("modelSelector.downloading", {
          percentage: Math.round(percentage),
        });
      case "checking":
        return t("modelSelector.verifyingGeneric");
      case "loading":
        return t("modelSelector.loadingGeneric");
      default:
        return t("settings.advanced.wordLists.switchModel", {
          model: vocabularyModel
            ? getTranslatedModelName(vocabularyModel, t)
            : "",
        });
    }
  };
  const busy =
    place === "downloading" || place === "checking" || place === "loading";

  return (
    <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
      {showSetUp && (
        <Button variant="secondary" size="sm" onClick={() => void setUp()}>
          {t("settings.advanced.wordLists.setUpPostProcessing")}
        </Button>
      )}
      {showSwitch && (
        <span className="inline-flex flex-wrap items-center gap-x-2 gap-y-1">
          <Button
            variant="secondary"
            size="sm"
            disabled={busy}
            onClick={() => pickModel(VOCABULARY_MODEL_ID, scope)}
          >
            {switchLabel()}
          </Button>
          <span className="text-xs text-ink-3">
            {t("settings.advanced.wordLists.switchModelNote")}
          </span>
        </span>
      )}
    </div>
  );
};

interface WordListLlmCheckProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** DanyWhere: the switch `word_list_llm_check`, shown while the scope's
 * speech model takes no vocabulary: a word that resembles a word list
 * entry in a dictation without post-processing goes to the LLM, which
 * confirms or rejects the entry. It needs a provider with a model that
 * isn't Apple Intelligence and post-processing other than "never", and
 * says why when either is missing. */
export const WordListLlmCheck: React.FC<WordListLlmCheckProps> = React.memo(
  ({ descriptionMode = "inline", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const { line, setup } = useWordListModel();
    if (!line || line.state === "vocabulary") return null;
    const enabled = getSetting("word_list_llm_check") ?? false;

    const description =
      setup.kind === "ready"
        ? t("settings.advanced.wordLists.llmCheckDescription", {
            provider: providerName(setup.provider, t),
          })
        : setup.kind === "unsupported"
          ? t("settings.advanced.wordLists.llmCheckNotWith", {
              provider: providerName(setup.provider, t),
            })
          : setup.kind === "never"
            ? t("settings.advanced.wordLists.llmCheckNever", {
                mode: t("settings.postProcessing.mode.options.never"),
              })
            : t("settings.advanced.wordLists.llmCheckNeedsProvider");

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(next) => updateSetting("word_list_llm_check", next)}
        isUpdating={isUpdating("word_list_llm_check")}
        // A switch that is on can always be turned off.
        disabled={setup.kind !== "ready" && !enabled}
        label={t("settings.advanced.wordLists.llmCheckTitle")}
        description={description}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);
