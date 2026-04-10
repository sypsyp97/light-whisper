import { useTranslation } from "react-i18next";
import { Copy, Check } from "lucide-react";
import type { HistoryItem } from "@/types";

interface TranscriptionHistoryProps {
  history: HistoryItem[];
  currentResult: string | null;
  copiedId: string | null;
  onCopy: (text: string, id: string) => void;
}

export default function TranscriptionHistory({
  history, currentResult, copiedId, onCopy,
}: TranscriptionHistoryProps) {
  const { t } = useTranslation();
  if (history.length === 0) return null;

  const filtered = history.filter(
    (item, idx) => !(idx === 0 && currentResult && item.text === currentResult),
  );

  if (filtered.length === 0) return null;

  return (
    <div className="history-list">
      {filtered.map((item, idx) => (
        <div key={item.id} className="history-item" style={{ animationDelay: `${idx * 50}ms` }}>
          <div className="history-item-body">
            <p className="history-item-text">{item.text}</p>
            <span className="history-item-time">{item.timeDisplay}</span>
          </div>
          <button aria-label={t("common.copy")} className="icon-btn icon-btn-sm" onClick={() => onCopy(item.text, item.id)}>
            {copiedId === item.id
              ? <span className="animate-check-draw"><Check size={11} /></span>
              : <Copy size={11} strokeWidth={1.5} />}
          </button>
        </div>
      ))}
    </div>
  );
}
