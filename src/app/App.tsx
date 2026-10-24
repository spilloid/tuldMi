import { Suspense, lazy, useEffect, useState } from "react";
import { createBackend } from "../lib/ipc";
import { useRoute } from "../lib/router";
import { AppStore } from "../state/store";
import { StoreProvider, useObservable, useSettings, useStore } from "../state/hooks";
import { UiProvider } from "../state/ui";
import { PlayerProvider } from "../state/player";
import { TooltipProvider } from "../components/ui/Tooltip";
import { ToastProvider } from "../components/ui/Toast";
import { ConfirmProvider } from "../components/ui/Confirm";
import { Titlebar } from "../components/shell/Titlebar";
import { Onboarding } from "../features/onboarding/Onboarding";
import { Shell } from "./Shell";
import { useTheme } from "./theme";

// DEV-only playback fixture (`?fixture`), excluded from production bundles.
const DevFixture = import.meta.env.DEV ? lazy(() => import("../dev/Fixture").then((m) => ({ default: m.Fixture }))) : null;

export function App() {
  if (DevFixture && new URLSearchParams(window.location.search).has("fixture")) {
    return (
      <Suspense fallback={null}>
        <DevFixture />
      </Suspense>
    );
  }
  return <MainApp />;
}

function MainApp() {
  const [store] = useState(() => new AppStore(createBackend()));
  useEffect(() => {
    void store.start();
    return () => store.stop();
  }, [store]);

  return (
    <StoreProvider store={store}>
      <TooltipProvider>
        <ToastProvider>
          <ConfirmProvider>
            <UiProvider>
              <PlayerProvider>
                <Root />
              </PlayerProvider>
            </UiProvider>
          </ConfirmProvider>
        </ToastProvider>
      </TooltipProvider>
    </StoreProvider>
  );
}

function Root() {
  const store = useStore();
  const { settings } = useSettings();
  const bootError = useObservable(store.bootError);
  const route = useRoute();
  useTheme(settings?.theme);
  const demo = store.backend.kind === "demo";

  if (bootError) {
    return (
      <div className="flex h-full flex-col">
        <Titlebar demo={demo} />
        <div className="flex flex-1 items-center justify-center p-8">
          <div className="max-w-[440px] text-center">
            <h1 className="font-display text-[26px] font-[460] text-text">tuldMi couldn't start its engine</h1>
            <p className="selectable mt-3 text-base text-muted">{bootError}</p>
            <p className="mt-4 text-sm text-faint">Your history is safe on disk. Restart tuldMi from the tray; if this keeps happening, the log in the data folder explains why.</p>
          </div>
        </div>
      </div>
    );
  }
  if (!settings) {
    return (
      <div className="flex h-full flex-col">
        <Titlebar demo={demo} />
        <div className="flex-1" aria-busy="true" />
      </div>
    );
  }
  if (!settings.onboardingComplete || route.view === "onboarding") {
    return <Onboarding step={route.view === "onboarding" ? route.step : 0} />;
  }
  return <Shell />;
}
