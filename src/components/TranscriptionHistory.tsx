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
  if (history.length === 0) return null;

  const filtered = history.filter(
    (item, idx) => !(idx === 0 && currentResult && item.text === currentResult),
  );

  if (filtered.length === 0) return null;

  return (
    <div className="history-list">
      {filtered.map((item) => (
        <div key={item.id} className="history-item">
          <div className="history-item-body">
            <p className="history-item-text">{item.text}</p>
            <span className="history-item-time">{item.timeDisplay}</span>
          </div>
          <button aria-label="复制" className="icon-btn" style={{ padding: 4, flexShrink: 0 }} onClick={() => onCopy(item.text, item.id)}>
            {copiedId === item.id ? <Check size={11} /> : <Copy size={11} strokeWidth={1.5} />}
          </button>
        </div>
      ))}
    </div>
  );
}
