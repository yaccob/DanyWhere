import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useState,
  useRef,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import { checkMicrophonePermission } from "tauri-plugin-macos-permissions-api";
import {
  choseToGoWithout,
  macosGrantsAccessibility,
} from "./lib/accessibility";
import "./App.css";
import SecureInputWarning from "./components/SecureInputWarning";
import Footer from "./components/footer";
import { OnboardingFlow } from "./components/onboarding/OnboardingFlow";
import ModelMigration from "./components/onboarding/ModelMigration";
import {
  ReplaceModel,
  ReturningInputMethodGuide,
  ReturningPermissions,
} from "./components/onboarding/StandaloneScreens";
import { inputMethodGuideDue, readInputMethodStatus } from "./lib/inputMethod";
import {
  clearOnboardingProgress,
  type OnboardingStep as OnboardingPreviewStep,
} from "./components/onboarding/onboardingStore";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { FileDropOverlay } from "./components/FileDropOverlay";
import { Sidebar } from "./components/Sidebar";
// DanyWhere: the settings in three areas, the page the navigation shows.
import { SettingsPage } from "./components/SettingsPage";
import {
  START_LOCATION,
  edgeToEdge,
  fillsWindow,
  locationKey,
  navContext,
  resolveLocation,
  sameLocation,
  type NavLocation,
} from "@/lib/navigation";
import { WhatsNewGate } from "./components/whats-new";
import { useSettings } from "./hooks/useSettings";
import { useSettingsStore } from "./stores/settingsStore";
import { useNavigationRequest } from "./stores/navigationRequestStore";
import { commands } from "@/bindings";
import { permissionsSatisfied } from "@/components/onboarding/permissionGate";
import { deleteRetiredModel } from "@/lib/models/retired";
import { NoticePanel } from "@/components/notices/NoticePanel";
import { getLanguageDirection, initializeRTL } from "@/lib/utils/rtl";

// flow is first-run onboarding; the rest are single screens for returning users.
type OnboardingStep =
  | "flow"
  | "permissions"
  | "migration"
  | "replaceModel"
  // DanyWhere: once, for an installation without Accessibility.
  | "inputMethodGuide"
  | "done";

function App() {
  const { t, i18n } = useTranslation();
  const [onboardingStep, setOnboardingStep] = useState<OnboardingStep | null>(
    null,
  );
  const [onboardingPreview, setOnboardingPreview] =
    useState<OnboardingPreviewStep | null>(null);
  // Set when a returning user's selected model is a retired (ONNX) model that
  // is still on disk: they must update it before the app can transcribe.
  const [migrationModelId, setMigrationModelId] = useState<string | null>(null);
  // DanyWhere: the one-time guide to the input method is due at this start
  // (lib/inputMethod.ts); shown after the permissions, if they need a screen.
  const [inputMethodGuide, setInputMethodGuide] = useState(false);
  // DanyWhere: where the navigation is (lib/navigation.ts).
  const [location, setLocation] = useState<NavLocation>(START_LOCATION);
  const showHistory = useCallback(() => setLocation({ kind: "history" }), []);
  const { settings, updateSetting } = useSettings();
  // A page or profile that went away (a profile switch hid Post Process,
  // debug mode ended, the profile was deleted) gives way to its area's first
  // page or the overview; the state follows, so it doesn't come back later.
  const shown = settings
    ? resolveLocation(location, navContext(settings))
    : location;
  const moved = !sameLocation(shown, location);
  useEffect(() => {
    if (moved) setLocation(shown);
  }, [moved, shown]);
  const shownKey = locationKey(shown);
  // DanyWhere: a place asked for from outside the settings page, e.g. a
  // notice's "Set up" (stores/navigationRequestStore.ts).
  const requested = useNavigationRequest((s) => s.requested);
  useEffect(() => {
    if (!requested) return;
    setLocation(requested);
    useNavigationRequest.getState().done();
  }, [requested]);
  const direction = getLanguageDirection(i18n.language);
  const refreshAudioDevices = useSettingsStore(
    (state) => state.refreshAudioDevices,
  );
  const refreshOutputDevices = useSettingsStore(
    (state) => state.refreshOutputDevices,
  );
  const hasCompletedPostOnboardingInit = useRef(false);
  const settingsScrollRef = useRef<HTMLDivElement>(null);
  // Reset the scroll position whenever the active section changes.
  useLayoutEffect(() => {
    settingsScrollRef.current?.scrollTo({ top: 0 });
  }, [shownKey]);

  useEffect(() => {
    checkOnboardingStatus();
  }, []);

  // Initialize RTL direction when language changes
  useEffect(() => {
    initializeRTL(i18n.language);
  }, [i18n.language]);

  // Initialize Enigo, shortcuts, and refresh audio devices when main app loads
  useEffect(() => {
    if (onboardingStep === "done" && !hasCompletedPostOnboardingInit.current) {
      hasCompletedPostOnboardingInit.current = true;
      Promise.all([
        commands.initializeEnigo(),
        commands.initializeShortcuts(),
      ]).catch((e) => {
        console.warn("Failed to initialize:", e);
      });
      refreshAudioDevices();
      refreshOutputDevices();
    }
  }, [onboardingStep, refreshAudioDevices, refreshOutputDevices]);

  // Handle keyboard shortcuts for debug mode toggle
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Check for Ctrl+Shift+D (Windows/Linux) or Cmd+Shift+D (macOS)
      const isDebugShortcut =
        event.shiftKey &&
        event.key.toLowerCase() === "d" &&
        (event.ctrlKey || event.metaKey);

      if (isDebugShortcut) {
        event.preventDefault();
        const currentDebugMode = settings?.debug_mode ?? false;
        updateSetting("debug_mode", !currentDebugMode);
      }
    };

    // Add event listener when component mounts
    document.addEventListener("keydown", handleKeyDown);

    // Cleanup event listener when component unmounts
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [settings?.debug_mode, updateSetting]);

  const revealMainWindowForPermissions = async () => {
    try {
      await commands.showMainWindowCommand();
    } catch (e) {
      console.warn("Failed to show main window for permission onboarding:", e);
    }
  };

  const getMigrationModelId = async (
    selectedModel: string,
  ): Promise<string | null> => {
    const retired = await commands.getRetiredModels();
    return retired.some((m) => m.id === selectedModel) ? selectedModel : null;
  };

  // Returning users land in the app unless their model needs updating, or
  // (DanyWhere) the input method's guide is due. The window may start
  // hidden, so bring it forward for either screen. A model update comes
  // first; the guide then waits for the next start.
  const finishReturningUser = async (
    retiredModelId: string | null,
    guideDue: boolean,
  ) => {
    if (retiredModelId) {
      await revealMainWindowForPermissions();
      setOnboardingStep("migration");
    } else if (guideDue) {
      await revealMainWindowForPermissions();
      setOnboardingStep("inputMethodGuide");
    } else {
      setOnboardingStep("done");
    }
  };

  const checkOnboardingStatus = async () => {
    try {
      const settingsResult = await commands.getAppSettings();
      const hasCompletedOnboarding =
        settingsResult.status === "ok" &&
        settingsResult.data.onboarding_completed === true;
      const accessibilitySkipped = choseToGoWithout(
        settingsResult.status === "ok" ? settingsResult.data : null,
      );
      const currentPlatform = platform();

      if (hasCompletedOnboarding) {
        // Returning user - check if they need to grant permissions first
        const retiredModelId = await getMigrationModelId(
          settingsResult.data.selected_model ?? "",
        );
        setMigrationModelId(retiredModelId);
        const guideDue =
          currentPlatform === "macos" &&
          inputMethodGuideDue(
            currentPlatform,
            settingsResult.data,
            await readInputMethodStatus(),
          );
        setInputMethodGuide(guideDue);

        if (currentPlatform === "macos") {
          try {
            const [hasAccessibility, hasMicrophone] = await Promise.all([
              macosGrantsAccessibility(),
              checkMicrophonePermission(),
            ]);
            const satisfied = permissionsSatisfied(
              "macos",
              { accessibility: hasAccessibility, microphone: hasMicrophone },
              accessibilitySkipped,
            );
            if (!satisfied) {
              await revealMainWindowForPermissions();
              setOnboardingStep("permissions");
              return;
            }
          } catch (e) {
            console.warn("Failed to check macOS permissions:", e);
            // If we can't check, proceed to main app and let them fix it there
          }
        }

        if (currentPlatform === "windows") {
          try {
            const microphoneStatus =
              await commands.getWindowsMicrophonePermissionStatus();
            if (
              microphoneStatus.supported &&
              microphoneStatus.overall_access === "denied"
            ) {
              await revealMainWindowForPermissions();
              setOnboardingStep("permissions");
              return;
            }
          } catch (e) {
            console.warn("Failed to check Windows microphone permissions:", e);
            // If we can't check, proceed to main app and let them fix it there
          }
        }

        await finishReturningUser(retiredModelId, guideDue);
      } else {
        // New user - start full onboarding
        setOnboardingStep("flow");
      }
    } catch (error) {
      console.error("Failed to check onboarding status:", error);
      setOnboardingStep("flow");
    }
  };

  // Debug: the real onboarding from Welcome, for testing it end to end.
  const runOnboarding = () => {
    clearOnboardingProgress();
    setOnboardingStep("flow");
  };

  // Debug: the real model migration screen. It leads with the selected model
  // if that's retired, otherwise with the first retired model on disk.
  const runModelMigration = async () => {
    const retired = await commands.getRetiredModels();
    if (retired.length === 0) {
      console.info("Model migration: no old-format models on disk");
      return;
    }
    const selected = settings?.selected_model;
    setMigrationModelId(
      retired.find((m) => m.id === selected)?.id ?? retired[0].id,
    );
    setOnboardingStep("migration");
  };

  const handleModelSelected = async () => {
    // Reached when the user picks a different model instead of updating.
    for (const model of await commands.getRetiredModels()) {
      await deleteRetiredModel(model.id);
    }
    setOnboardingStep("done");
  };

  // Still checking onboarding status
  if (onboardingStep === null) {
    return null;
  }

  // Select the content for the current step. NoticePanel is rendered once,
  // beside this node, so errors surface on every step, including onboarding.
  let content: ReactNode;
  if (onboardingPreview) {
    // Previews render in the same top-level slot as real onboarding, inert:
    // no prompts, downloads or setting writes.
    content = (
      <>
        <OnboardingFlow
          preview
          previewStep={onboardingPreview}
          onComplete={() => setOnboardingPreview(null)}
        />
        <button
          type="button"
          onClick={() => setOnboardingPreview(null)}
          className="fixed top-4 end-4 z-50 rounded-lg border border-line bg-background px-4 py-2 text-sm font-medium text-text shadow-lg hover:bg-logo-primary/20 cursor-pointer"
        >
          {t("settings.debug.onboardingPreview.exitButton")}
        </button>
      </>
    );
  } else if (onboardingStep === "flow") {
    content = <OnboardingFlow onComplete={() => setOnboardingStep("done")} />;
  } else if (onboardingStep === "permissions") {
    content = (
      <ReturningPermissions
        onComplete={() =>
          finishReturningUser(migrationModelId, inputMethodGuide)
        }
      />
    );
  } else if (onboardingStep === "inputMethodGuide") {
    content = (
      <ReturningInputMethodGuide onComplete={() => setOnboardingStep("done")} />
    );
  } else if (onboardingStep === "migration" && migrationModelId) {
    content = (
      <ModelMigration
        inUseModelId={migrationModelId}
        onComplete={() => setOnboardingStep("done")}
        onChooseAnother={() => setOnboardingStep("replaceModel")}
      />
    );
  } else if (onboardingStep === "replaceModel") {
    content = (
      <ReplaceModel
        onBack={() => setOnboardingStep("migration")}
        onComplete={handleModelSelected}
      />
    );
  } else {
    content = (
      <div
        dir={direction}
        className="h-screen flex flex-col select-none cursor-default"
      >
        <ErrorBoundary context="What's New">
          <WhatsNewGate />
        </ErrorBoundary>
        {/* Main content area that takes remaining space */}
        <div className="flex-1 flex overflow-hidden">
          <Sidebar location={shown} onNavigate={setLocation} />
          {/* Scrollable content area */}
          <div className="flex-1 flex flex-col overflow-hidden">
            <div ref={settingsScrollRef} className="flex-1 overflow-y-auto">
              {/* Models fills the window: its table scrolls, not the page.
                  The manual fills it edge to edge: its frame scrolls. */}
              <div
                className={`flex flex-col items-center ${edgeToEdge(shown) ? "" : "p-4 gap-4"} ${fillsWindow(shown) ? "h-full" : ""}`}
              >
                <SecureInputWarning />
                <SettingsPage
                  location={shown}
                  onNavigate={setLocation}
                  onPreviewOnboarding={setOnboardingPreview}
                  onRunOnboarding={runOnboarding}
                  onRunModelMigration={runModelMigration}
                />
              </div>
            </div>
          </div>
        </div>
        {/* Fixed footer at bottom */}
        <Footer onShowHistory={showHistory} />
        <FileDropOverlay onDropped={showHistory} />
      </div>
    );
  }

  return (
    <>
      {content}
      <NoticePanel />
    </>
  );
}

export default App;
