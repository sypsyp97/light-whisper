import { useState, useCallback, useEffect, useRef } from "react";

/**
 * Manages a group of mutually-exclusive dropdown pickers.
 *
 * At most one picker is open at a time. Clicking outside the active picker's
 * ref container or pressing Escape closes it automatically.
 */
export function useExclusivePicker<T extends string>() {
  const [active, setActive] = useState<T | null>(null);
  const refs = useRef(new Map<T, HTMLDivElement | null>());

  const isOpen = useCallback((id: T) => active === id, [active]);

  const toggle = useCallback((id: T) => {
    setActive((prev) => (prev === id ? null : id));
  }, []);

  const close = useCallback(() => setActive(null), []);

  const setRef = useCallback(
    (id: T) => (el: HTMLDivElement | null) => { refs.current.set(id, el); },
    [],
  );

  useEffect(() => {
    if (!active) return;
    const onPointerDown = (e: MouseEvent) => {
      const ref = refs.current.get(active);
      if (ref && !ref.contains(e.target as Node)) setActive(null);
    };
    const onEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") setActive(null);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onEscape);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onEscape);
    };
  }, [active]);

  return { active, isOpen, toggle, close, setRef };
}
