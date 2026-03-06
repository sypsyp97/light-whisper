import { useState, useCallback, useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import type { TranscriptionResult, HistoryItem } from "@/types";

interface UseRecordingReturn {
  isRecording: boolean;
  isProcessing: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<TranscriptionResult | null>;
  error: string | null;
  transcriptionResult: string | null;
  setTranscriptionResult: (text: string) => void;
  originalAsrText: string | null;
  durationSec: number | null;
  charCount: number | null;
  history: HistoryItem[];
}

interface RecordingStatePayload {
  sessionId: number;
  isRecording: boolean;
  isProcessing: boolean;
  error?: string;
}

interface TranscriptionPayload {
  sessionId: number;
  text: string;
  interim: boolean;
  durationSec?: number;
  charCount?: number;
}

/** 封装 Tauri 事件监听的 useEffect 样板 */
function useTauriEvent<T>(event: string, handler: (payload: T) => void) {
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let disposed = false;

    listen<T>(event, (e) => handler(e.payload))
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
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [event]);
}

export function useRecording(): UseRecordingReturn {
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transcriptionResult, setTranscriptionResult] = useState<string | null>(null);
  const [originalAsrText, setOriginalAsrText] = useState<string | null>(null);
  const [durationSec, setDurationSec] = useState<number | null>(null);
  const [charCount, setCharCount] = useState<number | null>(null);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const latestSessionIdRef = useRef(0);
  const latestDisplayedFinalSessionIdRef = useRef(0);

  useTauriEvent<RecordingStatePayload>("recording-state", (payload) => {
    const sessionId = Number(payload.sessionId || 0);
    if (sessionId < latestSessionIdRef.current) return;
    latestSessionIdRef.current = sessionId;
    setIsRecording(payload.isRecording);
    setIsProcessing(payload.isProcessing);
    setError(payload.error ?? null);
  });

  useTauriEvent<{ message: string; sessionId?: number }>("recording-error", (payload) => {
    const sessionId = Number(payload.sessionId || 0);
    if (sessionId > 0 && sessionId < latestSessionIdRef.current) return;
    const message = payload.message?.trim();
    if (message) setError(message);
  });

  useTauriEvent<TranscriptionPayload>("transcription-result", (payload) => {
    if (payload.interim) return;
    const sessionId = Number(payload.sessionId || 0);
    const { text } = payload;
    const now = Date.now();
    const historyId = sessionId > 0 ? `session-${sessionId}` : `session-local-${now}`;

    if (sessionId >= latestDisplayedFinalSessionIdRef.current) {
      latestDisplayedFinalSessionIdRef.current = sessionId;
      setTranscriptionResult(text);
      setOriginalAsrText(text);
      setDurationSec(payload.durationSec ?? null);
      setCharCount(payload.charCount ?? null);
    }

    if (text.trim()) {
      setHistory((prev) =>
        [
          {
            id: historyId, text, originalText: text,
            timestamp: now, timeDisplay: new Date(now).toLocaleTimeString(),
          },
          ...prev.filter((item) => item.id !== historyId),
        ].slice(0, 20)
      );
    }
  });

  useTauriEvent<{ status: string; error: string }>("ai-polish-status", ({ status, error: errMsg }) => {
    if (status === "applied") toast.success("AI 润色已应用", { duration: 1500 });
    else if (status === "error") toast.error(`AI 润色失败: ${errMsg}`, { duration: 2500 });
  });

  const startRecording = useCallback(async () => {
    setError(null);
    try {
      const sessionId = await invoke<number>("start_recording");
      if (Number.isFinite(sessionId) && sessionId > latestSessionIdRef.current) {
        latestSessionIdRef.current = sessionId;
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const stopRecording = useCallback(async (): Promise<TranscriptionResult | null> => {
    if (isRecording) {
      setIsRecording(false);
      setIsProcessing(true);
    }
    try {
      await invoke("stop_recording");
    } catch (err) {
      if (isRecording) setIsProcessing(false);
      setError(err instanceof Error ? err.message : String(err));
    }
    return null;
  }, [isRecording]);

  return {
    isRecording, isProcessing, startRecording, stopRecording,
    error, transcriptionResult, setTranscriptionResult,
    originalAsrText, durationSec, charCount, history,
  };
}
