// The manual's screenshots: the stand-in for `src/bindings.ts` in the
// browser (the Vite config swaps it in). It works like the component tests'
// `src/test/bindings-mock.ts`, which reads `bindings.ts` from disk and so
// can't run in a browser:
//
// - Every command of the real bindings exists, read from the generated file
//   itself (`?raw`), so a new command needs nothing here. It answers with
//   what `answerCommand` set, else with a default that fits its return type:
//   `{ status: "ok", data: [] }` for a list, `false` for a boolean, `null`
//   for anything else. Nothing is written anywhere.
// - Events live in memory; the backend never sends any.
import bindingsSource from "../../../../src/bindings.ts?raw";
import type {
  commands as RealCommands,
  events as RealEvents,
} from "../../../../src/bindings";

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

// tauri-specta writes each command's signature on one line.
const SIGNATURE = /^async (\w+)\(.*\)\s*:\s*Promise<(.*)>\s*\{$/gm;
const RESULT = /^Result<(.*), [^,]+>$/;

const SIGNATURES = new Map<string, Signature>();
for (const match of bindingsSource.matchAll(SIGNATURE)) {
  const result = RESULT.exec(match[2]);
  SIGNATURES.set(
    match[1],
    result
      ? { result: true, data: result[1] }
      : { result: false, data: match[2] },
  );
}

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

const answers = new Map<string, (...args: unknown[]) => unknown>();

/** Answers the command `name` with `impl` from now on. */
export const answerCommand = <K extends CommandName>(
  name: K,
  impl: CommandImpl<K>,
): void => {
  if (!SIGNATURES.has(name)) {
    throw new Error(`bindings.ts has no command ${String(name)}`);
  }
  answers.set(name, impl as (...args: unknown[]) => unknown);
};

export const commands = Object.fromEntries(
  [...SIGNATURES].map(([name, signature]) => [
    name,
    async (...args: unknown[]) => {
      const impl = answers.get(name);
      if (impl) return impl(...args);
      const data = defaultData(signature.data);
      return signature.result ? ok(data) : data;
    },
  ]),
) as unknown as Commands;

// ---- events -----------------------------------------------------------------

type Listener = (event: {
  event: string;
  id: number;
  payload: unknown;
}) => void;

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
) as unknown as typeof RealEvents;
