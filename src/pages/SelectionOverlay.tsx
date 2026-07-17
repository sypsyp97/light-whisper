import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Copy, LoaderCircle, MoveDiagonal2, Replace } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import {
  cancelSelectionAction,
  copySelection,
  getSelectionOverlayState,
  hideSelectionAssistant,
  replaceSelection,
  resizeSelectionWindow,
  runSelectionAction,
  searchSelection,
  startSelectionWindowDrag,
} from "@/api/tauri";
import { useTheme } from "@/hooks/useTheme";
import {
  SelectionToolbar,
  type SelectionToolbarAction,
} from "@/features/selection-assistant/SelectionToolbar";
import { SelectionResult } from "@/features/selection-assistant/SelectionResult";
import "@/styles/selection.css";

type AiAction = "translate" | "explain" | "optimize";

interface ResultContext {
  action: AiAction;
  sourceText: string;
  version: number;
}

interface SelectionStreamEvent {
  status?: string;
  sessionId?: number;
  chunk?: string;
}

const STREAM_LISTENER_READY_TIMEOUT_MS = 250;
let lastSelectionRequestId = Date.now() * 1_000;

function nextSelectionRequestId(): number {
  lastSelectionRequestId = Math.max(lastSelectionRequestId + 1, Date.now() * 1_000);
  return lastSelectionRequestId;
}

async function waitForStreamListener(ready: Promise<void>): Promise<void> {
  let timeoutId: number | undefined;
  const timeout = new Promise<void>((resolve) => {
    timeoutId = window.setTimeout(resolve, STREAM_LISTENER_READY_TIMEOUT_MS);
  });
  await Promise.race([ready, timeout]);
  if (timeoutId !== undefined) window.clearTimeout(timeoutId);
}

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export default function SelectionOverlay() {
  const { t } = useTranslation();
  useTheme();
  const [selectedText, setSelectedText] = useState("");
  const [selectionVersion, setSelectionVersion] = useState(0);
  const [result, setResult] = useState("");
  const [resultContext, setResultContext] = useState<ResultContext | null>(null);
  const [error, setError] = useState("");
  const [loadingAction, setLoadingAction] = useState<AiAction | null>(null);
  const [replacing, setReplacing] = useState(false);
  const [sourceCopied, setSourceCopied] = useState(false);
  const [resultCopied, setResultCopied] = useState(false);
  const requestGenerationRef = useRef(0);
  const activeRequestIdRef = useRef<number | null>(null);
  const streamListenerReadyRef = useRef<Promise<void>>(Promise.resolve());
  const cancellationInFlightRef = useRef<Promise<void> | null>(null);
  const expanded = Boolean(loadingAction || result || error);

  const requestSelectionCancellation = useCallback(() => {
    if (cancellationInFlightRef.current) return cancellationInFlightRef.current;
    const cancellation = cancelSelectionAction()
      .catch(() => false)
      .then(() => undefined);
    cancellationInFlightRef.current = cancellation;
    void cancellation.then(() => {
      if (cancellationInFlightRef.current === cancellation) {
        cancellationInFlightRef.current = null;
      }
    });
    return cancellation;
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    const ready = listen<SelectionStreamEvent>("selection-stream", (event) => {
      const { status, sessionId, chunk } = event.payload;
      if (
        typeof sessionId !== "number"
        || !Number.isSafeInteger(sessionId)
        || sessionId !== activeRequestIdRef.current
      ) return;
      if (status === "reset") {
        setResult("");
        return;
      }
      if (status !== "streaming" || typeof chunk !== "string" || !chunk) return;
      setResult((current) => current + chunk);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    streamListenerReadyRef.current = ready;

    return () => {
      disposed = true;
      activeRequestIdRef.current = null;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let lastVersion = 0;
    const poll = async () => {
      const payload = await getSelectionOverlayState().catch(() => null);
      if (disposed || !payload || payload.version <= lastVersion) return;
      lastVersion = payload.version;
      const hadActiveRequest = activeRequestIdRef.current !== null;
      requestGenerationRef.current += 1;
      activeRequestIdRef.current = null;
      if (hadActiveRequest) void requestSelectionCancellation();
      await (cancellationInFlightRef.current ?? Promise.resolve());
      if (disposed || payload.version !== lastVersion) return;
      setSelectedText(payload.text);
      setSelectionVersion(payload.version);
      setResult("");
      setResultContext(null);
      setError("");
      setReplacing(false);
      setSourceCopied(false);
      setResultCopied(false);
      setLoadingAction(null);
      void resizeSelectionWindow(false);
    };
    void poll();
    const timer = window.setInterval(() => { void poll(); }, 100);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [requestSelectionCancellation]);

  const close = useCallback(async () => {
    requestGenerationRef.current += 1;
    activeRequestIdRef.current = null;
    void requestSelectionCancellation();
    await Promise.allSettled([
      hideSelectionAssistant(),
      getCurrentWindow().hide(),
    ]);
  }, [requestSelectionCancellation]);

  const runAiAction = useCallback(async (action: AiAction) => {
    if (!selectedText.trim() || loadingAction) return;
    const requestGeneration = requestGenerationRef.current + 1;
    requestGenerationRef.current = requestGeneration;
    const requestId = nextSelectionRequestId();
    activeRequestIdRef.current = requestId;
    const sourceText = selectedText;
    const version = selectionVersion;
    setLoadingAction(action);
    setError("");
    setResult("");
    setResultContext(null);
    setResultCopied(false);
    try {
      if (!expanded) await resizeSelectionWindow(true);
      await (cancellationInFlightRef.current ?? Promise.resolve());
      await waitForStreamListener(streamListenerReadyRef.current);
      if (requestGeneration !== requestGenerationRef.current) return;
      const content = (await runSelectionAction(action, sourceText, requestId)).trim();
      if (requestGeneration !== requestGenerationRef.current) return;
      setResult(content);
      setResultContext(content ? { action, sourceText, version } : null);
    } catch (requestError) {
      if (requestGeneration !== requestGenerationRef.current) return;
      setResult("");
      setResultContext(null);
      setError(errorMessage(requestError));
    } finally {
      if (requestGeneration === requestGenerationRef.current) {
        activeRequestIdRef.current = null;
        setLoadingAction(null);
      }
    }
  }, [expanded, loadingAction, selectedText, selectionVersion]);

  const cancelActiveRequest = useCallback(() => {
    const cancellationGeneration = requestGenerationRef.current + 1;
    requestGenerationRef.current = cancellationGeneration;
    activeRequestIdRef.current = null;
    setResult("");
    setResultContext(null);
    setError("");
    void requestSelectionCancellation().then(() => {
      if (requestGenerationRef.current !== cancellationGeneration) return;
      setLoadingAction(null);
      void resizeSelectionWindow(false).catch(() => undefined);
    });
  }, [requestSelectionCancellation]);

  const handleCopy = useCallback(async () => {
    if (!selectedText) return;
    try {
      await copySelection(selectedText);
      setSourceCopied(true);
      window.setTimeout(() => setSourceCopied(false), 1200);
    } catch (copyError) {
      setError(errorMessage(copyError));
      await resizeSelectionWindow(true).catch(() => undefined);
    }
  }, [selectedText]);

  const handleSearch = useCallback(async () => {
    if (!selectedText) return;
    try {
      await searchSelection(selectedText);
      await close();
    } catch (searchError) {
      setError(errorMessage(searchError));
      await resizeSelectionWindow(true).catch(() => undefined);
    }
  }, [close, selectedText]);

  const copyResult = useCallback(async () => {
    if (!result) return;
    try {
      await copySelection(result);
      setResultCopied(true);
      window.setTimeout(() => setResultCopied(false), 1200);
    } catch (copyError) {
      setError(errorMessage(copyError));
    }
  }, [result]);

  const replaceResult = useCallback(async () => {
    if (!result || resultContext?.action !== "optimize" || replacing) return;
    setReplacing(true);
    setError("");
    try {
      await replaceSelection({
        replacementText: result,
        sourceText: resultContext.sourceText,
        version: resultContext.version,
      });
      await close();
    } catch (replaceError) {
      setError(errorMessage(replaceError));
    } finally {
      setReplacing(false);
    }
  }, [close, replacing, result, resultContext]);

  const handleToolbarAction = useCallback((action: SelectionToolbarAction) => {
    if (action === "translate" || action === "explain" || action === "optimize") {
      void runAiAction(action);
    } else if (action === "copy") {
      void handleCopy();
    } else {
      void handleSearch();
    }
  }, [handleCopy, handleSearch, runAiAction]);

  const startWindowDrag = useCallback(() => {
    void getCurrentWindow()
      .startDragging()
      .catch(() => startSelectionWindowDrag());
  }, []);

  const startWindowResize = useCallback(() => {
    void getCurrentWindow().startResizeDragging("SouthEast");
  }, []);

  return (
    <div
      className={`selection-overlay ${expanded ? "selection-overlay-expanded" : ""}`}
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) void close();
      }}
    >
      <section className="selection-panel" onPointerDown={(event) => event.stopPropagation()}>
        <SelectionToolbar
          selectionText={selectedText}
          onAction={handleToolbarAction}
          onStartDrag={startWindowDrag}
          onClose={() => { void close(); }}
          copied={sourceCopied}
          busy={Boolean(loadingAction)}
          labels={{
            toolbar: t("selection.toolbarLabel"),
            selected: t("selection.selected"),
            drag: t("selection.drag"),
            translate: t("selection.translate"),
            explain: t("selection.explain"),
            optimize: t("selection.optimize"),
            copy: t("selection.copy"),
            copied: t("selection.copied"),
            search: t("selection.search"),
            close: t("common.close"),
          }}
        />

        {expanded && (
          <div className="selection-result" aria-live="polite" onPointerDown={(event) => event.stopPropagation()}>
          <div
            className="selection-result-header"
            onPointerDown={(event) => {
              if (event.button !== 0) return;
              event.stopPropagation();
              startWindowDrag();
            }}
          >
            <span>{loadingAction ? t(`selection.${loadingAction}Working`) : error ? t("selection.error") : t("selection.result")}</span>
            {loadingAction ? (
              <button type="button" className="selection-result-action" onPointerDown={(event) => event.stopPropagation()} onClick={cancelActiveRequest}>{t("common.cancel")}</button>
            ) : result ? (
              <div className="selection-result-actions">
                <button type="button" disabled={replacing} className="selection-result-action selection-copy-result" onPointerDown={(event) => event.stopPropagation()} onClick={() => void copyResult()}>
                  {resultCopied ? <Check size={14} /> : <Copy size={14} />}
                  {resultCopied ? t("selection.copied") : t("selection.copyResult")}
                </button>
                {resultContext?.action === "optimize" && (
                  <button type="button" disabled={replacing} className="selection-result-action selection-replace-result" onPointerDown={(event) => event.stopPropagation()} onClick={() => void replaceResult()}>
                    {replacing ? <LoaderCircle size={14} /> : <Replace size={14} />}
                    {replacing ? t("selection.replaceWorking") : t("selection.replaceResult")}
                  </button>
                )}
              </div>
            ) : null}
          </div>
          {error ? (
            <div className="selection-error">{error}</div>
          ) : loadingAction && !result ? (
            <div className="selection-loading"><LoaderCircle size={18} />{t("selection.workingHint")}</div>
          ) : (
            <SelectionResult content={result} />
          )}
          <button
            type="button"
            tabIndex={-1}
            className="selection-resize-handle"
            aria-label={t("selection.resize")}
            title={t("selection.resize")}
            onPointerDown={(event) => {
              if (event.button !== 0) return;
              event.stopPropagation();
              startWindowResize();
            }}
          >
            <MoveDiagonal2 size={14} aria-hidden="true" />
          </button>
          </div>
        )}
      </section>
    </div>
  );
}
