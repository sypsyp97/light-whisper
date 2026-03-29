import { useState, useCallback, useEffect, useRef } from "react";

const POPOVER_EXIT_MS = 160;

/**
 * Manages a group of mutually-exclusive dropdown pickers.
 *
 * At most one picker is open at a time. Clicking outside the active picker's
 * ref container or pressing Escape closes it automatically.
 *
 * Closing plays a brief exit animation before the popover is removed from DOM.
 */
export function useExclusivePicker<T extends string>() {
  const [active, setActive] = useState<T | null>(null);
  const [closing, setClosing] = useState<T | null>(null);
  const refs = useRef(new Map<T, HTMLDivElement | null>());
  const closingTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  // Popover stays rendered during both "active" and "closing" phases
  const isOpen = useCallback(
    (id: T) => active === id || closing === id,
    [active, closing],
  );

  const toggle = useCallback((id: T) => {
    setActive((prev) => {
      if (prev === id) {
        // Close with exit animation
        setClosing(id);
        clearTimeout(closingTimer.current);
        closingTimer.current = setTimeout(() => setClosing(null), POPOVER_EXIT_MS);
        return null;
      }
      // Opening a new picker — cancel any pending exit
      clearTimeout(closingTimer.current);
      setClosing(null);
      return id;
    });
  }, []);

  const close = useCallback(() => {
    setActive((prev) => {
      if (prev) {
        setClosing(prev);
        clearTimeout(closingTimer.current);
        closingTimer.current = setTimeout(() => setClosing(null), POPOVER_EXIT_MS);
      }
      return null;
    });
  }, []);

  /** Returns the correct class name for the popover (entrance or exit). */
  const popoverClass = useCallback(
    (id: T) =>
      closing === id
        ? "picker-popover picker-popover-exit"
        : "picker-popover",
    [closing],
  );

  const setRef = useCallback(
    (id: T) => (el: HTMLDivElement | null) => { refs.current.set(id, el); },
    [],
  );

  useEffect(() => {
    if (!active) return;
    const onPointerDown = (e: MouseEvent) => {
      const ref = refs.current.get(active);
      if (ref && !ref.contains(e.target as Node)) {
        // Close via animated path
        setActive(null);
        setClosing(active);
        clearTimeout(closingTimer.current);
        closingTimer.current = setTimeout(() => setClosing(null), POPOVER_EXIT_MS);
      }
    };
    const onEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setActive(null);
        setClosing(active);
        clearTimeout(closingTimer.current);
        closingTimer.current = setTimeout(() => setClosing(null), POPOVER_EXIT_MS);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onEscape);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onEscape);
    };
  }, [active]);

  // Cleanup timer on unmount
  useEffect(() => () => clearTimeout(closingTimer.current), []);

  return { active, isOpen, toggle, close, setRef, popoverClass };
}
