// DanyWhere: the quiet bar after a coupled value was changed in a profile
// (P7.3): "“…” now applies only to “…”. [Change the Default Instead]
// [Undo]". It goes after a few seconds or with the next click.
import { create } from "zustand";

export interface DecoupledValue {
  profileId: string;
  field: string;
  /** The entry of a list field that became the profile's own. */
  entry?: string;
}

interface DecoupleBarState {
  bar: (DecoupledValue & { shownAt: number }) | null;
  show: (value: DecoupledValue) => void;
  hide: () => void;
}

/** How long the bar stays without a click. */
export const DECOUPLE_BAR_MS = 8000;

export const useDecoupleBar = create<DecoupleBarState>((set) => ({
  bar: null,
  show: (value) => set({ bar: { ...value, shownAt: Date.now() } }),
  hide: () => set({ bar: null }),
}));
