import React, { useState, useRef, useCallback, useEffect } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RecordingProvider } from "./contexts/RecordingContext";
import MainPage from "./pages/MainPage";
import SettingsPage from "./pages/SettingsPage";
import { useTheme } from "./hooks/useTheme";
import "./styles/theme.css";

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
  const appRef = useRef<HTMLDivElement>(null);
  const wasExited = useRef(false);
  useTheme();

  // Page navigation with directional slide
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

  // Smooth window exit: fade-out + scale-down before minimize/hide
  const exitWindow = useCallback(async (action: () => Promise<void>) => {
    if (wasExited.current) return;
    const el = appRef.current;
    if (el) {
      el.style.transition = "opacity 120ms ease, transform 120ms ease";
      el.style.opacity = "0";
      el.style.transform = "scale(0.97)";
    }
    await new Promise(r => setTimeout(r, 130));
    wasExited.current = true;
    await action();
  }, []);

  // Restore window visuals when focus returns after exit
  useEffect(() => {
    const restore = () => {
      if (!wasExited.current) return;
      wasExited.current = false;
      const el = appRef.current;
      if (el) {
        // Ensure starting state before animating in
        el.style.transition = "none";
        el.style.opacity = "0";
        el.style.transform = "scale(0.97)";
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            el.style.transition = "opacity 150ms ease-out, transform 150ms ease-out";
            el.style.opacity = "1";
            el.style.transform = "scale(1)";
          });
        });
      }
    };

    const handleVisibility = () => {
      if (document.visibilityState === "visible") restore();
    };
    document.addEventListener("visibilitychange", handleVisibility);

    const appWindow = getCurrentWindow();
    const unlistenPromise = appWindow.onFocusChanged(({ payload: focused }) => {
      if (focused) restore();
    });

    return () => {
      document.removeEventListener("visibilitychange", handleVisibility);
      unlistenPromise.then(fn => fn());
    };
  }, []);

  return (
    <RecordingProvider>
      <div ref={appRef} style={{ height: "100%", width: "100%" }}>
        <div className={animClass} style={{ height: "100%", width: "100%" }}>
          {view === "main" ? (
            <MainPage onNavigate={navigateTo} onExitWindow={exitWindow} />
          ) : (
            <SettingsPage onNavigate={navigateTo} />
          )}
        </div>
      </div>
      <Toaster
        position="top-center"
        richColors
        toastOptions={{
          className: "font-sans",
          duration: 3000,
        }}
      />
    </RecordingProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
