// DanyWhere: a place of the settings window asked for from outside the
// settings page, e.g. by a notice's action ("Set up" opens the setup of
// post-processing). The app shows it and takes the request back.
import { create } from "zustand";
import type { NavLocation } from "@/lib/navigation";

interface NavigationRequest {
  /** The place asked for, until the app shows it. */
  requested: NavLocation | null;
  open: (location: NavLocation) => void;
  /** The app showed it. */
  done: () => void;
}

export const useNavigationRequest = create<NavigationRequest>()((set) => ({
  requested: null,
  open: (location) => set({ requested: location }),
  done: () => set({ requested: null }),
}));

/** Asks the settings window to show `location`. */
export const openLocation = (location: NavLocation) =>
  useNavigationRequest.getState().open(location);
