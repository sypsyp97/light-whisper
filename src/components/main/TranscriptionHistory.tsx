import { useState } from "react";
import { Copy } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { HistoryItem } from "@/types";
import IconButton from "@/components/ui/IconButton";

export interface TranscriptionHistoryProps {
  items: HistoryItem[];
  onCopy: (item: HistoryItem) => void;
}

export function TranscriptionHistory({ items, onCopy }: TranscriptionHistoryProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  if (items.length === 0) return null;

  const toggle = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  return (
    <div className="lw-history" data-testid="main-history">
      {items.map((item) => {
        const isExpanded = expanded.has(item.id);
        return (
          <div
            key={item.id}
            className="lw-history-item"
            data-testid={`main-history-item-${item.id}`}
          >
            <div className="lw-history-item-body">
              <p
                className={`lw-history-item-text ${isExpanded ? "lw-history-item-text--expanded" : ""}`}
                role="button"
                tabIndex={0}
                onClick={() => toggle(item.id)}
                onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(item.id); } }}
              >
                {item.text}
              </p>
              <div className="lw-history-item-time">{item.timeDisplay}</div>
            </div>
            <IconButton
              label={t("common.copy")}
              icon={<Copy size={13} />}
              onClick={() => onCopy(item)}
              data-testid={`main-history-copy-${item.id}`}
            />
          </div>
        );
      })}
    </div>
  );
}

export default TranscriptionHistory;
