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
  const typeaheadTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const typeaheadBuffer = useRef("");

  // Popover stays rendered during both "active" and "closing" phases
  const isOpen = useCallback(
    (id: T) => active === id || closing === id,
    [active, closing],
  );
  const isExpanded = useCallback((id: T) => active === id, [active]);

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
    document.addEventListener("mousedown", onPointerDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
    };
  }, [active]);

  useEffect(() => {
    if (!active) return;
    const container = refs.current.get(active);
    const trigger = container?.querySelector<HTMLElement>('[aria-haspopup="listbox"]');
    const listbox = container?.querySelector<HTMLElement>('[role="listbox"]');
    if (!container || !trigger || !listbox) return;

    const listboxId = `picker-${String(active)}-listbox`;
    listbox.id = listboxId;
    trigger.setAttribute("aria-controls", listboxId);
    if (!listbox.hasAttribute("aria-label")) {
      const triggerLabel = trigger.getAttribute("aria-label");
      if (triggerLabel) listbox.setAttribute("aria-label", triggerLabel);
    }

    const options = Array.from(
      listbox.querySelectorAll<HTMLButtonElement>("button.picker-option:not(:disabled)"),
    );
    if (options.length === 0) return;

    const popover = listbox.closest<HTMLElement>(".picker-popover");
    if (popover) {
      const containerRect = container.getBoundingClientRect();
      const scrollBoundary = container.closest<HTMLElement>(".settings-content");
      const boundaryRect = scrollBoundary?.getBoundingClientRect();
      const boundaryTop = boundaryRect?.top ?? 0;
      const boundaryBottom = boundaryRect?.bottom ?? window.innerHeight;
      const availableBelow = boundaryBottom - containerRect.bottom;
      const availableAbove = containerRect.top - boundaryTop;
      const desiredHeight = Math.min(popover.scrollHeight || 280, 280);
      popover.dataset.placement = availableBelow < desiredHeight && availableAbove > availableBelow
        ? "top"
        : "bottom";
    }

    options.forEach((option, index) => {
      option.id = `${listboxId}-option-${index}`;
      option.setAttribute("role", "option");
      option.setAttribute("aria-selected", String(option.dataset.active === "true"));
      option.tabIndex = option.dataset.active === "true" ? 0 : -1;
    });
    if (!options.some((option) => option.tabIndex === 0)) options[0].tabIndex = 0;

    const focusOption = (index: number) => {
      const normalized = (index + options.length) % options.length;
      options.forEach((option, optionIndex) => {
        option.tabIndex = optionIndex === normalized ? 0 : -1;
      });
      options[normalized].focus();
    };

    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      const currentIndex = options.findIndex((option) => option === document.activeElement);
      if (event.key === "Escape") {
        event.preventDefault();
        close();
        window.requestAnimationFrame(() => trigger.focus());
        return;
      }
      if (event.key === "ArrowDown") {
        event.preventDefault();
        focusOption(currentIndex < 0 ? 0 : currentIndex + 1);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        focusOption(currentIndex < 0 ? options.length - 1 : currentIndex - 1);
        return;
      }
      if (event.key === "Home") {
        event.preventDefault();
        focusOption(0);
        return;
      }
      if (event.key === "End") {
        event.preventDefault();
        focusOption(options.length - 1);
        return;
      }
      if (
        event.key.length === 1
        && !event.altKey
        && !event.ctrlKey
        && !event.metaKey
        && !(event.target instanceof HTMLInputElement)
        && !(event.target instanceof HTMLTextAreaElement)
      ) {
        typeaheadBuffer.current += event.key.toLocaleLowerCase();
        clearTimeout(typeaheadTimer.current);
        typeaheadTimer.current = setTimeout(() => {
          typeaheadBuffer.current = "";
        }, 500);
        const matchIndex = options.findIndex((option) =>
          option.textContent?.trim().toLocaleLowerCase().startsWith(typeaheadBuffer.current),
        );
        if (matchIndex >= 0) {
          event.preventDefault();
          focusOption(matchIndex);
        }
      }
    };

    container.addEventListener("keydown", onKeyDown);
    if (!container.querySelector("input, textarea")) {
      window.requestAnimationFrame(() => {
        const selectedIndex = options.findIndex((option) => option.dataset.active === "true");
        focusOption(selectedIndex >= 0 ? selectedIndex : 0);
      });
    }

    return () => {
      container.removeEventListener("keydown", onKeyDown);
      trigger.removeAttribute("aria-controls");
    };
  }, [active, close]);

  // Cleanup timer on unmount
  useEffect(() => () => {
    clearTimeout(closingTimer.current);
    clearTimeout(typeaheadTimer.current);
  }, []);

  return { active, isOpen, isExpanded, toggle, close, setRef, popoverClass };
}
