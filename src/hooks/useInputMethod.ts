import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getName } from "@tauri-apps/api/app";
import { commands, type InputMethodStatus } from "@/bindings";
import { readInputMethodStatus } from "@/lib/inputMethod";
import { RELEASE_PRODUCT_NAME } from "@/lib/variant";
import { notify } from "@/stores/noticeStore";

/** How often an open page asks whether the user has added the source. */
export const INPUT_METHOD_POLL_MS = 1000;

/** What a page shows of DanyWhere's input method. */
export interface InputMethodView {
  /** The backend's answer; null until the first one is in. */
  status: InputMethodStatus | null;
  /** The input source's name in System Settings: the app's product name
   * ("DanyWhere", "DanyWhere Dev", "DanyWhere NoAX"). */
  name: string;
  /** A setup is running. */
  settingUp: boolean;
  /** Installs the input method and opens System Settings at Keyboard; a
   * failure shows a notice. */
  setUp: () => Promise<void>;
}

export interface InputMethodOptions {
  /** Ask the backend at all (macOS only); otherwise `status` stays null. */
  enabled?: boolean;
  /** Ask again every second while mounted, so the state follows what the
   * user does in System Settings. */
  poll?: boolean;
  /** Debug preview: nothing is asked or set up; the source shows as not
   * added. */
  inert?: boolean;
}

const PREVIEW_STATUS: InputMethodStatus = {
  available: true,
  installed: false,
  added: false,
  connected: false,
};

/** The app's product name, which is the input source's name. */
const productName = async (): Promise<string> => {
  try {
    return (await getName()) || RELEASE_PRODUCT_NAME;
  } catch (error) {
    console.error("Failed to get the app's name:", error);
    return RELEASE_PRODUCT_NAME;
  }
};

/**
 * DanyWhere: the input method's state for the settings and the guides, read
 * once with the product name and then, with `poll`, every second while the
 * page is open.
 */
export const useInputMethod = ({
  enabled = true,
  poll = true,
  inert = false,
}: InputMethodOptions = {}): InputMethodView => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<InputMethodStatus | null>(null);
  const [name, setName] = useState("");
  const [settingUp, setSettingUp] = useState(false);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    const next = await readInputMethodStatus();
    if (alive.current) setStatus(next);
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let current = true;
    // Status and name together, so a shown status always has its name.
    Promise.all([
      inert ? Promise.resolve(PREVIEW_STATUS) : readInputMethodStatus(),
      productName(),
    ]).then(([first, found]) => {
      if (!current) return;
      setName(found);
      setStatus(first);
    });
    if (inert || !poll) {
      return () => {
        current = false;
      };
    }
    const timer = setInterval(async () => {
      const next = await readInputMethodStatus();
      if (current) setStatus(next);
    }, INPUT_METHOD_POLL_MS);
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, [enabled, poll, inert]);

  const setUp = useCallback(async () => {
    if (inert) return;
    setSettingUp(true);
    try {
      const result = await commands.setUpInputMethod();
      if (result.status === "error")
        notify({
          title: t("settings.inputMethod.setUpFailed"),
          detail: result.error,
        });
    } catch (error) {
      console.error("Failed to set up the input method:", error);
      notify({
        title: t("settings.inputMethod.setUpFailed"),
        detail: String(error),
      });
    } finally {
      if (alive.current) setSettingUp(false);
    }
    await refresh();
  }, [inert, refresh, t]);

  return { status, name, settingUp, setUp };
};
