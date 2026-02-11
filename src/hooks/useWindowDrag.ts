import { useCallback } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface UseWindowDragReturn {
  /** Attach this to onMouseDown on your drag region element. */
  startDrag: (e: React.MouseEvent) => void;
}

/**
 * React hook for enabling window dragging on a frameless Tauri window.
 *
 * Usage:
 * ```tsx
 * const { startDrag } = useWindowDrag();
 * <div onMouseDown={startDrag} data-tauri-drag-region>Title Bar</div>
 * ```
 */
export function useWindowDrag(): UseWindowDragReturn {
  const onDragStart = useCallback((e: React.MouseEvent) => {
    // Only handle primary (left) mouse button
    if (e.button !== 0) return;

    // Don't initiate drag if the user clicked on an interactive element
    const target = e.target as HTMLElement;
    if (
      target.closest("button") ||
      target.closest("input") ||
      target.closest("select") ||
      target.closest("textarea") ||
      target.closest("a") ||
      target.closest("[data-no-drag]")
    ) {
      return;
    }

    e.preventDefault();
    getCurrentWindow().startDragging();
  }, []);

  return { startDrag: onDragStart };
}
