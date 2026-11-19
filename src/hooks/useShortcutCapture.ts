import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { notify } from "@/stores/noticeStore";
import { commands } from "@/bindings";
import { SECURE_INPUT_HELP_URL } from "@/components/SecureInputWarning";
import { useSettings } from "@/hooks/useSettings";
import {
  getKeyName,
  MODIFIERS,
  normalizeKey,
  type OSType,
} from "@/lib/utils/keyboard";

interface HandyKeysEvent {
  modifiers: string[];
  key: string | null;
  is_key_down: boolean;
  hotkey_string: string;
}

/**
 * DanyWhere: the toast for a refused binding. Without Accessibility "Paste
 * Last Transcript" is the "Insert Last Dictation" service's shortcut, and
 * the backend refuses what a service can't take with a code of its own.
 */
const bindingErrorTitle = (
  error: string,
  t: ReturnType<typeof useTranslation>["t"],
): string => {
  if (error.includes("service-shortcut-invalid"))
    return t("settings.general.insertService.invalid");
  const conflict = /conflict:([a-z_]+)/.exec(error);
  if (conflict)
    return t("settings.general.insertService.conflict", {
      name: t(
        `settings.general.shortcut.bindings.${conflict[1]}.name`,
        conflict[1],
      ),
    });
  return t("settings.general.shortcut.errors.set", { error });
};

/**
 * Records a new binding via handy-keys events, or this window's key events
 * with all bindings suspended. `inert` records without saving;
 * `escapeCancels` makes Escape exit instead of recording. DanyWhere:
 * `onCapture` takes the recorded combination instead of storing it as the
 * binding (an end shortcut's, end_keys.rs); a thrown error is shown.
 */
export const useShortcutCapture = (
  bindingId: string,
  osType: OSType,
  {
    onDone,
    inert = false,
    escapeCancels = false,
    onCapture,
  }: {
    onDone?: () => void;
    inert?: boolean;
    escapeCancels?: boolean;
    onCapture?: (binding: string) => Promise<void> | void;
  } = {},
) => {
  const { t } = useTranslation();
  const { getSetting, updateBinding } = useSettings();
  const handyKeys =
    !inert && getSetting("keyboard_implementation") === "handy_keys";
  const [capturing, setCapturing] = useState(false);
  const [partial, setPartial] = useState("");
  const onDoneRef = useRef(onDone);
  onDoneRef.current = onDone;

  // What the capture holds in the backend, released on end or unmount.
  const held = useRef<"handyKeys" | "bindings" | null>(null);
  const release = useCallback(async () => {
    const was = held.current;
    held.current = null;
    if (was === "handyKeys")
      await commands.stopHandyKeysRecording().catch(console.error);
    else if (was === "bindings")
      await commands.resumeAllBindings().catch(console.error);
  }, []);
  useEffect(
    () => () => {
      release();
    },
    [release],
  );

  const finish = useCallback(async () => {
    setCapturing(false);
    setPartial("");
    await release();
    onDoneRef.current?.();
  }, [release]);

  const commit = useCallback(
    async (binding: string) => {
      try {
        if (onCapture) await onCapture(binding);
        else if (!inert) await updateBinding(bindingId, binding);
      } catch (error) {
        console.error("Failed to change binding:", error);
        notify({ title: bindingErrorTitle(String(error), t) });
      }
      await finish();
    },
    [bindingId, finish, inert, onCapture, t, updateBinding],
  );

  /** Resolves to whether capture began. */
  const start = useCallback(async () => {
    if (held.current) return false;
    if (handyKeys) {
      // Refused while macOS Secure Input is on (no key events would arrive);
      // the backend also shows the warning banner the notice points at.
      const result = await commands
        .startHandyKeysRecording(bindingId)
        .catch((error) => ({ status: "error" as const, error: String(error) }));
      if (result.status === "error") {
        if (String(result.error).includes("secure-input-active")) {
          notify({
            title: t("secureInput.recorderBlocked"),
            action: {
              label: t("secureInput.learnMore"),
              onClick: () => openUrl(SECURE_INPUT_HELP_URL),
            },
          });
        } else {
          notify({
            title: t("settings.general.shortcut.errors.set", {
              error: result.error,
            }),
          });
        }
        return false;
      }
      held.current = "handyKeys";
    } else if (!inert) {
      await commands.suspendAllBindings().catch(console.error);
      held.current = "bindings";
    }
    setPartial("");
    setCapturing(true);
    return true;
  }, [bindingId, handyKeys, inert, t]);

  const cancel = useCallback(() => {
    if (capturing) finish();
  }, [capturing, finish]);

  useEffect(() => {
    if (!capturing || !escapeCancels) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      finish();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing, escapeCancels, finish]);

  // handy-keys: the backend reports keys while recording.
  useEffect(() => {
    if (!capturing || !handyKeys) return;
    let keyed = "";
    let modifierOnly = "";
    let stopped = false;
    const pending = listen<HandyKeysEvent>("handy-keys-event", (event) => {
      if (stopped) return;
      const { hotkey_string, is_key_down, key, modifiers } = event.payload;
      if (is_key_down && hotkey_string) {
        if (key) keyed = hotkey_string;
        else modifierOnly = hotkey_string;
        setPartial(hotkey_string);
      } else if (!is_key_down && key) {
        // Commit the keyed combo, never bare modifiers (that overwrote bindings).
        const binding = keyed || hotkey_string;
        if (binding) {
          stopped = true;
          commit(binding);
        }
      } else if (!is_key_down && !key && !modifiers.length && !keyed) {
        if (modifierOnly) {
          stopped = true;
          commit(modifierOnly);
        }
      }
    });
    return () => {
      stopped = true;
      pending.then((unlisten) => unlisten());
    };
  }, [capturing, handyKeys, commit]);

  // Tauri shortcuts: this window's own key events.
  useEffect(() => {
    if (!capturing || handyKeys) return;
    const pressed = new Set<string>();
    const recorded: string[] = [];
    const onDown = (e: KeyboardEvent) => {
      if (escapeCancels && e.key === "Escape") return;
      e.preventDefault();
      const key = normalizeKey(getKeyName(e, osType));
      pressed.add(key);
      if (!recorded.includes(key)) recorded.push(key);
      setPartial(recorded.join("+"));
    };
    const onUp = (e: KeyboardEvent) => {
      e.preventDefault();
      pressed.delete(normalizeKey(getKeyName(e, osType)));
      if (pressed.size || !recorded.length) return;
      const sorted = [...recorded].sort(
        (a, b) => +!MODIFIERS.includes(a) - +!MODIFIERS.includes(b),
      );
      commit(sorted.join("+"));
    };
    window.addEventListener("keydown", onDown);
    window.addEventListener("keyup", onUp);
    return () => {
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
    };
  }, [capturing, handyKeys, commit, escapeCancels, osType]);

  return { capturing, partial, start, cancel };
};
