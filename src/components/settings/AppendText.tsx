import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type AppendChoice,
  type AppendKey,
  type AppendPart,
} from "@/bindings";
import { Dropdown } from "../ui/Dropdown";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import {
  describeAppend,
  keyOptions,
  keysOfOption,
  selectedKeyOption,
} from "@/lib/appendText";

/** How long typing pauses before the preview follows the text. */
const PREVIEW_DELAY_MS = 250;

interface AppendTextProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * DanyWhere: the profile's "Append" field (append_text.rs), which replaced
 * "Append Trailing Space" and "Auto Submit": a text field and a selection
 * of the key press after the text, neither of which can show an error.
 * Rust splits the stored value into both and composes it again; the
 * preview in words under them says what a dictation gets.
 */
export const AppendText: React.FC<AppendTextProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t, i18n } = useTranslation();
    const os = useOsType();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const stored = getSetting("append_text") ?? "";
    const withoutAccessibility =
      os === "macos" && getSetting("accessibility_skipped") === true;
    const [offered, setOffered] = useState<AppendKey[]>([]);
    const [choice, setChoice] = useState<AppendChoice | null>(null);
    const [draft, setDraft] = useState("");
    const [parts, setParts] = useState<AppendPart[] | null>(null);
    // The value this field just stored, so storing it doesn't reset the text
    // field (a lone backslash may come back written as two).
    const saved = useRef<string | null>(null);

    useEffect(() => {
      let current = true;
      commands.appendKeyOptions().then((keys) => {
        if (current) setOffered(keys);
      });
      return () => {
        current = false;
      };
    }, []);

    // Follow the stored value, also changes from elsewhere (a profile
    // switch, another window).
    useEffect(() => {
      let current = true;
      const own = stored === saved.current;
      saved.current = null;
      commands.splitAppendText(stored).then((split) => {
        if (!current) return;
        setChoice(split);
        if (!own) setDraft(split.text);
      });
      return () => {
        current = false;
      };
    }, [stored]);

    // The preview follows the text once typing pauses, a stored value at once.
    useEffect(() => {
      if (!choice) return;
      let current = true;
      const timer = setTimeout(
        async () => {
          const composed = await commands.composeAppendText(draft, choice.keys);
          if (current) setParts(composed.parts);
        },
        draft === choice.text ? 0 : PREVIEW_DELAY_MS,
      );
      return () => {
        current = false;
        clearTimeout(timer);
      };
    }, [draft, choice]);

    const save = async (text: string, newKeys: AppendKey[]) => {
      const composed = await commands.composeAppendText(text, newKeys);
      setParts(composed.parts);
      if (composed.value === stored) return;
      saved.current = composed.value;
      await updateSetting("append_text", composed.value);
    };

    return (
      <SettingContainer
        title={t("settings.advanced.appendText.title")}
        description={t("settings.advanced.appendText.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        grow
      >
        <div className="flex w-full max-w-sm flex-col gap-1">
          <div className="flex w-full items-center gap-2">
            <Input
              variant="compact"
              className="min-w-0 flex-1 font-mono"
              value={draft}
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              onChange={(event) => setDraft(event.target.value)}
              onBlur={() => choice && save(draft, choice.keys)}
              onKeyDown={(event) => {
                if (event.key === "Enter") event.currentTarget.blur();
              }}
              disabled={isUpdating("append_text") || choice === null}
            />
            <Dropdown
              compact
              className="w-40 shrink-0"
              options={choice ? keyOptions(offered, choice, t, os) : []}
              selectedValue={choice ? selectedKeyOption(choice) : null}
              onSelect={(value) => {
                if (choice) save(draft, keysOfOption(value, choice));
              }}
              disabled={choice === null || withoutAccessibility}
            />
          </div>
          {parts && (
            <p className="text-xs text-text/70">
              {describeAppend(parts, t, os, i18n.language)}
            </p>
          )}
          {withoutAccessibility && (
            <p className="text-xs text-text/70">
              {t("settings.advanced.appendText.noAccessibility")}
            </p>
          )}
        </div>
      </SettingContainer>
    );
  },
);
