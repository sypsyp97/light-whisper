import { getCurrentWindow } from "@tauri-apps/api/window";
import { PADDING } from "@/lib/constants";

interface TitleBarProps {
  title: string;
  leftAction?: React.ReactNode;
  rightActions?: React.ReactNode;
}

export default function TitleBar({ title, leftAction, rightActions }: TitleBarProps) {
  const startDrag = (e: React.MouseEvent) => {
    if (e.button !== 0) return;

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
  };

  return (
    <header
      onMouseDown={startDrag}
      className="title-bar"
      style={{ padding: `0 ${PADDING - 8}px`, justifyContent: rightActions ? "space-between" : "flex-start" }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8 }} onMouseDown={e => e.stopPropagation()}>
        {leftAction}
        <span style={{ fontSize: 13, fontWeight: 600, letterSpacing: "0.01em", fontFamily: "var(--font-display)", marginLeft: leftAction ? 0 : 8 }}>
          {title}
        </span>
      </div>
      {rightActions && (
        <div style={{ display: "flex", alignItems: "center", gap: 2 }} onMouseDown={e => e.stopPropagation()}>
          {rightActions}
        </div>
      )}
    </header>
  );
}
