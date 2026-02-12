import { useState, useEffect, useCallback } from "react";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";

export type ThemeMode = "light" | "dark" | "system";

interface UseThemeReturn {
  theme: ThemeMode;
  isDark: boolean;
  setTheme: (mode: ThemeMode) => void;
  toggleTheme: () => void;
}

const STORAGE_KEY = "light-whisper-theme";

function getSystemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function resolveIsDark(mode: ThemeMode): boolean {
  if (mode === "system") return getSystemPrefersDark();
  return mode === "dark";
}

function applyThemeToDOM(isDark: boolean): void {
  const root = document.documentElement;

  // Suppress all transitions during theme switch to prevent color flash
  root.classList.add("no-transition");

  if (isDark) {
    root.classList.add("dark");
    root.setAttribute("data-theme", "dark");
  } else {
    root.classList.remove("dark");
    root.setAttribute("data-theme", "light");
  }

  // Force reflow, then re-enable transitions
  root.offsetHeight;
  root.classList.remove("no-transition");
}

/**
 * React hook for theme management.
 * Supports light, dark, and system-following modes.
 * Persists the user's choice to localStorage and applies the
 * corresponding class / data-attribute to <html>.
 */
export function useTheme(): UseThemeReturn {
  const [theme, setThemeState] = useState<ThemeMode>(() => {
    const stored = readLocalStorage(STORAGE_KEY);
    if (stored === "light" || stored === "dark" || stored === "system") {
      return stored;
    }
    return "system";
  });

  const [isDark, setIsDark] = useState(() => resolveIsDark(theme));

  // Apply theme whenever it changes
  useEffect(() => {
    const dark = resolveIsDark(theme);
    setIsDark(dark);
    applyThemeToDOM(dark);
    writeLocalStorage(STORAGE_KEY, theme);
  }, [theme]);

  // Listen for system preference changes when in "system" mode
  useEffect(() => {
    if (theme !== "system") return;

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    const handler = (e: MediaQueryListEvent) => {
      setIsDark(e.matches);
      applyThemeToDOM(e.matches);
    };

    mediaQuery.addEventListener("change", handler);
    return () => mediaQuery.removeEventListener("change", handler);
  }, [theme]);

  const setTheme = useCallback((mode: ThemeMode) => {
    setThemeState(mode);
  }, []);

  const toggleTheme = useCallback(() => {
    setThemeState((prev) => {
      if (prev === "light") return "dark";
      if (prev === "dark") return "system";
      return "light";
    });
  }, []);

  return { theme, isDark, setTheme, toggleTheme };
}
