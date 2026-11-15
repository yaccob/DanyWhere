import { useState } from "react";
import { commands } from "@/bindings";
import { useSettings } from "./useSettings";

/** DanyWhere: the active profile's "Send Context" (`send_context.rs`). The
 * settings hold the active profile's value, so a profile switch shows the
 * new profile's; the tray changes it too, and the shortcut when the app in
 * front uses the active profile; the settings follow through
 * `settings-changed`. `postProcessing`: whether the active profile
 * post-processes, without which the switch does nothing. */
export const useSendContext = () => {
  const { getSetting, refreshSettings } = useSettings();
  const [busy, setBusy] = useState(false);

  const enabled = getSetting("send_context") ?? false;
  const postProcessing = getSetting("post_process_enabled") ?? false;

  const setEnabled = async (value: boolean) => {
    setBusy(true);
    try {
      await commands.setSendContext(value);
    } catch (error) {
      console.error("Failed to switch Send Context:", error);
    } finally {
      await refreshSettings();
      setBusy(false);
    }
  };

  return { enabled, postProcessing, setEnabled, busy };
};
