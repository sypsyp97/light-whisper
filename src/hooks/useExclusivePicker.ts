import { useState, useCallback, useEffect, useLayoutEffect, useRef } from "react";

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
  const activeRef = useRef<T | null>(null);
  const refs = useRef(new Map<T, HTMLDivElement | null>());
  const closingTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const typeaheadTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const typeaheadBuffer = useRef("");
  activeRef.current = active;

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
    const closingId = activeRef.current;
    if (!closingId) return;
    setActive(null);
    setClosing(closingId);
    clearTimeout(closingTimer.current);
    closingTimer.current = setTimeout(() => setClosing(null), POPOVER_EXIT_MS);
    window.requestAnimationFrame(() => {
      refs.current
        .get(closingId)
        ?.querySelector<HTMLElement>('[aria-haspopup]')
        ?.focus();
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
    const onPointerDown = (e: PointerEvent) => {
      const ref = refs.current.get(active);
      if (ref && !ref.contains(e.target as Node)) {
        // Close via animated path
        setActive(null);
        setClosing(active);
        clearTimeout(closingTimer.current);
        closingTimer.current = setTimeout(() => setClosing(null), POPOVER_EXIT_MS);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [active]);

  useLayoutEffect(() => {
    if (!active) return;
    const container = refs.current.get(active);
    const trigger = container?.querySelector<HTMLElement>('[aria-haspopup]');
    const listbox = container?.querySelector<HTMLElement>('[role="listbox"], [role="dialog"]');
    if (!container || !trigger || !listbox) return;

    const isDialog = listbox.getAttribute("role") === "dialog";
    const listboxId = `picker-${String(active)}-${isDialog ? "dialog" : "listbox"}`;
    listbox.id = listboxId;
    trigger.setAttribute("aria-controls", listboxId);
    if (!listbox.hasAttribute("aria-label")) {
      const triggerLabel = trigger.getAttribute("aria-label");
      if (triggerLabel) listbox.setAttribute("aria-label", triggerLabel);
    }

    let options: HTMLButtonElement[] = [];
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
    // Account popovers contain selection and management buttons. Keep ordinary
    // tab navigation instead of treating their actions as listbox options.
    if (isDialog) {
      const onEscape = (event: globalThis.KeyboardEvent) => {
        if (event.key === "Escape") {
          event.preventDefault();
          close();
        }
      };
      container.addEventListener("keydown", onEscape);
      const frame = window.requestAnimationFrame(() => {
        const selected = listbox.querySelector<HTMLElement>('[aria-pressed="true"]:not(:disabled)');
        (selected ?? listbox.querySelector<HTMLElement>("button:not(:disabled)"))?.focus();
      });
      return () => {
        window.cancelAnimationFrame(frame);
        container.removeEventListener("keydown", onEscape);
        trigger.removeAttribute("aria-controls");
      };
    }
    const updateOptions = () => {
      options = Array.from(listbox.querySelectorAll<HTMLButtonElement>("button.picker-option:not(:disabled)"));
      const focusedOption = options.find((option) => option === document.activeElement);
      options.forEach((option, index) => {
        option.id = `${listboxId}-option-${index}`;
        option.setAttribute("role", "option");
        option.setAttribute("aria-selected", String(option.dataset.active === "true"));
        option.tabIndex = focusedOption
          ? option === focusedOption ? 0 : -1
          : option.dataset.active === "true" ? 0 : -1;
      });
      if (options.length && !options.some((option) => option.tabIndex === 0)) options[0].tabIndex = 0;
    };
    updateOptions();
    // Model fetches and search can replace the options while the picker stays open.
    const observer = new MutationObserver(updateOptions);
    observer.observe(listbox, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-active", "disabled"] });

    const focusOption = (index: number) => {
      if (!options.length) return;
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
        return;
      }
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) {
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
    let openingFocusFrame: number | undefined;
    if (!container.querySelector("input, textarea")) {
      openingFocusFrame = window.requestAnimationFrame(() => {
        const selectedIndex = options.findIndex((option) => option.dataset.active === "true");
        focusOption(selectedIndex >= 0 ? selectedIndex : 0);
      });
    }

    return () => {
      if (openingFocusFrame !== undefined) {
        window.cancelAnimationFrame(openingFocusFrame);
      }
      container.removeEventListener("keydown", onKeyDown);
      observer.disconnect();
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
