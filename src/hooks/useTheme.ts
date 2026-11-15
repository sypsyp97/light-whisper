import { useState, useEffect, useCallback, useRef } from "react";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import { THEME_STORAGE_KEY } from "@/lib/constants";
import { prefersReducedMotion } from "@/lib/motion";

export type ThemeMode = "light" | "dark" | "system";
const THEME_CHANGE = "light-whisper:theme-change";

let mountedConsumers = 0;
let sharedTheme: ThemeMode | undefined;

function themeMode(value: string | null): ThemeMode {
  return value === "light" || value === "dark" ? value : "system";
}

interface UseThemeReturn {
  theme: ThemeMode;
  isDark: boolean;
  setTheme: (mode: ThemeMode) => void;
}

function getSystemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function resolveIsDark(mode: ThemeMode): boolean {
  if (mode === "system") return getSystemPrefersDark();
  return mode === "dark";
}

let isFirstApply = true;
let activeTransition: ViewTransition | undefined;
let themeRevision = 0;

function applyThemeToDOM(isDark: boolean): void {
  const root = document.documentElement;

  const revision = ++themeRevision;
  activeTransition?.skipTransition();
  const nextTheme = isDark ? "dark" : "light";
  if (root.dataset.theme === nextTheme) return;
  const apply = () => {

    if (revision !== themeRevision) return;
    root.classList.add("no-transition");
    root.classList.toggle("dark", isDark);
    root.dataset.theme = nextTheme;
    void root.offsetHeight;
    root.classList.remove("no-transition");
  };
  if (isFirstApply || prefersReducedMotion() || !document.startViewTransition) {
    isFirstApply = false;
    apply();
  } else {
    activeTransition = document.startViewTransition(apply);
    void activeTransition.finished.catch(() => undefined);
  }
}







export function useTheme(): UseThemeReturn {
  const [theme, setThemeState] = useState<ThemeMode>(() => {
    return sharedTheme ?? themeMode(readLocalStorage(THEME_STORAGE_KEY));
  });
  const themeRef = useRef(theme);
  themeRef.current = theme;

  const [isDark, setIsDark] = useState(() => resolveIsDark(theme));


  useEffect(() => {
    const dark = resolveIsDark(theme);
    setIsDark(dark);
    applyThemeToDOM(dark);
  }, [theme]);

  useEffect(() => {
    mountedConsumers += 1;
    sharedTheme = themeRef.current;
    const update = (mode: ThemeMode) => {
      sharedTheme = mode;
      themeRef.current = mode;
      setThemeState(mode);
    };
    const localChange = (event: Event) => update((event as CustomEvent<ThemeMode>).detail);
    const storageChange = (event: StorageEvent) => {
      if (event.storageArea !== localStorage) return;
      if (event.key === THEME_STORAGE_KEY || event.key === null) {

        update(themeMode(readLocalStorage(THEME_STORAGE_KEY)));
      }
    };
    window.addEventListener(THEME_CHANGE, localChange);
    window.addEventListener("storage", storageChange);
    return () => {
      window.removeEventListener(THEME_CHANGE, localChange);
      window.removeEventListener("storage", storageChange);
      if (--mountedConsumers === 0) sharedTheme = undefined;
    };
  }, []);


  useEffect(() => {
    if (theme !== "system") return;

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    const handler = (e: MediaQueryListEvent) => {
      if (themeRef.current !== "system") return;
      setIsDark(e.matches);
      applyThemeToDOM(e.matches);
    };

    mediaQuery.addEventListener("change", handler);
    return () => mediaQuery.removeEventListener("change", handler);
  }, [theme]);

  const setTheme = useCallback((mode: ThemeMode) => {
    writeLocalStorage(THEME_STORAGE_KEY, mode);
    window.dispatchEvent(new CustomEvent(THEME_CHANGE, { detail: mode }));
  }, []);

  return { theme, isDark, setTheme };
}
