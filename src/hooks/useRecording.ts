import { useState, useRef, useCallback, useEffect } from "react";
import { toast } from "sonner";
import { emit } from "@tauri-apps/api/event";
import {
  hideSubtitleWindow,
  pasteText,
  showSubtitleWindow,
  transcribeAudio,
} from "@/api/tauri";
import { convertToWav, arrayBufferToBase64 } from "@/lib/audio";
import { INPUT_METHOD_KEY } from "@/lib/constants";
import { readLocalStorage } from "@/lib/storage";
import type { TranscriptionResult, HistoryItem } from "@/types";

interface UseRecordingReturn {
  isRecording: boolean;
  isProcessing: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<TranscriptionResult | null>;
  error: string | null;
  /** The raw transcription text from FunASR (null until a transcription completes). */
  transcriptionResult: string | null;
  /** Recent transcription history (newest first, max 20 items). */
  history: HistoryItem[];
}

interface RecordingStateEventPayload {
  sessionId: number;
  isRecording: boolean;
  isProcessing: boolean;
}

interface TranscriptionEventPayload {
  sessionId: number;
  text: string;
  interim: boolean;
}

const MIN_AUDIO_DURATION_SEC = 0.5;
const INTERIM_INTERVAL_MIN_MS = 140;
const INTERIM_INTERVAL_BASE_MS = 220;
const INTERIM_INTERVAL_MAX_MS = 460;
const INTERIM_INTERVAL_DOWN_STEP_MS = 24;
const INTERIM_INTERVAL_UP_STEP_MS = 42;
const INTERIM_HEAVY_COST_MS = 420;
const INTERIM_LIGHT_COST_MS = 180;
const INTERIM_MIN_BYTES_GROWTH = 2 * 1024;
const RESULT_HIDE_DELAY_MS = 2500;
const EMPTY_RESULT_HIDE_DELAY_MS = 360;
const PASTE_DELAY_MS = 260;
const PASTE_RETRY_INTERVAL_MS = 140;
const RECORDER_TIMESLICE_MS = 80;
const STOP_SAFETY_TIMEOUT_MS = 15000;
const WAV_HEADER_BYTES = 44;
const WAV_BYTES_PER_SECOND = 32000; // 16kHz * 16bit * mono

function clearTimer(ref: { current: ReturnType<typeof setTimeout> | null }): void {
  if (!ref.current) return;
  clearTimeout(ref.current);
  ref.current = null;
}

function getWavDurationSeconds(buffer: ArrayBuffer): number {
  return Math.max(0, (buffer.byteLength - WAV_HEADER_BYTES) / WAV_BYTES_PER_SECOND);
}

function toErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

function getInputMethodPreference(): "sendInput" | "clipboard" {
  return readLocalStorage(INPUT_METHOD_KEY) === "clipboard"
    ? "clipboard"
    : "sendInput";
}

/**
 * React hook that manages audio recording and transcription via FunASR.
 *
 * Features:
 * - Higher-frequency interim transcription for smoother streaming feel
 * - Subtitle window lifecycle management
 * - Reliable queued paste to prevent missed results across rapid sessions
 */
export function useRecording(): UseRecordingReturn {
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transcriptionResult, setTranscriptionResult] = useState<string | null>(null);
  const [history, setHistory] = useState<HistoryItem[]>([]);

  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const streamRef = useRef<MediaStream | null>(null);

  const activeSessionIdRef = useRef<number | null>(null);
  const sessionSequenceRef = useRef(0);
  const isStartingRef = useRef(false);
  const isStoppingRef = useRef(false);
  const isRecordingStateRef = useRef(false);
  const isProcessingStateRef = useRef(false);

  const totalChunkBytesRef = useRef(0);
  const lastInterimBytesRef = useRef(0);
  const interimIntervalRef = useRef(INTERIM_INTERVAL_BASE_MS);

  const periodicRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const busyRef = useRef(false);
  const hideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scheduleInterimLoopRef = useRef<
    (sessionId: number, mimeType: string, delayMs: number) => void
  >(() => {});

  const pasteTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pasteQueueRef = useRef<string[]>([]);
  const pasteWorkerBusyRef = useRef(false);
  const schedulePasteWorkerRef = useRef<(delay: number) => void>(() => {});

  const setRecordingState = useCallback((value: boolean) => {
    isRecordingStateRef.current = value;
    setIsRecording(value);
  }, []);

  const setProcessingState = useCallback((value: boolean) => {
    isProcessingStateRef.current = value;
    setIsProcessing(value);
  }, []);

  const isSessionActive = useCallback((sessionId: number) => {
    return activeSessionIdRef.current === sessionId;
  }, []);

  const emitRecordingState = useCallback((payload: RecordingStateEventPayload) => {
    void emit("recording-state", payload).catch(() => undefined);
  }, []);

  const emitTranscription = useCallback((payload: TranscriptionEventPayload) => {
    void emit("transcription-result", payload).catch(() => undefined);
  }, []);

  const clearTimers = useCallback((options?: { preserveHide?: boolean }) => {
    clearTimer(periodicRef);
    if (!options?.preserveHide) {
      clearTimer(hideTimerRef);
    }
  }, []);

  const clearPasteTimer = useCallback(() => {
    clearTimer(pasteTimerRef);
  }, []);

  const releaseMediaResources = useCallback(() => {
    busyRef.current = false;

    if (mediaRecorderRef.current) {
      mediaRecorderRef.current.ondataavailable = null;
      mediaRecorderRef.current.onstop = null;
      mediaRecorderRef.current.onerror = null;
      mediaRecorderRef.current = null;
    }

    if (streamRef.current) {
      streamRef.current.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
    }

    chunksRef.current = [];
    totalChunkBytesRef.current = 0;
    lastInterimBytesRef.current = 0;
    interimIntervalRef.current = INTERIM_INTERVAL_BASE_MS;
  }, []);

  const hideSubtitleSilently = useCallback(() => {
    void hideSubtitleWindow().catch(() => undefined);
  }, []);

  const processPasteQueue = useCallback(async () => {
    if (pasteWorkerBusyRef.current) return;
    if (pasteQueueRef.current.length === 0) return;

    pasteWorkerBusyRef.current = true;
    try {
      if (isRecordingStateRef.current || isProcessingStateRef.current) {
        return;
      }

      const nextText = pasteQueueRef.current.shift();
      if (!nextText) return;

      try {
        await pasteText(nextText, getInputMethodPreference());
      } catch {
        toast.error("文字输入失败，结果已保留，请手动复制");
      }
    } finally {
      pasteWorkerBusyRef.current = false;
      if (pasteQueueRef.current.length > 0) {
        const wait =
          isRecordingStateRef.current || isProcessingStateRef.current
            ? PASTE_RETRY_INTERVAL_MS
            : 0;
        schedulePasteWorkerRef.current(wait);
      }
    }
  }, []);

  const schedulePasteWorker = useCallback((delay: number) => {
    if (pasteTimerRef.current) return;
    pasteTimerRef.current = setTimeout(() => {
      pasteTimerRef.current = null;
      void processPasteQueue();
    }, Math.max(0, delay));
  }, [processPasteQueue]);

  schedulePasteWorkerRef.current = schedulePasteWorker;

  const enqueuePasteText = useCallback((text: string) => {
    if (!text) return;
    pasteQueueRef.current.push(text);
    schedulePasteWorkerRef.current(PASTE_DELAY_MS);
  }, []);

  const runInterimTranscription = useCallback(
    async (
      sessionId: number,
      mimeType: string
    ): Promise<{ executed: boolean; elapsedMs: number }> => {
      if (busyRef.current) return { executed: false, elapsedMs: 0 };
      if (isStoppingRef.current) return { executed: false, elapsedMs: 0 };
      if (!isSessionActive(sessionId)) return { executed: false, elapsedMs: 0 };
      if (totalChunkBytesRef.current === 0) return { executed: false, elapsedMs: 0 };
      if (
        totalChunkBytesRef.current - lastInterimBytesRef.current <
        INTERIM_MIN_BYTES_GROWTH
      ) {
        return { executed: false, elapsedMs: 0 };
      }

      busyRef.current = true;
      const runStart = performance.now();
      try {
        const blob = new Blob([...chunksRef.current], {
          type: mimeType || "audio/webm",
        });
        if (blob.size === 0) return { executed: false, elapsedMs: 0 };

        const wavBuffer = await convertToWav(blob);
        if (!isSessionActive(sessionId)) return { executed: false, elapsedMs: 0 };

        const duration = getWavDurationSeconds(wavBuffer);
        if (duration < MIN_AUDIO_DURATION_SEC) return { executed: false, elapsedMs: 0 };

        lastInterimBytesRef.current = totalChunkBytesRef.current;
        const result = await transcribeAudio(arrayBufferToBase64(wavBuffer));
        if (!isSessionActive(sessionId)) return { executed: false, elapsedMs: 0 };

        if (result.success && result.text) {
          emitTranscription({
            sessionId,
            text: result.text,
            interim: true,
          });
        }

        return { executed: true, elapsedMs: performance.now() - runStart };
      } catch {
        // 中间转写失败静默忽略
        return { executed: false, elapsedMs: 0 };
      } finally {
        busyRef.current = false;
      }
    },
    [emitTranscription, isSessionActive]
  );

  const getNextInterimDelay = useCallback((stats: { executed: boolean; elapsedMs: number }) => {
    let next = interimIntervalRef.current;

    if (!stats.executed) {
      next = Math.max(
        INTERIM_INTERVAL_MIN_MS,
        Math.min(INTERIM_INTERVAL_BASE_MS, next - 8)
      );
      interimIntervalRef.current = next;
      return next;
    }

    if (stats.elapsedMs >= INTERIM_HEAVY_COST_MS) {
      next = Math.min(INTERIM_INTERVAL_MAX_MS, next + INTERIM_INTERVAL_UP_STEP_MS);
    } else if (stats.elapsedMs <= INTERIM_LIGHT_COST_MS) {
      next = Math.max(INTERIM_INTERVAL_MIN_MS, next - INTERIM_INTERVAL_DOWN_STEP_MS);
    } else if (next > INTERIM_INTERVAL_BASE_MS) {
      next = Math.max(INTERIM_INTERVAL_BASE_MS, next - 8);
    } else if (next < INTERIM_INTERVAL_BASE_MS) {
      next = Math.min(INTERIM_INTERVAL_BASE_MS, next + 4);
    }

    interimIntervalRef.current = next;
    return next;
  }, []);

  const scheduleInterimLoop = useCallback(
    (sessionId: number, mimeType: string, delayMs: number) => {
      if (periodicRef.current) {
        clearTimeout(periodicRef.current);
        periodicRef.current = null;
      }

      periodicRef.current = setTimeout(() => {
        periodicRef.current = null;

        if (
          !isSessionActive(sessionId) ||
          !isRecordingStateRef.current
        ) {
          return;
        }

        void (async () => {
          const stats = await runInterimTranscription(sessionId, mimeType);
          if (
            !isSessionActive(sessionId) ||
            !isRecordingStateRef.current
          ) {
            return;
          }
          const nextDelay = getNextInterimDelay(stats);
          scheduleInterimLoopRef.current(sessionId, mimeType, nextDelay);
        })();
      }, Math.max(INTERIM_INTERVAL_MIN_MS, delayMs));
    },
    [getNextInterimDelay, isSessionActive, runInterimTranscription]
  );

  scheduleInterimLoopRef.current = scheduleInterimLoop;

  const resetAfterStop = useCallback((options?: { preserveHide?: boolean }) => {
    activeSessionIdRef.current = null;
    isStartingRef.current = false;
    isStoppingRef.current = false;
    setRecordingState(false);
    setProcessingState(false);
    clearTimers({ preserveHide: options?.preserveHide });
    releaseMediaResources();
  }, [clearTimers, releaseMediaResources, setProcessingState, setRecordingState]);

  const startRecording = useCallback(async () => {
    if (
      isStartingRef.current ||
      isStoppingRef.current ||
      activeSessionIdRef.current !== null ||
      isRecording ||
      isProcessing
    ) {
      return;
    }

    isStartingRef.current = true;
    const sessionId = ++sessionSequenceRef.current;
    activeSessionIdRef.current = sessionId;

    try {
      setError(null);
      clearTimers();
      releaseMediaResources();

      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          channelCount: 1,
          sampleRate: 16000,
          echoCancellation: true,
          noiseSuppression: true,
        },
      });

      if (!isSessionActive(sessionId)) {
        stream.getTracks().forEach((track) => track.stop());
        return;
      }

      streamRef.current = stream;

      const preferredTypes = ["audio/webm;codecs=opus", "audio/webm"];
      const mimeType = preferredTypes.find((t) => MediaRecorder.isTypeSupported(t));
      const recorder = mimeType
        ? new MediaRecorder(stream, { mimeType })
        : new MediaRecorder(stream);

      recorder.ondataavailable = (event: BlobEvent) => {
        if (!isSessionActive(sessionId)) return;
        if (event.data.size > 0) {
          chunksRef.current.push(event.data);
          totalChunkBytesRef.current += event.data.size;
        }
      };

      mediaRecorderRef.current = recorder;
      recorder.start(RECORDER_TIMESLICE_MS);
      setRecordingState(true);
      setProcessingState(false);

      emitRecordingState({ sessionId, isRecording: true, isProcessing: false });
      void showSubtitleWindow().catch(() => undefined);

      scheduleInterimLoopRef.current(
        sessionId,
        recorder.mimeType,
        INTERIM_INTERVAL_BASE_MS
      );
    } catch (err) {
      setError(toErrorMessage(err, "启动录音失败"));
      if (isSessionActive(sessionId)) {
        activeSessionIdRef.current = null;
      }
      clearTimers();
      releaseMediaResources();
      setRecordingState(false);
      setProcessingState(false);
      hideSubtitleSilently();
    } finally {
      if (!isSessionActive(sessionId)) {
        clearTimers();
        releaseMediaResources();
      }
      isStartingRef.current = false;
    }
  }, [
    clearTimers,
    emitRecordingState,
    hideSubtitleSilently,
    isProcessing,
    isRecording,
    isSessionActive,
    releaseMediaResources,
    setProcessingState,
    setRecordingState,
  ]);

  const stopRecording = useCallback(async (): Promise<TranscriptionResult | null> => {
    if (isStoppingRef.current) return null;

    clearTimers({ preserveHide: true });

    const sessionId = activeSessionIdRef.current;
    const recorder = mediaRecorderRef.current;
    if (sessionId === null || !recorder) {
      resetAfterStop();
      hideSubtitleSilently();
      return null;
    }

    isStoppingRef.current = true;

    return new Promise<TranscriptionResult | null>((resolve) => {
      let settled = false;
      const settle = (
        result: TranscriptionResult | null,
        options?: { hideNow?: boolean }
      ) => {
        if (settled) return;
        settled = true;
        clearTimeout(safetyTimeout);
        const preserveHide = result !== null && !options?.hideNow;
        resetAfterStop({ preserveHide });
        if (options?.hideNow) {
          hideSubtitleSilently();
        }
        resolve(result);
      };

      const safetyTimeout = setTimeout(() => {
        setError("停止录音超时，请重试");
        settle(null, { hideNow: true });
      }, STOP_SAFETY_TIMEOUT_MS);

      recorder.onstop = async () => {
        if (!isSessionActive(sessionId)) {
          settle(null);
          return;
        }

        setRecordingState(false);
        setProcessingState(true);
        setError(null);
        emitRecordingState({ sessionId, isRecording: false, isProcessing: true });

        try {
          const blob = new Blob([...chunksRef.current], {
            type: recorder.mimeType || "audio/webm",
          });

          if (blob.size === 0) {
            setError("未录制到音频数据");
            settle(null, { hideNow: true });
            return;
          }

          const wavBuffer = await convertToWav(blob);
          if (!isSessionActive(sessionId)) {
            settle(null);
            return;
          }

          const audioDurationSec = getWavDurationSeconds(wavBuffer);
          if (audioDurationSec < MIN_AUDIO_DURATION_SEC) {
            setError("录音时间过短，请至少录制 0.5 秒");
            settle(null, { hideNow: true });
            return;
          }

          const audioBase64 = arrayBufferToBase64(wavBuffer);
          const result = await transcribeAudio(audioBase64);
          if (!isSessionActive(sessionId)) {
            settle(null);
            return;
          }

          if (!result.success) {
            setError(result.error || "语音识别失败");
            settle(null, { hideNow: true });
            return;
          }

          const finalText = (result.text ?? "").trim();
          setTranscriptionResult(finalText);
          setProcessingState(false);

          emitRecordingState({ sessionId, isRecording: false, isProcessing: false });
          emitTranscription({
            sessionId,
            text: finalText,
            interim: false,
          });

          if (finalText) {
            setHistory((prev) => [{
              id: Date.now().toString(),
              text: finalText,
              timestamp: Date.now(),
            }, ...prev].slice(0, 20));
            enqueuePasteText(finalText);
          }

          const hideDelayMs = finalText ? RESULT_HIDE_DELAY_MS : EMPTY_RESULT_HIDE_DELAY_MS;
          hideTimerRef.current = setTimeout(() => {
            if (sessionSequenceRef.current !== sessionId) return;
            hideSubtitleSilently();
            hideTimerRef.current = null;
          }, hideDelayMs);

          settle(result);
        } catch (err) {
          setError(toErrorMessage(err, "处理失败"));
          settle(null, { hideNow: true });
        }
      };

      try {
        if (recorder.state !== "inactive") {
          recorder.stop();
        } else {
          setError("录音器状态异常，已取消本次录音");
          settle(null, { hideNow: true });
        }
      } catch (stopErr) {
        setError(toErrorMessage(stopErr, "停止录音失败"));
        settle(null, { hideNow: true });
      }
    });
  }, [
    emitRecordingState,
    emitTranscription,
    enqueuePasteText,
    hideSubtitleSilently,
    isSessionActive,
    resetAfterStop,
    clearTimers,
  ]);

  useEffect(() => {
    return () => {
      clearPasteTimer();
      pasteQueueRef.current = [];
      resetAfterStop();
      hideSubtitleSilently();
    };
  }, [clearPasteTimer, hideSubtitleSilently, resetAfterStop]);

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
