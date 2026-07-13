import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowLeft,
  Check,
  ChevronDown,
  ChevronUp,
  Clock3,
  Copy,
  Download,
  FileAudio,
  Minus,
  RefreshCw,
  Search,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import {
  copyToClipboard,
  deleteTranscriptionHistory,
  exportTranscriptionHistory,
  getTranscriptionHistoryStats,
  hideMainWindow,
  listTranscriptionHistory,
  reprocessTranscriptionHistory,
} from "@/api/tauri";
import TitleBar from "@/components/TitleBar";
import type {
  PersistentHistoryRecord,
  PersistentHistoryStats,
  RecordingMode,
} from "@/types";

type View = "main" | "settings" | "history";
type StatusFilter = "" | "success" | "failed";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatLatency(ms?: number | null): string {
  if (ms == null) return "—";
  return ms >= 1000 ? `${(ms / 1000).toFixed(ms >= 10_000 ? 1 : 2)}s` : `${ms}ms`;
}

function recordDate(value: number, language: string): string {
  return new Intl.DateTimeFormat(language, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

export default function HistoryPage({
  onNavigate,
  animClass = "",
}: {
  onNavigate: (view: View) => void;
  animClass?: string;
}) {
  const { t, i18n } = useTranslation();
  const [items, setItems] = useState<PersistentHistoryRecord[]>([]);
  const [stats, setStats] = useState<PersistentHistoryStats | null>(null);
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"" | RecordingMode>("");
  const [status, setStatus] = useState<StatusFilter>("");
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [copiedId, setCopiedId] = useState<number | null>(null);
  const [workingId, setWorkingId] = useState<number | null>(null);
  const [exportFormat, setExportFormat] = useState<"json" | "markdown">("markdown");
  const requestId = useRef(0);
  const itemCount = useRef(0);
  const pageSize = 50;

  useEffect(() => {
    itemCount.current = items.length;
  }, [items.length]);

  const refreshStats = useCallback(async () => {
    try {
      setStats(await getTranscriptionHistoryStats());
    } catch (error) {
      console.error("Failed to load history stats:", error);
    }
  }, []);

  const load = useCallback(async (reset: boolean) => {
    const currentRequest = ++requestId.current;
    const offset = reset ? 0 : itemCount.current;
    if (reset) setLoading(true);
    setLoadError("");
    try {
      const page = await listTranscriptionHistory({
        query,
        mode,
        status,
        limit: pageSize,
        offset,
      });
      if (currentRequest !== requestId.current) return;
      setItems((previous) => reset ? page.items : [...previous, ...page.items]);
      setTotal(page.total);
      setHasMore(page.hasMore);
    } catch (error) {
      if (currentRequest !== requestId.current) return;
      setLoadError(errorMessage(error));
    } finally {
      if (currentRequest === requestId.current) setLoading(false);
    }
  }, [mode, query, status]);
  const loadRef = useRef(load);

  useEffect(() => {
    loadRef.current = load;
  }, [load]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void load(true);
    }, query ? 180 : 0);
    return () => window.clearTimeout(timer);
  }, [load, query, mode, status]);

  useEffect(() => {
    void refreshStats();
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen("history-updated", () => {
      void loadRef.current(true);
      void refreshStats();
    }).then((dispose) => {
      if (disposed) {
        dispose();
      } else {
        unlisten = dispose;
      }
    }).catch((error) => {
      console.error("Failed to listen for history updates:", error);
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refreshStats]);

  const hasActiveFilter = Boolean(query || mode || status);
  const latencyCards = useMemo(() => stats ? [
    { label: t("historyPage.asrLatency"), value: stats.asr },
    { label: t("historyPage.polishLatency"), value: stats.polish },
    { label: t("historyPage.totalLatency"), value: stats.totalLatency },
  ] : [], [stats, t]);

  const copyRecord = async (record: PersistentHistoryRecord) => {
    const text = record.text || record.originalText;
    if (!text) return;
    try {
      await copyToClipboard(text);
      setCopiedId(record.id);
      window.setTimeout(() => setCopiedId((value) => value === record.id ? null : value), 1400);
    } catch {
      toast.error(t("common.copyFailed"));
    }
  };

  const reprocess = async (record: PersistentHistoryRecord, kind: "polish" | "asr") => {
    if (workingId != null) return;
    setWorkingId(record.id);
    try {
      await reprocessTranscriptionHistory(record.id, kind);
      toast.success(t("historyPage.reprocessDone"));
      await Promise.all([load(true), refreshStats()]);
    } catch (error) {
      toast.error(t("historyPage.reprocessFailed", { error: errorMessage(error) }));
    } finally {
      setWorkingId(null);
    }
  };

  const remove = async (record: PersistentHistoryRecord) => {
    if (workingId != null) return;
    if (!window.confirm(t("historyPage.deleteConfirm"))) return;
    setWorkingId(record.id);
    try {
      const removed = await deleteTranscriptionHistory(record.id);
      if (!removed) {
        await Promise.all([load(true), refreshStats()]);
        return;
      }
      setItems((previous) => previous.filter((item) => item.id !== record.id));
      setTotal((value) => Math.max(0, value - 1));
      await refreshStats();
    } catch (error) {
      toast.error(t("historyPage.deleteFailed", { error: errorMessage(error) }));
    } finally {
      setWorkingId(null);
    }
  };

  const exportHistory = async () => {
    try {
      const path = await exportTranscriptionHistory(exportFormat);
      if (path) toast.success(t("historyPage.exportDone"));
    } catch (error) {
      toast.error(t("historyPage.exportFailed", { error: errorMessage(error) }));
    }
  };

  return (
    <div className="page-root">
      <TitleBar
        title={t("historyPage.title")}
        leftAction={(
          <button className="icon-btn plain" aria-label={t("common.back")} onClick={() => onNavigate("main")}>
            <ArrowLeft size={14} />
          </button>
        )}
        rightActions={(
          <>
            <button aria-label={t("common.minimize")} className="icon-btn" onClick={() => getCurrentWindow().minimize()}>
              <Minus size={12} />
            </button>
            <button aria-label={t("common.close")} className="icon-btn" onClick={() => hideMainWindow()}>
              <X size={12} />
            </button>
          </>
        )}
      />

      <main className={`page-content history-page ${animClass}`.trim()}>
        <section className="history-command-bar" aria-label={t("historyPage.title")}>
          <label className="history-search">
            <Search size={14} aria-hidden="true" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("historyPage.searchPlaceholder")}
              aria-label={t("historyPage.searchPlaceholder")}
            />
          </label>
          <div className="history-export-control">
            <select value={exportFormat} onChange={(event) => setExportFormat(event.target.value as "json" | "markdown")}>
              <option value="markdown">{t("historyPage.exportMarkdown")}</option>
              <option value="json">{t("historyPage.exportJson")}</option>
            </select>
            <button className="icon-btn" aria-label={t("historyPage.export")} onClick={() => void exportHistory()}>
              <Download size={14} />
            </button>
          </div>
        </section>

        <section className="history-filter-row">
          <div className="history-filter-group" role="group" aria-label={t("historyPage.allModes")}>
            {(["", "dictation", "assistant"] as const).map((value) => (
              <button key={value || "all"} className={mode === value ? "active" : ""} onClick={() => setMode(value)}>
                {value === "" ? t("historyPage.allModes") : t(`historyPage.${value}`)}
              </button>
            ))}
          </div>
          <select className="history-status-filter" value={status} onChange={(event) => setStatus(event.target.value as StatusFilter)}>
            <option value="">{t("historyPage.allStatuses")}</option>
            <option value="success">{t("historyPage.success")}</option>
            <option value="failed">{t("historyPage.failed")}</option>
          </select>
        </section>

        {stats && stats.total > 0 && (
          <section className="history-metrics" aria-label={t("historyPage.total", { count: stats.total })}>
            <div className="history-metric history-metric-primary">
              <strong>{stats.total.toLocaleString()}</strong>
              <span>{t("historyPage.characters", { count: stats.totalCharacters.toLocaleString() })}</span>
            </div>
            {latencyCards.map(({ label, value }) => (
              <div className="history-metric" key={label}>
                <span>{label}</span>
                <strong>{formatLatency(value.p50Ms)}</strong>
                <small>P95 {formatLatency(value.p95Ms)}</small>
              </div>
            ))}
          </section>
        )}

        <section className="history-scroll-region">
          {!loading && loadError && (
            <div className="history-empty history-empty-error">
              <p>{t("historyPage.loadFailed", { error: loadError })}</p>
              <button className="btn-ghost" onClick={() => void load(true)}><RefreshCw size={13} />{t("common.retry")}</button>
            </div>
          )}

          {!loading && !loadError && items.length === 0 && (
            <div className="history-empty">
              <Clock3 size={24} />
              <strong>{t(hasActiveFilter ? "historyPage.noMatchTitle" : "historyPage.emptyTitle")}</strong>
              <p>{t(hasActiveFilter ? "historyPage.noMatchHint" : "historyPage.emptyHint")}</p>
            </div>
          )}

          {loading && items.length === 0 && (
            <div className="history-loading" aria-label={t("common.loading")}>
              <span /><span /><span />
            </div>
          )}

          <div className="persistent-history-list">
            {items.map((record) => {
              const failed = record.status !== "success";
              const hasDistinctRaw = Boolean(record.originalText && record.originalText !== record.text);
              const hasSourceText = Boolean(record.sourceText?.trim());
              const hasDetails = hasDistinctRaw || hasSourceText;
              const expanded = expandedId === record.id;
              const isWorking = workingId === record.id;
              const canReprocess = record.workflow === "dictation";
              return (
                <article className={`persistent-history-card${failed ? " history-card-failed" : ""}`} key={record.id}>
                  <header className="history-card-header">
                    <div className="history-card-context">
                      <span className={`history-status-dot${failed ? " failed" : ""}`} />
                      <strong>{record.appProcess || t(`historyPage.${record.mode}`)}</strong>
                      <time>{recordDate(record.createdAt, i18n.language)}</time>
                    </div>
                    <div className="history-card-flags">
                      {record.reprocessedFromId != null && <span>{t("historyPage.reprocessed")}</span>}
                      {record.audioAvailable && <span title={t("historyPage.audioSaved")}><FileAudio size={11} /></span>}
                    </div>
                  </header>

                  <div className="history-card-copy">
                    {failed ? (
                      <p className="history-card-error">{record.error || record.status}</p>
                    ) : (
                      <p>{record.text}</p>
                    )}
                    {record.appRuleName && <small>{t("historyPage.appRule", { name: record.appRuleName })}</small>}
                  </div>

                  {hasDetails && expanded && (
                    <div className="history-raw-panel">
                      {hasDistinctRaw && (
                        <>
                          <span>{t("historyPage.rawText")}</span>
                          <p>{record.originalText}</p>
                        </>
                      )}
                      {hasSourceText && (
                        <>
                          <span>{t("historyPage.sourceText")}</span>
                          <p>{record.sourceText}</p>
                        </>
                      )}
                    </div>
                  )}

                  <div className="history-latency-strip" aria-label={t("historyPage.totalLatency")}>
                    <span><b>{t("historyPage.asrLatency")}</b>{formatLatency(record.asrMs)}</span>
                    <span><b>{t("historyPage.polishLatency")}</b>{formatLatency(record.polishMs)}</span>
                    <span><b>{t("historyPage.totalLatency")}</b>{formatLatency(record.totalMs)}</span>
                    {record.engine && <span className="history-engine-label">{record.engine}</span>}
                  </div>

                  <footer className="history-card-actions">
                    {hasDetails && (
                      <button className="history-text-action" onClick={() => setExpandedId(expanded ? null : record.id)}>
                        {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
                        {t(expanded ? "historyPage.hideDetails" : "historyPage.showDetails")}
                      </button>
                    )}
                    <span className="history-action-spacer" />
                    {canReprocess && (
                      <button className="icon-btn icon-btn-sm" disabled={isWorking || (!record.text && !record.originalText)} title={t("historyPage.reprocessPolish")} aria-label={t("historyPage.reprocessPolish")} onClick={() => void reprocess(record, "polish")}>
                        {isWorking ? <RefreshCw className="history-spin" size={13} /> : <Sparkles size={13} />}
                      </button>
                    )}
                    {canReprocess && record.audioAvailable && (
                      <button className="icon-btn icon-btn-sm" disabled={isWorking} title={t("historyPage.reprocessAsr")} aria-label={t("historyPage.reprocessAsr")} onClick={() => void reprocess(record, "asr")}>
                        <RefreshCw size={13} />
                      </button>
                    )}
                    <button className="icon-btn icon-btn-sm" title={t("common.copy")} aria-label={t("common.copy")} onClick={() => void copyRecord(record)}>
                      {copiedId === record.id ? <Check size={13} /> : <Copy size={13} />}
                    </button>
                    <button className="icon-btn icon-btn-sm history-delete-action" disabled={isWorking} title={t("historyPage.delete")} aria-label={t("historyPage.delete")} onClick={() => void remove(record)}>
                      <Trash2 size={13} />
                    </button>
                  </footer>
                </article>
              );
            })}
          </div>

          {hasMore && !loading && (
            <button className="history-load-more" onClick={() => void load(false)}>
              {t("historyPage.loadMore")} · {items.length}/{total}
            </button>
          )}
        </section>
      </main>
    </div>
  );
}
