import React, { useState, useRef, useCallback } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RecordingProvider } from "./contexts/RecordingContext";
import MainPage from "./pages/MainPage";
import SettingsPage from "./pages/SettingsPage";
import { useTheme } from "./hooks/useTheme";
import "./styles/theme.css";
import "./styles/pages.css";

type View = "main" | "settings";

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
          <p>应用遇到错误，请重启。</p>
          {this.state.errorMessage && (
            <p style={{ fontSize: 12, color: "var(--color-text-tertiary)", marginTop: 8 }}>
              {this.state.errorMessage}
            </p>
          )}
          <button onClick={() => this.setState({ hasError: false, errorMessage: null })} style={{ marginTop: 8, padding: "6px 16px", cursor: "pointer" }}>
            重试
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

function App() {
  const [view, setView] = useState<View>("main");
  const [animClass, setAnimClass] = useState("");
  const isTransitioning = useRef(false);
  useTheme();

  // Page navigation with directional slide — both pages stay mounted
  const navigateTo = useCallback((target: View) => {
    if (isTransitioning.current || target === view) return;
    isTransitioning.current = true;

    const exitClass = target === "settings" ? "page-exit-left" : "page-exit-right";
    const enterClass = target === "settings" ? "page-enter-right" : "page-enter-left";

    setAnimClass(exitClass);
    setTimeout(() => {
      setView(target);
      setAnimClass(enterClass);
      setTimeout(() => {
        setAnimClass("");
        isTransitioning.current = false;
      }, 150);
    }, 120);
  }, [view]);

  return (
    <RecordingProvider>
      <div style={{ height: "100%", width: "100%" }}>
        <div className={animClass} style={{ height: "100%", width: "100%" }}>
          <div style={{ height: "100%", display: view === "main" ? "contents" : "none" }}>
            <MainPage onNavigate={navigateTo} />
          </div>
          <div style={{ height: "100%", display: view === "settings" ? "contents" : "none" }}>
            <SettingsPage onNavigate={navigateTo} />
          </div>
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
      console.error("字幕窗口加载失败:", error);
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
