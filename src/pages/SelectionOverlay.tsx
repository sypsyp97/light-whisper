import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Copy, LoaderCircle, MoveDiagonal2, Replace } from "lucide-react";
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
  const expanded = Boolean(loadingAction || result || error);

  useEffect(() => {
    let disposed = false;
    let lastVersion = 0;
    const poll = async () => {
      const payload = await getSelectionOverlayState().catch(() => null);
      if (disposed || !payload || payload.version === lastVersion) return;
      lastVersion = payload.version;
      requestGenerationRef.current += 1;
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
  }, []);

  const close = useCallback(async () => {
    void cancelSelectionAction().catch(() => false);
    await Promise.allSettled([
      hideSelectionAssistant(),
      getCurrentWindow().hide(),
    ]);
  }, []);

  const runAiAction = useCallback(async (action: AiAction) => {
    if (!selectedText.trim() || loadingAction) return;
    const requestGeneration = requestGenerationRef.current + 1;
    requestGenerationRef.current = requestGeneration;
    const sourceText = selectedText;
    const version = selectionVersion;
    setLoadingAction(action);
    setError("");
    setResult("");
    setResultContext(null);
    try {
      if (!expanded) await resizeSelectionWindow(true);
      const content = (await runSelectionAction(action, sourceText)).trim();
      if (requestGeneration !== requestGenerationRef.current) return;
      setResult(content);
      setResultContext(content ? { action, sourceText, version } : null);
    } catch (requestError) {
      if (requestGeneration !== requestGenerationRef.current) return;
      setError(errorMessage(requestError));
    } finally {
      if (requestGeneration === requestGenerationRef.current) {
        setLoadingAction(null);
      }
    }
  }, [expanded, loadingAction, selectedText, selectionVersion]);

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
              <button type="button" className="selection-result-action" onPointerDown={(event) => event.stopPropagation()} onClick={() => void cancelSelectionAction()}>{t("common.cancel")}</button>
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
          {loadingAction ? (
            <div className="selection-loading"><LoaderCircle size={18} />{t("selection.workingHint")}</div>
          ) : (
            error ? <div className="selection-error">{error}</div> : <SelectionResult content={result} />
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
