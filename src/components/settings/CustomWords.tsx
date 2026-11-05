import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { notify } from "@/stores/noticeStore";
import { useSettings } from "../../hooks/useSettings";
import { isComposingKeyEvent } from "../../lib/utils/keyboard";
import {
  MAX_WORD_LENGTH,
  addWord,
  normalizeWord,
  removeWord,
} from "../../lib/utils/wordLists";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface CustomWordsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

interface WordInputProps {
  words: string[];
  onChange: (words: string[]) => void;
  disabled?: boolean;
}

/** Field and button that add a word to `words`; shared by the global list
 * and the additional word lists. */
export const WordInput: React.FC<WordInputProps> = ({
  words,
  onChange,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const [newWord, setNewWord] = useState("");
  const normalizedWord = normalizeWord(newWord);

  const handleAddWord = () => {
    const result = addWord(words, newWord);
    if (result.kind === "duplicate") {
      notify({
        title: t("settings.advanced.customWords.duplicate", {
          word: result.word,
        }),
      });
      return;
    }
    if (result.kind === "added") {
      onChange(result.words);
      setNewWord("");
    }
  };

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (isComposingKeyEvent(e.nativeEvent)) return;
    if (e.key === "Enter") {
      e.preventDefault();
      handleAddWord();
    }
  };

  return (
    <div className="flex items-center gap-2">
      <Input
        type="text"
        className="max-w-40"
        value={newWord}
        onChange={(e) => setNewWord(e.target.value)}
        onKeyDown={handleKeyPress}
        placeholder={t("settings.advanced.customWords.placeholder")}
        variant="compact"
        disabled={disabled}
      />
      <Button
        onClick={handleAddWord}
        disabled={
          !normalizedWord || normalizedWord.length > MAX_WORD_LENGTH || disabled
        }
        variant="primary"
        size="md"
      >
        {t("settings.advanced.customWords.add")}
      </Button>
    </div>
  );
};

interface WordChipsProps {
  words: string[];
  onChange: (words: string[]) => void;
  disabled?: boolean;
  className?: string;
}

/** The words as removable chips. */
export const WordChips: React.FC<WordChipsProps> = ({
  words,
  onChange,
  disabled = false,
  className = "",
}) => {
  const { t } = useTranslation();
  if (words.length === 0) return null;
  return (
    <div className={`flex flex-wrap gap-1 ${className}`}>
      {words.map((word) => (
        <Button
          key={word}
          onClick={() => onChange(removeWord(words, word))}
          disabled={disabled}
          variant="secondary"
          size="sm"
          className="inline-flex items-center gap-1 cursor-pointer"
          aria-label={t("settings.advanced.customWords.remove", { word })}
        >
          <span>{word}</span>
          <svg
            className="w-3 h-3"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M6 18L18 6M6 6l12 12"
            />
          </svg>
        </Button>
      ))}
    </div>
  );
};

/** The global word list: always given to the model. */
export const CustomWords: React.FC<CustomWordsProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const customWords = getSetting("custom_words") || [];
    const updating = isUpdating("custom_words");
    const save = (words: string[]) => updateSetting("custom_words", words);

    return (
      <>
        <SettingContainer
          title={t("settings.advanced.wordLists.globalTitle")}
          description={t("settings.advanced.customWords.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <WordInput words={customWords} onChange={save} disabled={updating} />
        </SettingContainer>
        <WordChips
          words={customWords}
          onChange={save}
          disabled={updating}
          className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-line"}`}
        />
      </>
    );
  },
);
