import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { NO_WORD_LIST, activeListOption } from "../../lib/utils/wordLists";
import { Button } from "../ui/Button";
import { Dropdown } from "../ui/Dropdown";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";
import { WordChips, WordInput } from "./CustomWords";

interface WordListsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** Which additional word list is switched on, if any. */
export const ActiveWordList: React.FC<WordListsProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const lists = getSetting("word_lists") || [];
    if (lists.length === 0) return null;

    return (
      <SettingContainer
        title={t("settings.advanced.wordLists.activeTitle")}
        description={t("settings.advanced.wordLists.activeDescription")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          selectedValue={activeListOption(
            lists,
            getSetting("active_word_list_id"),
          )}
          options={[
            {
              value: NO_WORD_LIST,
              label: t("settings.advanced.wordLists.onlyGlobal"),
            },
            ...lists.map((list) => ({ value: list.id, label: list.name })),
          ]}
          onSelect={(value) =>
            updateSetting(
              "active_word_list_id",
              value === NO_WORD_LIST ? null : value,
            )
          }
          disabled={isUpdating("active_word_list_id")}
        />
      </SettingContainer>
    );
  },
);
ActiveWordList.displayName = "ActiveWordList";

/** Create, rename, delete and fill the additional word lists, the way the
 * post-processing prompts are managed. */
export const WordLists: React.FC<WordListsProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, refreshSettings } = useSettings();
    const lists = getSetting("word_lists") || [];
    const activeId = getSetting("active_word_list_id") ?? null;

    const [editingId, setEditingId] = useState<string | null>(null);
    const [isCreating, setIsCreating] = useState(false);
    const [draftName, setDraftName] = useState("");
    const [busy, setBusy] = useState(false);

    // Edit the active list by default, else the first; follow deletions.
    const editing =
      lists.find((list) => list.id === editingId) ??
      lists.find((list) => list.id === activeId) ??
      lists[0] ??
      null;

    useEffect(() => {
      if (!isCreating) setDraftName(editing?.name ?? "");
    }, [isCreating, editing?.id, editing?.name]);

    const run = async (action: () => Promise<unknown>) => {
      setBusy(true);
      try {
        await action();
        await refreshSettings();
      } catch (error) {
        console.error("Word list change failed:", error);
      } finally {
        setBusy(false);
      }
    };

    const handleCreate = () =>
      run(async () => {
        const result = await commands.addWordList(draftName.trim());
        if (result.status === "ok") {
          setEditingId(result.data.id);
          setIsCreating(false);
        }
      });

    const handleRename = () =>
      editing &&
      run(() => commands.renameWordList(editing.id, draftName.trim()));

    const handleDelete = () =>
      editing &&
      run(async () => {
        await commands.deleteWordList(editing.id);
        setEditingId(null);
      });

    const editingWords = editing?.words ?? [];

    const handleWords = (words: string[]) =>
      editing && run(() => commands.setWordListWords(editing.id, words));

    const startCreate = () => {
      setIsCreating(true);
      setDraftName("");
    };

    const cancelCreate = () => {
      setIsCreating(false);
      setDraftName(editing?.name ?? "");
    };

    return (
      <SettingContainer
        title={t("settings.advanced.wordLists.title")}
        description={t("settings.advanced.wordLists.description")}
        descriptionMode={descriptionMode}
        layout="stacked"
        grouped={grouped}
      >
        <div className="space-y-3">
          <div className="flex gap-2 min-w-0">
            <Dropdown
              selectedValue={editing?.id ?? null}
              options={lists.map((list) => ({
                value: list.id,
                label: list.name,
              }))}
              onSelect={(value) => {
                setEditingId(value);
                setIsCreating(false);
              }}
              placeholder={
                lists.length === 0
                  ? t("settings.advanced.wordLists.empty")
                  : t("settings.advanced.wordLists.selectList")
              }
              disabled={isCreating || lists.length === 0}
              className="flex-1 min-w-0"
            />
            <Button
              onClick={startCreate}
              variant="primary"
              size="md"
              disabled={isCreating || busy}
              className="shrink-0"
            >
              {t("settings.advanced.wordLists.createNew")}
            </Button>
          </div>

          {isCreating && (
            <div className="flex gap-2 items-center">
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && draftName.trim()) handleCreate();
                }}
                placeholder={t("settings.advanced.wordLists.namePlaceholder")}
                variant="compact"
                className="flex-1"
              />
              <Button
                onClick={handleCreate}
                variant="primary"
                size="md"
                disabled={!draftName.trim() || busy}
              >
                {t("settings.advanced.wordLists.create")}
              </Button>
              <Button onClick={cancelCreate} variant="secondary" size="md">
                {t("settings.postProcessing.prompts.cancel")}
              </Button>
            </div>
          )}

          {!isCreating && editing && (
            <div className="space-y-3">
              <div className="flex gap-2 items-center">
                <Input
                  type="text"
                  value={draftName}
                  onChange={(e) => setDraftName(e.target.value)}
                  placeholder={t("settings.advanced.wordLists.namePlaceholder")}
                  variant="compact"
                  className="flex-1"
                />
                <Button
                  onClick={handleRename}
                  variant="primary"
                  size="md"
                  disabled={
                    !draftName.trim() ||
                    draftName.trim() === editing.name ||
                    busy
                  }
                >
                  {t("settings.advanced.wordLists.rename")}
                </Button>
                <Button
                  onClick={handleDelete}
                  variant="secondary"
                  size="md"
                  disabled={busy}
                >
                  {t("settings.advanced.wordLists.delete")}
                </Button>
              </div>
              <WordInput
                words={editingWords}
                onChange={handleWords}
                disabled={busy}
              />
              <WordChips
                words={editingWords}
                onChange={handleWords}
                disabled={busy}
              />
            </div>
          )}
        </div>
      </SettingContainer>
    );
  },
);
WordLists.displayName = "WordLists";
