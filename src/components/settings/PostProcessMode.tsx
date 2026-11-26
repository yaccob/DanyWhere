import React from "react";
import { useTranslation } from "react-i18next";
import type { PostProcessMode } from "@/bindings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

interface PostProcessModeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** The choices in their order, by the key of their label. */
const MODES: ReadonlyArray<[PostProcessMode, string]> = [
  ["never", "never"],
  ["with_instruction_key", "withInstructionKey"],
  ["always", "always"],
];

/**
 * DanyWhere: when a dictation is post-processed, per profile (qa
 * 2026-11-26_1841, decision: variant b; 2026-11-26_1810, "Schnittstelle
 * Phase 2"), where Handy's switch "Post Processing" was. Never: nothing
 * goes to a provider and instruction shortcuts aren't taken. With
 * Instruction Shortcut (out of the box): only a dictation in which one was
 * pressed. Always: every dictation; an instruction shortcut adds an
 * instruction. A mode not known shows as Never, which is what the backend
 * treats it as.
 */
export const PostProcessModeSetting: React.FC<PostProcessModeProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const mode = getSetting("post_process_mode") ?? "never";

    return (
      <SettingContainer
        title={t("settings.postProcessing.mode.title")}
        description={t("settings.postProcessing.mode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={MODES.map(([value, key]) => ({
            value,
            label: t(`settings.postProcessing.mode.options.${key}`),
          }))}
          selectedValue={mode}
          onSelect={(value) =>
            updateSetting("post_process_mode", value as PostProcessMode)
          }
          disabled={isUpdating("post_process_mode")}
        />
      </SettingContainer>
    );
  });
