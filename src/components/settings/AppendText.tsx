import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import {
  commands,
  type AppendKeyNames,
  type AppendPart,
  type AppendTextError,
} from "@/bindings";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import {
  appendDescription,
  appendErrorMessage,
  describeAppend,
} from "@/lib/appendText";

/** How long typing pauses before the field is parsed for the preview. */
const PREVIEW_DELAY_MS = 250;

type Parsed =
  | { value: string; parts: AppendPart[] }
  | { value: string; error: AppendTextError };

interface AppendTextProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * DanyWhere: the profile's "Append" field (append_text.rs), which replaced
 * "Append Trailing Space" and "Auto Submit". Rust parses it, for the
 * preview in words under the field and when it is stored; an invalid value
 * shows its error and is not stored. Key names are this system's, as Rust
 * gives them (append_key_names, and each key part's name).
 */
export const AppendText: React.FC<AppendTextProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t, i18n } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const stored = getSetting("append_text") ?? "";
    const withoutAccessibility =
      type() === "macos" && getSetting("accessibility_skipped") === true;
    const [draft, setDraft] = useState(stored);
    const [parsed, setParsed] = useState<Parsed | null>(null);
    const [names, setNames] = useState<AppendKeyNames | null>(null);

    useEffect(() => {
      let current = true;
      commands.appendKeyNames().then((loaded) => {
        if (current) setNames(loaded);
      });
      return () => {
        current = false;
      };
    }, []);

    // Follow changes from elsewhere (a profile switch, another window).
    useEffect(() => setDraft(stored), [stored]);

    // Parse while typing, once typing pauses.
    useEffect(() => {
      let current = true;
      const timer = setTimeout(
        async () => {
          const result = await commands.parseAppendText(draft);
          if (!current) return;
          setParsed(
            result.status === "ok"
              ? { value: draft, parts: result.data }
              : { value: draft, error: result.error },
          );
        },
        draft === stored ? 0 : PREVIEW_DELAY_MS,
      );
      return () => {
        current = false;
        clearTimeout(timer);
      };
    }, [draft, stored]);

    const commit = async () => {
      if (draft === stored) return;
      const result = await commands.parseAppendText(draft);
      if (result.status === "error") {
        setParsed({ value: draft, error: result.error });
        return;
      }
      await updateSetting("append_text", draft);
    };

    const error = parsed && "error" in parsed ? parsed.error : null;

    return (
      <SettingContainer
        title={t("settings.advanced.appendText.title")}
        description={names ? appendDescription(t, names) : ""}
        descriptionMode={descriptionMode}
        grouped={grouped}
        grow
      >
        <div className="flex w-full max-w-sm flex-col gap-1">
          <Input
            variant="compact"
            className={`w-full font-mono ${error ? "border-red-500" : ""}`}
            value={draft}
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="off"
            aria-invalid={error !== null}
            onChange={(event) => setDraft(event.target.value)}
            onBlur={commit}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
            disabled={isUpdating("append_text")}
          />
          {parsed && "parts" in parsed && (
            <p className="text-xs text-text/70">
              {describeAppend(parsed.parts, t, i18n.language)}
            </p>
          )}
          {error && names && (
            <p className="text-xs text-red-500">
              {appendErrorMessage(error, t, names.platform)}{" "}
              {draft !== stored && t("settings.advanced.appendText.notSaved")}
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
