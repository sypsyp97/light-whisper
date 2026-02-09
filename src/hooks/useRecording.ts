import { useState, useRef, useCallback } from "react";
import { transcribeAudio } from "@/api/funasr";
import { pasteText } from "@/api/clipboard";
import { convertToWav, arrayBufferToBase64 } from "@/lib/audio";
import { INPUT_METHOD_KEY } from "@/lib/constants";
import type { TranscriptionResult } from "@/types";

interface UseRecordingReturn {
  isRecording: boolean;
  isProcessing: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<TranscriptionResult | null>;
  cancelRecording: () => void;
  error: string | null;
  /** The raw transcription text from FunASR (null until a transcription completes). */
  transcriptionResult: string | null;
}

/**
 * React hook that manages audio recording and transcription via FunASR.
 */
export function useRecording(): UseRecordingReturn {
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transcriptionResult, setTranscriptionResult] = useState<string | null>(null);

  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const streamRef = useRef<MediaStream | null>(null);
  const cancelledRef = useRef(false);

  const cleanup = useCallback(() => {
    if (mediaRecorderRef.current) {
      if (mediaRecorderRef.current.state !== "inactive") {
        mediaRecorderRef.current.stop();
      }
      mediaRecorderRef.current = null;
    }
    if (streamRef.current) {
      streamRef.current.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
    }
    chunksRef.current = [];
  }, []);

  const startRecording = useCallback(async () => {
    try {
      setError(null);
      setTranscriptionResult(null);
      cancelledRef.current = false;

      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          channelCount: 1,
          sampleRate: 16000,
          echoCancellation: true,
          noiseSuppression: true,
        },
      });

      streamRef.current = stream;
      chunksRef.current = [];

      const preferredTypes = ["audio/webm;codecs=opus", "audio/webm"];
      const mimeType = preferredTypes.find((t) =>
        MediaRecorder.isTypeSupported(t)
      );

      const recorder = mimeType
        ? new MediaRecorder(stream, { mimeType })
        : new MediaRecorder(stream);

      recorder.ondataavailable = (event: BlobEvent) => {
        if (event.data.size > 0) {
          chunksRef.current.push(event.data);
        }
      };

      mediaRecorderRef.current = recorder;
      recorder.start(100); // collect data every 100ms
      setIsRecording(true);
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "启动录音失败";
      setError(message);
      cleanup();
    }
  }, [cleanup]);

  const stopRecording = useCallback(async (): Promise<TranscriptionResult | null> => {
    if (!mediaRecorderRef.current || cancelledRef.current) {
      cleanup();
      setIsRecording(false);
      return null;
    }

    return new Promise<TranscriptionResult | null>((resolve) => {
      // 超时保护：如果 onstop 在 15 秒内未触发，自动 resolve
      const safetyTimeout = setTimeout(() => {
        cleanup();
        setIsRecording(false);
        setIsProcessing(false);
        resolve(null);
      }, 15000);

      const recorder = mediaRecorderRef.current!;

      recorder.onstop = async () => {
        clearTimeout(safetyTimeout);
        setIsRecording(false);
        setIsProcessing(true);
        setError(null);

        try {
          const blob = new Blob(chunksRef.current, {
            type: recorder.mimeType,
          });

          if (blob.size === 0) {
            setError("未录制到音频数据");
            setIsProcessing(false);
            cleanup();
            resolve(null);
            return;
          }

          // Convert WebM to WAV (16 kHz mono PCM)
          const wavBuffer = await convertToWav(blob);

          // 检查音频时长：WAV header 44 bytes, 16kHz 16-bit mono = 32000 bytes/s
          const audioDurationSec = (wavBuffer.byteLength - 44) / 32000;
          if (audioDurationSec < 0.5) {
            setError("录音时间过短，请至少录制 0.5 秒");
            setIsProcessing(false);
            cleanup();
            resolve(null);
            return;
          }

          const audioBase64 = arrayBufferToBase64(wavBuffer);

          // Send to FunASR backend
          const result = await transcribeAudio(audioBase64);

          if (!result.success) {
            setError(result.error || "语音识别失败");
            setIsProcessing(false);
            cleanup();
            resolve(null);
            return;
          }

          // Store the raw transcription result
          setTranscriptionResult(result.text);
          setIsProcessing(false);

          // Auto-paste: write to clipboard and simulate Ctrl+V
          if (result.text) {
            try {
              let inputMethod: "sendInput" | "clipboard" | null = null;
              try {
                inputMethod = localStorage.getItem(INPUT_METHOD_KEY) as "sendInput" | "clipboard" | null;
              } catch { /* localStorage 不可用 */ }
              await pasteText(result.text, inputMethod ?? "sendInput");
            } catch (pasteErr) {
              console.warn("Auto-paste failed:", pasteErr);
            }
          }

          cleanup();
          resolve(result);
        } catch (err) {
          const message =
            err instanceof Error ? err.message : "处理失败";
          setError(message);
          setIsProcessing(false);
          cleanup();
          resolve(null);
        }
      };

      try {
        if (recorder.state !== "inactive") {
          recorder.stop();
        } else {
          clearTimeout(safetyTimeout);
          setIsRecording(false);
          setIsProcessing(false);
          cleanup();
          resolve(null);
        }
      } catch (stopErr) {
        clearTimeout(safetyTimeout);
        const message =
          stopErr instanceof Error ? stopErr.message : "停止录音失败";
        setError(message);
        setIsRecording(false);
        setIsProcessing(false);
        cleanup();
        resolve(null);
      }
    });
  }, [cleanup]);

  const cancelRecording = useCallback(() => {
    cancelledRef.current = true;
    setIsRecording(false);
    setIsProcessing(false);
    setError(null);
    cleanup();
  }, [cleanup]);

  return {
    isRecording,
    isProcessing,
    startRecording,
    stopRecording,
    cancelRecording,
    error,
    transcriptionResult,
  };
}
