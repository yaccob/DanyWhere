// DanyWhere: the post-processing prompts (idea 1): the library all profiles
// share, in the active profile's order with its active prompts. The list
// (`PromptLibraryList`) edits the order of whichever profile it is given;
// here that is the active one, through the flat setting, and the library
// through its commands.
import React from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { commands, type LLMPrompt } from "@/bindings";
import { SettingContainer } from "@/components/ui";
import { useSettings } from "../../../hooks/useSettings";
import { notify } from "@/stores/noticeStore";
import { PromptLibraryList } from "./PromptLibraryList";

/** The backend's code for a name another prompt has (`prompt_library`). */
const NAME_TAKEN = "prompt_name_taken";

export const PromptLibrary: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, refreshSettings } = useSettings();
  const prompts = getSetting("post_process_prompts") || [];
  const order = getSetting("post_process_prompt_order") || [];

  /** Tells about a refused save; `false` for the list. */
  const refused = (error: string, name: string): false => {
    notify({
      title:
        error === NAME_TAKEN
          ? t("settings.postProcessing.prompts.library.nameTaken", { name })
          : t("settings.postProcessing.prompts.library.saveFailed"),
      detail: error === NAME_TAKEN ? undefined : error,
      level: "warning",
    });
    return false;
  };

  const onSave = async (id: string, name: string, text: string) => {
    const result = await commands.updatePostProcessPrompt(id, name, text);
    if (result.status === "error") return refused(result.error, name);
    await refreshSettings();
    return true;
  };

  const onCreate = async (name: string, text: string) => {
    const result = await commands.addPostProcessPrompt(name, text);
    if (result.status === "error") return refused(result.error, name);
    await refreshSettings();
    return true;
  };

  /** Names the profiles that have the prompt active and asks first. */
  const onDelete = async (prompt: LLMPrompt) => {
    const profiles = await commands.profilesUsingPrompt(prompt.id);
    if (profiles.length > 0) {
      let confirmed = false;
      try {
        confirmed = await ask(
          t("settings.postProcessing.prompts.library.deleteActiveIn", {
            name: prompt.name,
            profiles: profiles.join(", "),
          }),
          {
            title: t("settings.postProcessing.prompts.library.deleteTitle"),
            kind: "warning",
            okLabel: t("settings.postProcessing.prompts.deletePrompt"),
            cancelLabel: t("settings.postProcessing.prompts.cancel"),
          },
        );
      } catch (error) {
        // Without the dialog nothing is deleted unasked.
        console.error("Failed to ask before deleting a prompt:", error);
      }
      if (!confirmed) return;
    }
    const result = await commands.deletePostProcessPrompt(prompt.id);
    if (result.status === "error") {
      notify({
        title: t("settings.postProcessing.prompts.library.deleteFailed"),
        detail: result.error,
        level: "warning",
      });
    }
    await refreshSettings();
  };

  return (
    <SettingContainer
      title={t("settings.postProcessing.prompts.library.title")}
      description={t("settings.postProcessing.prompts.library.description")}
      descriptionMode="tooltip"
      layout="stacked"
      grouped={true}
    >
      <PromptLibraryList
        prompts={prompts}
        order={order}
        onOrderChange={(next) =>
          updateSetting("post_process_prompt_order", next)
        }
        onSave={onSave}
        onCreate={onCreate}
        onDelete={onDelete}
      />
    </SettingContainer>
  );
};
