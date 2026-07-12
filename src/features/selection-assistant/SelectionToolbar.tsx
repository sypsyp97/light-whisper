import { useRef } from "react";
import { Check, Copy, GripVertical, Languages, Search, Sparkles, WandSparkles, X } from "lucide-react";

import { normalizeSelectionText } from "./selectionPolicy";

export type SelectionToolbarAction = "translate" | "explain" | "optimize" | "copy" | "search";
interface ToolbarLabels {
  toolbar: string;
  translate: string;
  explain: string;
  optimize: string;
  copy: string;
  copied: string;
  search: string;
  close: string;
  selected: string;
  drag: string;
}

const DEFAULT_LABELS: ToolbarLabels = {
  toolbar: "划词助手",
  translate: "翻译",
  explain: "解释",
  optimize: "优化",
  copy: "复制",
  copied: "已复制",
  search: "搜索",
  close: "关闭",
  selected: "已选择",
  drag: "拖动窗口",
};

interface SelectionToolbarProps {
  selectionText: string;
  onAction(action: SelectionToolbarAction): void;
  onStartDrag(): void;
  onClose?: () => void;
  copied?: boolean;
  busy?: boolean;
  labels?: Partial<ToolbarLabels>;
}

export function SelectionToolbar({
  selectionText,
  onAction,
  onStartDrag,
  onClose,
  copied = false,
  busy = false,
  labels: labelOverrides,
}: SelectionToolbarProps) {
  const closeHandledByPointer = useRef(false);
  const labels = { ...DEFAULT_LABELS, ...labelOverrides };
  const hasSelection = Boolean(normalizeSelectionText(selectionText));
  const actionButton = (
    action: SelectionToolbarAction,
    label: string,
    icon: React.ReactNode,
  ) => (
    <button
      type="button"
      tabIndex={-1}
      disabled={!hasSelection || busy}
      aria-label={label}
      title={label}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={() => onAction(action)}
    >
      {icon}<span>{action === "copy" && copied ? labels.copied : label}</span>
    </button>
  );

  return (
    <div
      className="selection-toolbar"
      role="toolbar"
      aria-label={labels.toolbar}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div
        className="selection-preview-row"
        aria-label={labels.drag}
        title={labels.drag}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          event.stopPropagation();
          onStartDrag();
        }}
      >
        <div className="selection-brand" aria-hidden="true"><Sparkles size={15} /></div>
        <div className="selection-preview-copy">
          <span>{labels.selected}</span>
          <p title={selectionText}>{selectionText}</p>
        </div>
        <GripVertical className="selection-drag-handle" size={16} aria-hidden="true" />
        {onClose ? (
          <button
            type="button"
            tabIndex={-1}
            className="selection-close"
            aria-label={labels.close}
            title={labels.close}
            onPointerDown={(event) => {
              event.stopPropagation();
              closeHandledByPointer.current = true;
              onClose();
              window.setTimeout(() => {
                closeHandledByPointer.current = false;
              }, 0);
            }}
            onClick={() => {
              if (closeHandledByPointer.current) {
                closeHandledByPointer.current = false;
                return;
              }
              onClose();
            }}
          >
            <X size={16} />
          </button>
        ) : null}
      </div>
      <div className="selection-actions">
        {actionButton("translate", labels.translate, <Languages size={15} />)}
        {actionButton("explain", labels.explain, <Sparkles size={15} />)}
        {actionButton("optimize", labels.optimize, <WandSparkles size={15} />)}
        {actionButton("copy", labels.copy, copied ? <Check size={15} /> : <Copy size={15} />)}
        {actionButton("search", labels.search, <Search size={15} />)}
      </div>
    </div>
  );
}
