// DanyWhere: how each value of a profile couples (backend
// `profile_layers::fields`, command `get_profile_fields`): as a whole, or
// entry by entry for lists and maps (P10). Loaded once; until then the
// backend's list fields as of strand E2 stand in.
import { create } from "zustand";
import { commands } from "@/bindings";
import type { FieldKind } from "@/lib/profileScope";

const VALUE: FieldKind = { kind: "value" };

/** `profile_layers::LIST_FIELDS` when this was written. */
const KNOWN_LISTS: Record<string, FieldKind> = {
  word_lists: { kind: "list", key: "id" },
  post_process_providers: { kind: "list", key: "id" },
  post_process_models: { kind: "map" },
  post_process_efforts: { kind: "map" },
  post_process_api_keys: { kind: "map" },
  bindings: { kind: "map" },
  start_keys: { kind: "list", key: "id" },
  hold_keys: { kind: "list", key: "id" },
  end_keys: { kind: "list", key: "id" },
};

interface FieldKinds {
  kinds: Record<string, FieldKind>;
  loaded: boolean;
  load: () => Promise<void>;
}

export const useFieldKinds = create<FieldKinds>((set, get) => ({
  kinds: KNOWN_LISTS,
  loaded: false,
  load: async () => {
    if (get().loaded) return;
    set({ loaded: true });
    try {
      const fields = await commands.getProfileFields();
      const kinds: Record<string, FieldKind> = {};
      for (const field of fields) kinds[field.name] = field.kind;
      set({ kinds });
    } catch (error) {
      console.error("Failed to load how profile values couple:", error);
    }
  },
}));

/** How `field` couples, from what is known now. */
export const kindOf = (field: string): FieldKind => {
  const store = useFieldKinds.getState();
  if (!store.loaded) void store.load();
  return store.kinds[field] ?? VALUE;
};
