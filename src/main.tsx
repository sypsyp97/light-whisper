import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "sonner";
import { RecordingProvider } from "./contexts/RecordingContext";
import MainPage from "./pages/MainPage";
import SettingsPage from "./pages/SettingsPage";
import { useTheme } from "./hooks/useTheme";
import "./styles/theme.css";

type View = "main" | "settings";

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
    <App />
  </React.StrictMode>
);
