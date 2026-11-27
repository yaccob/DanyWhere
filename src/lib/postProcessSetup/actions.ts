// DanyWhere: what a failure's next step does when its button is clicked
// (`postProcessFailureAction` in lib/postProcessFailure.ts says which one):
// it opens the provider's page, the setup's questions on the page "Post
// Process", or a new sign-in.
import { openUrl } from "@tauri-apps/plugin-opener";
import type { NavLocation } from "../navigation";
import type { Translate } from "../displayNames";
import type { FailureAction } from "../postProcessFailure";
import { openLocation } from "@/stores/navigationRequestStore";
import {
  signInIntentOf,
  usePostProcessSetupStore,
} from "@/stores/postProcessSetupStore";
import { useSettingsStore } from "@/stores/settingsStore";

/** Where the setup is: the defaults' page "Post Process". */
export const POST_PROCESS_SETUP_PAGE: NavLocation = {
  kind: "defaults",
  page: "postprocessing",
};

const LABEL = "errors.postProcessFailure.actions";

/** The button's label of a next step. */
export const failureActionLabel = (
  t: Translate,
  action: FailureAction,
): string => {
  switch (action.kind) {
    case "set_up":
      return t(`${LABEL}.setUp`);
    case "open":
      return t(`${LABEL}.${action.label}`);
    case "change_setup":
      return t(`${LABEL}.changeSetup`);
    case "sign_in_again":
      return t(`${LABEL}.signInAgain`);
  }
};

/** Does what the next step's button says. */
export const runFailureAction = (action: FailureAction): void => {
  const setup = usePostProcessSetupStore.getState();
  switch (action.kind) {
    case "open":
      void openUrl(action.url);
      return;
    case "set_up":
    case "change_setup":
      setup.openQuestions();
      openLocation(POST_PROCESS_SETUP_PAGE);
      return;
    case "sign_in_again":
      // OpenRouter for the way the answers chose (free, or only with
      // credit), as kept in the settings before the setup's page has read
      // them; ChatGPT has no such ways.
      void setup.startSignIn(
        action.provider,
        action.provider === "openrouter"
          ? signInIntentOf(
              setup.answers ??
                useSettingsStore.getState().settings?.post_process_setup
                  ?.answers,
            )
          : null,
      );
      openLocation(POST_PROCESS_SETUP_PAGE);
      return;
  }
};
