// DanyWhere: rendering a settings page in a component test with the
// settings it should see. The settings go where the app has them (the
// settings store) and are what `get_app_settings` answers, so a refresh
// after a change reads them again; how the fields couple is known at once.
import React from "react";
import {
  act,
  fireEvent,
  render,
  type RenderResult,
} from "@testing-library/react";
import type { AppSettings } from "@/bindings";
import type { Scope } from "@/lib/profileScope";
import { useNoticeStore } from "@/stores/noticeStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useFieldKinds } from "@/components/settings/profiles/scope/fieldKinds";
import { SettingsScopeProvider } from "@/components/settings/profiles/scope/SettingsScope";
import { PROFILE_LIST_FIELDS, mockCommand, ok } from "./bindings-mock";
import { settingsWith } from "./fixtures";

/** What `get_app_settings` (and `get_default_settings`) answer from now
 * on, as the backend's state after a change. */
export const setBackendSettings = (settings: AppSettings): void => {
  mockCommand("getAppSettings", () => ok(settings));
  mockCommand("getDefaultSettings", () => ok(settings));
};

type Layered = Omit<AppSettings, "default_values" | "profiles"> & {
  default_values: Record<string, unknown>;
  profiles: {
    id: string;
    values: Record<string, unknown>;
    own?: string[];
    own_entries?: Record<string, unknown>;
  }[];
};

/** `settings` after `set_profile_value(scope, field, value)`, as far as a
 * page notices: the scope's value changes; under the defaults the profiles
 * that follow the field change along; in a profile a value becomes its own
 * (lists keep their entries' states, which a test sets up itself). */
export const withValue = (
  settings: AppSettings,
  scope: Scope,
  field: string,
  value: unknown,
): AppSettings => {
  const next = structuredClone(settings) as unknown as Layered;
  const done = () => next as unknown as AppSettings;
  const list = PROFILE_LIST_FIELDS[field] !== undefined;
  const inUse = (id: string) => next.active_profile_id === id;
  if (scope.kind === "defaults") {
    next.default_values[field] = value;
    for (const profile of next.profiles) {
      const holds = list
        ? Object.keys(profile.own_entries ?? {}).includes(field)
        : (profile.own ?? []).includes(field);
      if (!holds) profile.values[field] = value;
    }
    if (!next.profiles.some((p) => inUse(p.id))) {
      (next as Record<string, unknown>)[field] = value;
    }
    return done();
  }
  const profile = next.profiles.find((p) => p.id === scope.id);
  if (!profile) return done();
  profile.values[field] = value;
  if (!list && !(profile.own ?? []).includes(field)) {
    profile.own = [...(profile.own ?? []), field];
  }
  if (inUse(profile.id)) (next as Record<string, unknown>)[field] = value;
  return done();
};

/** From now on the backend takes every `set_profile_value` (`withValue`)
 * and answers the refresh after it with the new state. */
export const acceptValueWrites = (settings: AppSettings): void => {
  let current = settings;
  setBackendSettings(current);
  mockCommand("setProfileValue", async (scope, field, value) => {
    current = withValue(current, scope, field, value);
    setBackendSettings(current);
    return ok(null);
  });
};

export interface RenderPageOptions {
  /** The settings the backend has (`settingsWith`); its defaults if missing. */
  settings?: AppSettings;
  /** The page's scope: the defaults or a profile; none for a page outside
   * the settings' scopes. */
  scope?: Scope | null;
  /** The backend takes `set_profile_value` (`acceptValueWrites`), so the
   * page shows a change after its refresh as it would in the app. */
  acceptWrites?: boolean;
}

/**
 * Renders `ui` as the settings window would, with `settings` loaded, inside
 * `scope`, and lets its first effects and answers run. It sets what
 * `get_app_settings` answers; a test that answers it itself does so after.
 */
export const renderPage = async (
  ui: React.ReactElement,
  {
    settings = settingsWith(),
    scope = null,
    acceptWrites = false,
  }: RenderPageOptions = {},
): Promise<RenderResult> => {
  if (acceptWrites) acceptValueWrites(settings);
  else setBackendSettings(settings);
  useSettingsStore.setState({
    settings,
    defaultSettings: settings,
    isLoading: false,
  });
  useFieldKinds.setState({ loaded: true, kinds: PROFILE_LIST_FIELDS });
  const tree = scope ? (
    <SettingsScopeProvider scope={scope}>{ui}</SettingsScopeProvider>
  ) : (
    ui
  );
  let result: RenderResult | undefined;
  await act(async () => {
    result = render(tree);
  });
  await settle();
  if (!result) throw new Error("render gave nothing");
  return result;
};

/** The notices the frontend raised (`notify`), newest first, as their toast
 * reads: the title, and the detail after it. */
export const raisedNotices = (): string[] =>
  useNoticeStore
    .getState()
    .local.map(({ title, detail }) =>
      detail ? `${title} | ${detail}` : title,
    );

/** A turn of the event loop: answers, `setTimeout(…, 0)` and what they
 * start. */
const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/** Lets pending answers, refreshes, effects and zero-delay timers run, so
 * their state updates happen inside `act`. */
export const settle = (): Promise<void> =>
  act(async () => {
    for (let turn = 0; turn < 3; turn += 1) await tick();
  });

/** The `key` of a key event for its `code`, as far as tests press them. */
const KEY_OF_CODE: Record<string, string> = {
  ControlLeft: "Control",
  ShiftLeft: "Shift",
  AltLeft: "Alt",
  MetaLeft: "Meta",
  Enter: "Enter",
  Escape: "Escape",
  Space: " ",
  Tab: "Tab",
};

/** Presses the keys with these codes (`ControlLeft`, `Enter`, `KeyJ`) on the
 * window one after the other and lets them go in reverse, as a person
 * presses a combination; then lets what they started finish. */
export const pressKeys = async (...codes: string[]): Promise<void> => {
  const keyOf = (code: string) =>
    KEY_OF_CODE[code] ?? code.replace(/^Key/, "").toLowerCase();
  await act(async () => {
    for (const code of codes) {
      fireEvent.keyDown(window, { code, key: keyOf(code) });
    }
    for (const code of [...codes].reverse()) {
      fireEvent.keyUp(window, { code, key: keyOf(code) });
    }
  });
  await settle();
};

/** Like `pressKeys`, but each press and release is an event of its own with
 * the page rendering in between, as a person's key events arrive in the
 * app: state a capture keeps across renders must survive them. */
export const typeKeys = async (...codes: string[]): Promise<void> => {
  const keyOf = (code: string) =>
    KEY_OF_CODE[code] ?? code.replace(/^Key/, "").toLowerCase();
  for (const code of codes) {
    await act(async () => {
      fireEvent.keyDown(window, { code, key: keyOf(code) });
    });
    await settle();
  }
  for (const code of [...codes].reverse()) {
    await act(async () => {
      fireEvent.keyUp(window, { code, key: keyOf(code) });
    });
    await settle();
  }
};

/** Clicks `element` and lets what the click started finish (a command, the
 * refresh after it). */
export const press = async (element: Element): Promise<void> => {
  await act(async () => {
    fireEvent.click(element);
  });
  await settle();
};
