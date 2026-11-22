// DanyWhere: the stand-in for `src/bindings.ts` in component tests. The
// preload (`src/test/setup.ts`) puts it in place with `mock.module`, so every
// component that imports `commands` or `events` gets these instead.
//
// - Every command of the real bindings exists (read from `bindings.ts`, so a
//   new command needs nothing here). It records its call and answers with
//   what a test set with `mockCommand`, else with a default that fits its
//   return type: `{ status: "ok", data: [] }` for a list, `false` for a
//   boolean, `null` for anything else.
// - Events live in memory: `emitEvent` calls what a component registered
//   with `events.<name>.listen`.
// - `resetBindingsMock` (after every test) forgets calls, answers and
//   listeners.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type {
  commands as RealCommands,
  events as RealEvents,
} from "@/bindings";
import type { FieldKind } from "@/lib/profileScope";

type Commands = typeof RealCommands;
export type CommandName = keyof Commands;
type Answer<K extends CommandName> = Awaited<ReturnType<Commands[K]>>;
export type CommandImpl<K extends CommandName> = (
  ...args: Parameters<Commands[K]>
) => Answer<K> | Promise<Answer<K>>;

/** What a command returns: a `Result` around `data`, or `data` itself. */
interface Signature {
  result: boolean;
  data: string;
}

const BINDINGS = fileURLToPath(new URL("../bindings.ts", import.meta.url));
// tauri-specta writes each command's signature on one line.
const SIGNATURE = /^async (\w+)\(.*\)\s*:\s*Promise<(.*)>\s*\{$/gm;
const RESULT = /^Result<(.*), [^,]+>$/;

const readSignatures = (): Map<string, Signature> => {
  const signatures = new Map<string, Signature>();
  for (const match of readFileSync(BINDINGS, "utf8").matchAll(SIGNATURE)) {
    const result = RESULT.exec(match[2]);
    signatures.set(
      match[1],
      result
        ? { result: true, data: result[1] }
        : { result: false, data: match[2] },
    );
  }
  return signatures;
};

const SIGNATURES = readSignatures();

/** The default data of a return type: empty for lists and text, false for
 * a boolean, nothing for the rest. */
const defaultData = (type: string): unknown => {
  const trimmed = type.trim();
  if (trimmed.endsWith("[]")) return [];
  switch (trimmed) {
    case "boolean":
      return false;
    case "string":
      return "";
    case "number":
      return 0;
    case "void":
      return undefined;
    default:
      return null;
  }
};

export const ok = <T>(data: T) => ({ status: "ok" as const, data });
export const err = (error: string) => ({ status: "error" as const, error });

/** The fields that couple entry by entry (`profile_layers::LIST_FIELDS`);
 * every other field couples as a value, which is what `kindOf` assumes for
 * a name it doesn't know. */
export const PROFILE_LIST_FIELDS: Record<string, FieldKind> = {
  word_lists: { kind: "list", key: "id" },
  post_process_providers: { kind: "list", key: "id" },
  post_process_models: { kind: "map" },
  post_process_efforts: { kind: "map" },
  post_process_api_keys: { kind: "map" },
  bindings: { kind: "map" },
  end_keys: { kind: "list", key: "id" },
};

/** Answers that a default by type would get wrong, as the backend gives
 * them when nothing special is set up. */
const DEFAULT_ANSWERS: Partial<
  Record<string, (...args: unknown[]) => unknown>
> = {
  checkCustomSounds: () => ({ start: false, stop: false }),
  // An "Append" value as plain text without a key press (append_text.rs
  // splits off the key and spells the text out).
  splitAppendText: (value) => ({ text: value, keys: [], kept: false }),
  composeAppendText: (text) => ({
    value: text,
    parts: text ? [{ kind: "text", text }] : [],
  }),
  getProfileFields: () =>
    Object.entries(PROFILE_LIST_FIELDS).map(([name, kind]) => ({
      name,
      kind,
    })),
};

export interface CommandCall {
  command: string;
  args: unknown[];
}

const overrides = new Map<string, (...args: unknown[]) => unknown>();
const recorded: CommandCall[] = [];

/** Every command call since the last reset, in order. */
export const commandCalls = (): readonly CommandCall[] => recorded;

/** The arguments of each call of `name`, in order. */
export const callsOf = <K extends CommandName>(
  name: K,
): Parameters<Commands[K]>[] =>
  recorded
    .filter((call) => call.command === name)
    .map((call) => call.args as Parameters<Commands[K]>);

/** Answers `name` with `impl` until the next reset. */
export const mockCommand = <K extends CommandName>(
  name: K,
  impl: CommandImpl<K>,
): void => {
  if (!SIGNATURES.has(name)) {
    throw new Error(`bindings.ts has no command ${String(name)}`);
  }
  overrides.set(name, impl as (...args: unknown[]) => unknown);
};

const answer = (name: string, signature: Signature, args: unknown[]) => {
  const impl = overrides.get(name) ?? DEFAULT_ANSWERS[name];
  if (impl) return impl(...args);
  const data = defaultData(signature.data);
  return signature.result ? ok(data) : data;
};

export const commands = Object.fromEntries(
  [...SIGNATURES].map(([name, signature]) => [
    name,
    async (...args: unknown[]) => {
      recorded.push({ command: name, args });
      return answer(name, signature, args);
    },
  ]),
) as unknown as Commands;

// ---- events -----------------------------------------------------------------

type Events = typeof RealEvents;
export type EventName = keyof Events;
type Listener = (event: {
  event: string;
  id: number;
  payload: unknown;
}) => void;
type PayloadOf<K extends EventName> = Parameters<
  Parameters<Events[K]["listen"]>[0]
>[0]["payload"];

const listeners = new Map<string, Set<Listener>>();
let eventId = 0;

const listenersOf = (name: string): Set<Listener> => {
  let set = listeners.get(name);
  if (!set) {
    set = new Set();
    listeners.set(name, set);
  }
  return set;
};

/** Calls every listener of `name` with `payload`, as the backend would. */
export const emitEvent = <K extends EventName>(
  name: K,
  payload: PayloadOf<K>,
): void => {
  for (const listener of [...listenersOf(String(name))]) {
    listener({ event: String(name), id: ++eventId, payload });
  }
};

const eventObject = (name: string) => ({
  listen: async (callback: Listener) => {
    listenersOf(name).add(callback);
    return () => {
      listenersOf(name).delete(callback);
    };
  },
  once: async (callback: Listener) => {
    const once: Listener = (event) => {
      listenersOf(name).delete(once);
      callback(event);
    };
    listenersOf(name).add(once);
    return () => {
      listenersOf(name).delete(once);
    };
  },
  emit: async (payload: unknown) => {
    for (const listener of [...listenersOf(name)]) {
      listener({ event: name, id: ++eventId, payload });
    }
  },
});

export const events = new Proxy(
  {},
  {
    get: (_, name) => {
      const object = eventObject(String(name));
      // `events.x(window)` gives the same object as `events.x`.
      return Object.assign(() => object, object);
    },
  },
) as unknown as Events;

/** Forgets every call, answer and listener. */
export const resetBindingsMock = (): void => {
  overrides.clear();
  recorded.length = 0;
  listeners.clear();
};
