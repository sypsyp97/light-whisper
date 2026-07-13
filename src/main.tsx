import React, { Suspense, lazy, useState, useRef, useCallback, useEffect } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RecordingProvider } from "./contexts/RecordingContext";
import { prefersReducedMotion } from "./lib/motion";
import MainPage from "./pages/MainPage";
import { useTheme } from "./hooks/useTheme";
import i18n from "./i18n";
import "./styles/theme.css";
import "./styles/pages.css";

type View = "main" | "settings" | "history";
const loadSettingsPage = () => import("./pages/SettingsPage");
const loadHistoryPage = () => import("./pages/HistoryPage");
const SettingsPage = lazy(loadSettingsPage);
const HistoryPage = lazy(loadHistoryPage);

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { hasError: boolean; errorMessage: string | null }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { hasError: false, errorMessage: null };
  }
  static getDerivedStateFromError(error: Error) {
    return { hasError: true, errorMessage: error?.message || null };
  }
  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("Uncaught render error:", error, info);
  }
  render() {
    if (this.state.hasError) {
      return (
        <div style={{ padding: 24, textAlign: "center", color: "var(--color-text-secondary)" }}>
          <p>{i18n.t("app.errorRestart")}</p>
          {this.state.errorMessage && (
            <p style={{ fontSize: 12, color: "var(--color-text-tertiary)", marginTop: 8 }}>
              {this.state.errorMessage}
            </p>
          )}
          <button onClick={() => this.setState({ hasError: false, errorMessage: null })} style={{ marginTop: 8, padding: "6px 16px", cursor: "pointer" }}>
            {i18n.t("common.retry")}
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

export function App() {
  const [view, setView] = useState<View>("main");
  const [animClass, setAnimClass] = useState("");
  const isTransitioning = useRef(false);
  useTheme();

  useEffect(() => {
    const preloadTimer = window.setTimeout(() => {
      void loadSettingsPage().catch((error) => {
        console.error("Settings page preload failed:", error);
      });
      void loadHistoryPage().catch((error) => {
        console.error("History page preload failed:", error);
      });
    }, 400);

    return () => window.clearTimeout(preloadTimer);
  }, []);

  // Page navigation with directional slide while only mounting the active page
  const navigateTo = useCallback((target: View) => {
    if (isTransitioning.current || target === view) return;
    isTransitioning.current = true;

    const startTransition = () => {
      if (prefersReducedMotion()) {
        setAnimClass("");
        setView(target);
        isTransitioning.current = false;
        return;
      }

      const movingAwayFromMain = view === "main" && target !== "main";
      const exitClass = movingAwayFromMain ? "page-exit-left" : "page-exit-right";
      const enterClass = movingAwayFromMain ? "page-enter-right" : "page-enter-left";

      setAnimClass(exitClass);
      setTimeout(() => {
        setView(target);
        setAnimClass(enterClass);
        setTimeout(() => {
          setAnimClass("");
          isTransitioning.current = false;
        }, 180);
      }, 140);
    };

    if (target !== "main") {
      const loadTarget = target === "settings" ? loadSettingsPage : loadHistoryPage;
      void loadTarget().then(startTransition).catch((error) => {
        isTransitioning.current = false;
        console.error(`${target} page load failed:`, error);
      });
      return;
    }

    startTransition();
  }, [view]);

  return (
    <RecordingProvider>
      <div style={{ height: "100%", width: "100%" }}>
        <div style={{ height: "100%", width: "100%" }}>
          <Suspense fallback={<div style={{ height: "100%", width: "100%" }} />}>
            {view === "main" ? (
              <MainPage onNavigate={navigateTo} animClass={animClass} />
            ) : view === "settings" ? (
              <SettingsPage onNavigate={navigateTo} active animClass={animClass} />
            ) : (
              <HistoryPage onNavigate={navigateTo} animClass={animClass} />
            )}
          </Suspense>
        </div>
      </div>
      <Toaster
        position="bottom-right"
        offset={14}
        richColors
        toastOptions={{
          className: "font-sans",
          duration: 1800,
        }}
      />
    </RecordingProvider>
  );
}

const windowLabel = getCurrentWindow().label;

if (windowLabel === "subtitle") {
  import("./pages/SubtitleOverlay")
    .then(({ default: SubtitleOverlay }) => {
      ReactDOM.createRoot(document.getElementById("root")!).render(
        <React.StrictMode>
          <SubtitleOverlay />
        </React.StrictMode>
      );
    })
    .catch((error) => {
      console.error("Subtitle window load failed:", error);
    });
} else if (windowLabel === "selection-toolbar") {
  import("./pages/SelectionOverlay")
    .then(({ default: SelectionOverlay }) => {
      ReactDOM.createRoot(document.getElementById("root")!).render(
        <React.StrictMode>
          <SelectionOverlay />
        </React.StrictMode>
      );
    })
    .catch((error) => {
      console.error("Selection window load failed:", error);
    });
} else {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </React.StrictMode>
  );
}
