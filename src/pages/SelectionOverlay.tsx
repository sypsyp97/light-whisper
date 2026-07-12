import { useCallback, useEffect, useState } from "react";
import { Check, Copy, LoaderCircle, MoveDiagonal2 } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import {
  cancelSelectionAction,
  copySelection,
  getSelectionOverlayState,
  hideSelectionAssistant,
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

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export default function SelectionOverlay() {
  const { t } = useTranslation();
  useTheme();
  const [selectedText, setSelectedText] = useState("");
  const [result, setResult] = useState("");
  const [error, setError] = useState("");
  const [loadingAction, setLoadingAction] = useState<AiAction | null>(null);
  const [sourceCopied, setSourceCopied] = useState(false);
  const [resultCopied, setResultCopied] = useState(false);
  const expanded = Boolean(loadingAction || result || error);

  useEffect(() => {
    let disposed = false;
    let lastVersion = 0;
    const poll = async () => {
      const payload = await getSelectionOverlayState().catch(() => null);
      if (disposed || !payload || payload.version === lastVersion) return;
      lastVersion = payload.version;
      setSelectedText(payload.text);
      setResult("");
      setError("");
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
    setLoadingAction(action);
    setError("");
    setResult("");
    try {
      if (!expanded) await resizeSelectionWindow(true);
      const content = await runSelectionAction(action, selectedText);
      setResult(content.trim());
    } catch (requestError) {
      setError(errorMessage(requestError));
    } finally {
      setLoadingAction(null);
    }
  }, [expanded, loadingAction, selectedText]);

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
              <button type="button" className="selection-result-action selection-copy-result" onPointerDown={(event) => event.stopPropagation()} onClick={() => void copyResult()}>
                {resultCopied ? <Check size={14} /> : <Copy size={14} />}
                {resultCopied ? t("selection.copied") : t("selection.copyResult")}
              </button>
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
