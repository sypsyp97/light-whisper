import { useState, useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
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

export function useRecording(): UseRecordingReturn {
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transcriptionResult, setTranscriptionResult] = useState<string | null>(null);
  const [durationSec, setDurationSec] = useState<number | null>(null);
  const [charCount, setCharCount] = useState<number | null>(null);
  const [history, setHistory] = useState<HistoryItem[]>([]);

  // 监听 recording-state 事件
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<RecordingStatePayload>("recording-state", (e) => {
          setIsRecording(e.payload.isRecording);
          setIsProcessing(e.payload.isProcessing);
          if (e.payload.error) {
            setError(e.payload.error);
          }
        });

        if (disposed && unlisten) {
          unlisten();
          unlisten = null;
        }
      } catch {
        // 忽略事件监听初始化失败
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 监听 transcription-result 事件
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<TranscriptionPayload>("transcription-result", (e) => {
          if (!e.payload.interim) {
            const text = e.payload.text;
            setTranscriptionResult(text);
            setDurationSec(e.payload.durationSec ?? null);
            setCharCount(e.payload.charCount ?? null);
            if (text) {
              const now = Date.now();
              setHistory((prev) =>
                [
                  {
                    id: now.toString(),
                    text,
                    timestamp: now,
                    timeDisplay: new Date(now).toLocaleTimeString(),
                  },
                  ...prev,
                ].slice(0, 20)
              );
            }
          }
        });

        if (disposed && unlisten) {
          unlisten();
          unlisten = null;
        }
      } catch {
        // 忽略事件监听初始化失败
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 监听 AI 润色状态
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<{ status: string; original: string; polished: string; error: string }>(
          "ai-polish-status",
          (e) => {
            const { status, error: errMsg } = e.payload;
            if (status === "applied") {
              toast.success("AI 润色已应用", { duration: 1500 });
            } else if (status === "error") {
              toast.error(`AI 润色失败: ${errMsg}`, { duration: 2500 });
            }
          }
        );

        if (disposed && unlisten) {
          unlisten();
          unlisten = null;
        }
      } catch {
        // 忽略
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
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
    durationSec,
    charCount,
    history,
  };
}
