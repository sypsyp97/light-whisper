import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "sonner";
import { RecordingProvider } from "./contexts/RecordingContext";
import MainPage from "./pages/MainPage";
import SettingsPage from "./pages/SettingsPage";
import { useTheme } from "./hooks/useTheme";
import "./styles/theme.css";

type View = "main" | "settings";

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { hasError: boolean }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { hasError: false };
  }
  static getDerivedStateFromError() {
    return { hasError: true };
  }
  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("Uncaught render error:", error, info);
  }
  render() {
    if (this.state.hasError) {
      return (
        <div style={{ padding: 24, textAlign: "center", color: "#888" }}>
          <p>应用遇到错误，请重启。</p>
          <button onClick={() => this.setState({ hasError: false })} style={{ marginTop: 8, padding: "6px 16px", cursor: "pointer" }}>
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
  useTheme();

  return (
    <RecordingProvider>
      {view === "main" ? (
        <MainPage onNavigate={setView} />
      ) : (
        <SettingsPage onNavigate={setView} />
      )}
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
