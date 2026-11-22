// DanyWhere: the app's zustand stores live as long as the test run; after
// every test they go back to how they started, so no test sees another's
// settings, notices or pending jump.
import { useSettingsStore } from "@/stores/settingsStore";
import { useModelStore } from "@/stores/modelStore";
import { useNoticeStore } from "@/stores/noticeStore";
import { useFieldKinds } from "@/components/settings/profiles/scope/fieldKinds";
import { useDecoupleBar } from "@/components/settings/profiles/scope/decoupleBarStore";
import { useSettingJump } from "@/components/settings/profiles/scope/settingJumpStore";

interface Resettable<S> {
  getInitialState: () => S;
  setState: (state: S, replace: true) => void;
}

const reset = <S,>(store: Resettable<S>) =>
  store.setState(store.getInitialState(), true);

export const resetStores = (): void => {
  reset(useSettingsStore);
  reset(useModelStore);
  reset(useNoticeStore);
  reset(useFieldKinds);
  reset(useDecoupleBar);
  reset(useSettingJump);
};
