import { useState, useCallback, useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toast } from "sonner";
import i18n from "@/i18n";
import { cancelDictationPolish, copyToClipboard, startRecording as invokeStartRecording, stopRecording as invokeStopRecording } from "@/api/tauri";
import type {
  EditGrabStatus,
  HistoryItem,
  RecordingMode,
  TranscriptionTiming,
} from "@/types";

interface UseRecordingReturn {
  polishAuditWarning: boolean;
  isStarting: boolean;
  isRecording: boolean;
  isProcessing: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<void>;
  cancelPolish: () => Promise<void>;
  error: string | null;
  transcriptionResult: string | null;
  setTranscriptionResult: (text: string) => void;
  originalAsrText: string | null;
  editBaselineText: string | null;
  setEditBaselineText: (text: string | null) => void;
  durationSec: number | null;
  charCount: number | null;
  detectedLanguage: string | null;
  editGrabStatus: EditGrabStatus | null;
  timing: TranscriptionTiming | null;
  history: HistoryItem[];
  resultMode: RecordingMode;
}

interface RecordingStatePayload {
  sessionId: number;
  revision?: number;
  isStarting?: boolean;
  isRecording: boolean;
  isProcessing: boolean;
  error?: string;
  mode?: RecordingMode;
}

interface TranscriptionPayload {
  sessionId: number;
  text: string;
  interim: boolean;
  durationSec?: number;
  charCount?: number;
  language?: string;
  mode?: RecordingMode;
  originalText?: string;
  editGrabStatus?: EditGrabStatus;
  timing?: TranscriptionTiming;
}

/** 封装 Tauri 事件监听的 useEffect 样板 */
function useTauriEvent<T>(event: string, handler: (payload: T) => void) {
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let disposed = false;

    listen<T>(event, (e) => {
      if (!disposed) handler(e.payload);
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      unlisten?.();
    };
  // handler is intentionally excluded - callers use refs or stable callbacks
  }, [event]);
}

export function useRecording(): UseRecordingReturn {
  const [isRecording, setIsRecording] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transcriptionResult, setTranscriptionResult] = useState<string | null>(null);
  const [originalAsrText, setOriginalAsrText] = useState<string | null>(null);
  const [editBaselineText, setEditBaselineText] = useState<string | null>(null);
  const [durationSec, setDurationSec] = useState<number | null>(null);
  const [charCount, setCharCount] = useState<number | null>(null);
  const [detectedLanguage, setDetectedLanguage] = useState<string | null>(null);
  const [editGrabStatus, setEditGrabStatus] = useState<EditGrabStatus | null>(null);
  const [timing, setTiming] = useState<TranscriptionTiming | null>(null);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [resultMode, setResultMode] = useState<RecordingMode>("dictation");
  const latestSessionIdRef = useRef(0);
  const pendingStopRef = useRef<{
    sessionId: number;
    promise: ReturnType<typeof invokeStopRecording>;
  } | null>(null);
  const latestRecordingStateRevisionRef = useRef(-1);
  const latestDisplayedFinalSessionIdRef = useRef(0);
  const outputTimingBySessionRef = useRef(new Map<number, number>());
  const lastAuditWarningSessionRef = useRef(0);
  const lastAuditToastSessionRef = useRef(0);
  const [auditWarningSession, setAuditWarningSession] = useState<number | null>(null);

  useTauriEvent<{ sessionId: number; text: string; reason: string }>("paste-recovery", (payload) => {
    if (!Number.isSafeInteger(payload.sessionId) || payload.sessionId <= 0
      || typeof payload.text !== "string" || payload.text.length === 0) return;
    toast.warning(i18n.t("toast.pasteRecovery"), {
      id: `paste-recovery-${payload.sessionId}`,
      duration: Infinity,
      action: {
        label: i18n.t("common.copy"),
        onClick: async () => {
          try {
            await copyToClipboard(payload.text);
          } catch {
            toast.error(i18n.t("toast.pasteRecoveryCopyFailed"));
          }
        },
      },
    });
  });

  useTauriEvent<{ sessionId: number; status: string }>("jev-polish-audit", (payload) => {
    if (payload.status !== "possible_change" || !Number.isSafeInteger(payload.sessionId)
      || payload.sessionId <= 0 || payload.sessionId < latestSessionIdRef.current
      || payload.sessionId <= lastAuditWarningSessionRef.current) return;
    lastAuditWarningSessionRef.current = payload.sessionId;
    setAuditWarningSession(payload.sessionId);
  });

  useEffect(() => {
    if (auditWarningSession === null || auditWarningSession !== latestDisplayedFinalSessionIdRef.current
      || auditWarningSession < latestSessionIdRef.current
      || auditWarningSession <= lastAuditToastSessionRef.current) return;
    lastAuditToastSessionRef.current = auditWarningSession;
    toast.warning(i18n.t("settings.jevAuditWarning"), { duration: 8000 });
  }, [auditWarningSession, history]);

  useTauriEvent<RecordingStatePayload>("recording-state", (payload) => {
    const sessionId = Number(payload.sessionId || 0);
    if (sessionId < latestSessionIdRef.current) return;
    if (sessionId > latestSessionIdRef.current) {
      setAuditWarningSession(null);
      latestSessionIdRef.current = sessionId;
      latestRecordingStateRevisionRef.current = -1;
    }
    const revision = Number(payload.revision);
    const hasRevision = Number.isSafeInteger(revision) && revision >= 0;
    if (hasRevision && revision <= latestRecordingStateRevisionRef.current) return;
    if (!hasRevision && latestRecordingStateRevisionRef.current >= 0) return;
    if (hasRevision) latestRecordingStateRevisionRef.current = revision;
    setIsStarting(payload.isStarting ?? false);
    setIsRecording(payload.isRecording);
    setIsProcessing(payload.isProcessing);
    setError(payload.error ?? null);
    setResultMode(payload.mode ?? "dictation");
  });

  useTauriEvent<{ message: string; sessionId?: number }>("recording-error", (payload) => {
    const sessionId = Number(payload.sessionId || 0);
    if (sessionId > 0 && sessionId < latestSessionIdRef.current) return;
    const message = payload.message?.trim();
    if (message) setError(message);
  });

  useTauriEvent<{ sessionId: number; totalMs: number }>("transcription-timing", ({ sessionId, totalMs }) => {
    if (!Number.isSafeInteger(sessionId) || sessionId <= 0
      || !Number.isSafeInteger(totalMs) || totalMs < 0) return;
    const timings = outputTimingBySessionRef.current;
    timings.delete(sessionId);
    timings.set(sessionId, totalMs);
    if (timings.size > 20) {
      const oldestSession = timings.keys().next().value;
      if (oldestSession !== undefined) timings.delete(oldestSession);
    }
    setHistory((previous) => previous.map((item) => item.id === `session-${sessionId}`
      ? { ...item, timing: { ...item.timing, totalMs } }
      : item));
    if (sessionId === latestDisplayedFinalSessionIdRef.current
      && sessionId >= latestSessionIdRef.current) {
      setTiming((previous) => ({ ...previous, totalMs }));
    }
  });

  useTauriEvent<TranscriptionPayload>("transcription-result", (payload) => {
    if (payload.interim) return;
    const sessionId = Number(payload.sessionId || 0);
    const { text } = payload;
    const rawText = payload.originalText ?? text;
    const outputTotalMs = outputTimingBySessionRef.current.get(sessionId);
    const resultTiming = outputTotalMs === undefined
      ? payload.timing
      : { ...payload.timing, totalMs: outputTotalMs };
    const now = Date.now();
    const historyId = sessionId > 0 ? `session-${sessionId}` : `session-local-${now}`;
    // 旧 session 的 final 结果可以进 history，但不能覆盖当前显示。避免的是
    // 用户已经开始下一段录音后，上一段 ASR 延迟回来闪一下当前画面。
    const staleForDisplay = sessionId > 0 && sessionId < latestSessionIdRef.current;

    if (!staleForDisplay && sessionId >= latestDisplayedFinalSessionIdRef.current) {
      latestDisplayedFinalSessionIdRef.current = sessionId;
      setTranscriptionResult(text);
      setOriginalAsrText(rawText);
      setEditBaselineText(text);
      setDurationSec(payload.durationSec ?? null);
      setCharCount(payload.charCount ?? null);
      setDetectedLanguage(payload.language ?? null);
      setEditGrabStatus(payload.editGrabStatus ?? null);
      setTiming(resultTiming ?? null);
      setResultMode(payload.mode ?? "dictation");
    }

    if (text.trim()) {
      setHistory((prev) =>
        [
          {
            id: historyId,
            text,
            originalText: rawText,
            timestamp: now, timeDisplay: new Date(now).toLocaleTimeString(i18n.language),
            editGrabStatus: payload.editGrabStatus,
            timing: resultTiming,
          },
          ...prev.filter((item) => item.id !== historyId),
        ].slice(0, 20)
      );
    }
  });

  useTauriEvent<{ status: string; error: string }>("ai-polish-status", ({ status, error: errMsg }) => {
    if (status === "applied") toast.success(i18n.t("toast.aiPolishApplied"), { duration: 1500 });
    else if (status === "error") toast.error(i18n.t("toast.aiPolishFailed", { error: errMsg }), { duration: 2500 });
  });

  const startRecording = useCallback(async () => {
    setError(null);
    try {
      const sessionId = await invokeStartRecording();
      if (Number.isFinite(sessionId) && sessionId > latestSessionIdRef.current) {
        latestSessionIdRef.current = sessionId;
        latestRecordingStateRevisionRef.current = -1;
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const stopRecording = useCallback(async (): Promise<void> => {
    const sessionId = latestSessionIdRef.current;
    if (isStarting || isRecording) {
      setIsStarting(false);
      setIsRecording(false);
      setIsProcessing(isRecording);
    }
    let pendingStop: ReturnType<typeof invokeStopRecording> | null = null;
    try {
      pendingStop = invokeStopRecording();
      pendingStopRef.current = { sessionId, promise: pendingStop };
      await pendingStop;
    } catch (err) {
      if (sessionId === latestSessionIdRef.current) {
        if (isRecording) setIsProcessing(false);
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      if (pendingStopRef.current?.promise === pendingStop) pendingStopRef.current = null;
    }
  }, [isRecording, isStarting]);

  const cancelPolish = useCallback(async (): Promise<void> => {
    const sessionId = latestSessionIdRef.current;
    if (!isProcessing || resultMode !== "dictation"
      || !Number.isSafeInteger(sessionId) || sessionId <= 0) return;
    try {
      const pendingStop = pendingStopRef.current;
      if (pendingStop?.sessionId === sessionId) {
        await pendingStop.promise;
        if (sessionId !== latestSessionIdRef.current) return;
      }
      await cancelDictationPolish(sessionId);
    } catch (err) {
      if (sessionId === latestSessionIdRef.current) {
        setError(err instanceof Error ? err.message : String(err));
      }
    }
  }, [isProcessing, resultMode]);

  return {
    polishAuditWarning: auditWarningSession !== null && auditWarningSession === latestDisplayedFinalSessionIdRef.current,
    isStarting, isRecording, isProcessing, startRecording, stopRecording, cancelPolish,
    error, transcriptionResult, setTranscriptionResult,
    originalAsrText, editBaselineText, setEditBaselineText,
    durationSec, charCount, detectedLanguage, editGrabStatus, history,
    timing,
    resultMode,
  };
}
