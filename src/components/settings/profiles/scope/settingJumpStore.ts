// DanyWhere: the jump from a profile's page "Profile" to one of its own
// values that waits for its setting to show (qa 2026-11-22_1603, B5). The
// rules are in `lib/settingJump.ts`.
import { create } from "zustand";
import {
  takesJump,
  type JumpTarget,
  type PendingJump,
} from "@/lib/settingJump";

interface SettingJumpState {
  jump: PendingJump | null;
  /** Asks for a jump; it replaces one still waiting. */
  request: (target: JumpTarget) => void;
  /** Takes the waiting jump for the wrapper of `fields` (`entry`) in
   * `profileId`: true once, for the first such wrapper; false when there is
   * none, it went elsewhere, it is over or another wrapper took it. */
  claim: (
    profileId: string,
    fields: readonly string[],
    entry?: string,
  ) => boolean;
}

export const useSettingJump = create<SettingJumpState>((set, get) => ({
  jump: null,
  request: (target) => set({ jump: { ...target, requestedAt: Date.now() } }),
  claim: (profileId, fields, entry) => {
    if (!takesJump(get().jump, Date.now(), profileId, fields, entry)) {
      return false;
    }
    set({ jump: null });
    return true;
  },
}));
