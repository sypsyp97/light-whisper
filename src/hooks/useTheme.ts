import { useState, useEffect, useCallback } from "react";

export type ThemeMode = "light" | "dark" | "system";

interface UseThemeReturn {
  theme: ThemeMode;
  isDark: boolean;
  setTheme: (mode: ThemeMode) => void;
  toggleTheme: () => void;
}

const STORAGE_KEY = "ququ-theme";

function getSystemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function resolveIsDark(mode: ThemeMode): boolean {
  if (mode === "system") return getSystemPrefersDark();
  return mode === "dark";
}

function applyThemeToDOM(isDark: boolean): void {
  const root = document.documentElement;
  if (isDark) {
    root.classList.add("dark");
    root.setAttribute("data-theme", "dark");
  } else {
    root.classList.remove("dark");
    root.setAttribute("data-theme", "light");
  }
}

/**
 * React hook for theme management.
 * Supports light, dark, and system-following modes.
 * Persists the user's choice to localStorage and applies the
 * corresponding class / data-attribute to <html>.
 */
export function useTheme(): UseThemeReturn {
  const [theme, setThemeState] = useState<ThemeMode>(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored === "light" || stored === "dark" || stored === "system") {
        return stored;
      }
    } catch {
      // localStorage may be unavailable
    }
    return "system";
  });

  const [isDark, setIsDark] = useState(() => resolveIsDark(theme));

  // Apply theme whenever it changes
  useEffect(() => {
    const dark = resolveIsDark(theme);
    setIsDark(dark);
    applyThemeToDOM(dark);

    try {
      localStorage.setItem(STORAGE_KEY, theme);
    } catch {
      // ignore write failures
    }
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
