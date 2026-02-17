import { useState, useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { TranscriptionResult, HistoryItem } from "@/types";

interface UseRecordingReturn {
  isRecording: boolean;
  isProcessing: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<TranscriptionResult | null>;
  error: string | null;
  transcriptionResult: string | null;
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
}

export function useRecording(): UseRecordingReturn {
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transcriptionResult, setTranscriptionResult] = useState<string | null>(null);
  const [history, setHistory] = useState<HistoryItem[]>([]);

  // 监听 recording-state 事件
  useEffect(() => {
    const unlisten = listen<RecordingStatePayload>("recording-state", (e) => {
      setIsRecording(e.payload.isRecording);
      setIsProcessing(e.payload.isProcessing);
      if (e.payload.error) {
        setError(e.payload.error);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // 监听 transcription-result 事件
  useEffect(() => {
    const unlisten = listen<TranscriptionPayload>("transcription-result", (e) => {
      if (!e.payload.interim) {
        const text = e.payload.text;
        setTranscriptionResult(text);
        if (text) {
          setHistory((prev) =>
            [
              {
                id: Date.now().toString(),
                text,
                timestamp: Date.now(),
              },
              ...prev,
            ].slice(0, 20)
          );
        }
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const startRecording = useCallback(async () => {
    setError(null);
    try {
      await invoke("start_recording");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const stopRecording = useCallback(async (): Promise<TranscriptionResult | null> => {
    try {
      await invoke("stop_recording");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
    return null;
  }, []);

  return {
    isRecording,
    isProcessing,
    startRecording,
    stopRecording,
    error,
    transcriptionResult,
    history,
  };
}
