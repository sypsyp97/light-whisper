import React, { useState, useRef, useCallback, useEffect } from "react";
import { flushSync } from "react-dom";
import ReactDOM from "react-dom/client";
import { toast, Toaster } from "sonner";
import { CircleCheck, CircleAlert, Info, TriangleAlert, LoaderCircle } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RecordingProvider } from "./contexts/RecordingContext";
import { prefersReducedMotion } from "./lib/motion";
import MainPage from "./pages/MainPage";
import { useTheme } from "./hooks/useTheme";
import { useProfileSaveNotifications } from "./hooks/useProfileSaveNotifications";
import i18n from "./i18n";
import "./styles/theme.css";
import "./styles/pages.css";

type View = "main" | "settings" | "history";
const loadSettingsPage = () => import("./pages/SettingsPage");
const loadHistoryPage = () => import("./pages/HistoryPage");

interface PageProps {
  onNavigate: (target: View) => void;
  animClass: string;
  pendingView: View | null;
  active: boolean;
}

interface Route {
  view: View;
  Page: React.ComponentType<PageProps>;
  animClass: string;
}

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
  const [{ view, Page, animClass }, setRoute] = useState<Route>({ view: "main", Page: MainPage, animClass: "" });
  const [pendingView, setPendingView] = useState<View | null>(null);
  const pendingNavigation = useRef<{ target: View } | null>(null);
  useTheme();
  useProfileSaveNotifications();

  useEffect(() => {
    const preloadTimer = window.setTimeout(() => {
      void loadSettingsPage().catch((error) => {
        console.error("Settings page preload failed:", error);
      });
      void loadHistoryPage().catch((error) => {
        console.error("History page preload failed:", error);
      });
    }, 400);

    return () => {
      window.clearTimeout(preloadTimer);
      pendingNavigation.current = null;
    };
  }, []);

  const navigateTo = useCallback((target: View) => {
    if (pendingNavigation.current?.target === target) return;
    const request = { target };
    pendingNavigation.current = null;
    if (target === view) {
      setPendingView(null);
      return;
    }

    const showPage = (NextPage: Route["Page"]) => {
      const entryClass = target === "main" ? "page-enter-left" : "page-enter-right";
      setRoute({ view: target, Page: NextPage, animClass: prefersReducedMotion() ? "" : entryClass });
    };

    if (target === "main") {
      setPendingView(null);
      showPage(MainPage);
      return;
    }

    pendingNavigation.current = request;
    setPendingView(target);
    const loadTarget = target === "settings" ? loadSettingsPage : loadHistoryPage;
    void loadTarget().then(({ default: NextPage }) => {
      if (pendingNavigation.current !== request) return;
      pendingNavigation.current = null;
      // Retire the old page before it can accept another navigation click.
      flushSync(() => {
        setPendingView(null);
        showPage(NextPage);
      });
    }, (error) => {
      if (pendingNavigation.current !== request) return;
      pendingNavigation.current = null;
      setPendingView(null);
      console.error(`${target} page load failed:`, error);
      toast.error(i18n.t("app.pageLoadFailed"));
    });
  }, [view]);

  return (
    <RecordingProvider>
      <Page onNavigate={navigateTo} active animClass={animClass} pendingView={pendingView} />
      <Toaster
        className="app-toaster"
        position="bottom-right"
        offset={14}
        mobileOffset={14}
        icons={{
          success: <CircleCheck size={18} aria-hidden="true" />,
          error: <CircleAlert size={18} aria-hidden="true" />,
          info: <Info size={18} aria-hidden="true" />,
          warning: <TriangleAlert size={18} aria-hidden="true" />,
          loading: <LoaderCircle size={18} className="animate-spin" aria-hidden="true" />,
        }}
        toastOptions={{
          className: "app-toast",
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
