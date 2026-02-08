import { useState, useRef, useCallback } from "react";
import { transcribeAudio } from "@/api/funasr";
import { pasteText } from "@/api/clipboard";
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
 * Write a WAV file header into a DataView.
 * Produces a standard 16-bit mono PCM WAV at the given sample rate.
 */
function writeWavHeader(
  view: DataView,
  numSamples: number,
  sampleRate: number
): void {
  const numChannels = 1;
  const bitsPerSample = 16;
  const byteRate = sampleRate * numChannels * (bitsPerSample / 8);
  const blockAlign = numChannels * (bitsPerSample / 8);
  const dataSize = numSamples * numChannels * (bitsPerSample / 8);

  // "RIFF" chunk descriptor
  writeString(view, 0, "RIFF");
  view.setUint32(4, 36 + dataSize, true);
  writeString(view, 8, "WAVE");

  // "fmt " sub-chunk
  writeString(view, 12, "fmt ");
  view.setUint32(16, 16, true); // sub-chunk size (PCM = 16)
  view.setUint16(20, 1, true); // audio format (PCM = 1)
  view.setUint16(22, numChannels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, byteRate, true);
  view.setUint16(32, blockAlign, true);
  view.setUint16(34, bitsPerSample, true);

  // "data" sub-chunk
  writeString(view, 36, "data");
  view.setUint32(40, dataSize, true);
}

function writeString(view: DataView, offset: number, str: string): void {
  for (let i = 0; i < str.length; i++) {
    view.setUint8(offset + i, str.charCodeAt(i));
  }
}

/**
 * Convert an AudioBuffer (any sample rate) to a 16-bit mono WAV
 * ArrayBuffer re-sampled to the target sample rate.
 */
function audioBufferToWav(buffer: AudioBuffer, targetSampleRate = 16000): ArrayBuffer {
  // Down-mix to mono (average all channels)
  const numChannels = buffer.numberOfChannels;
  let channelData: Float32Array;

  if (numChannels <= 1) {
    channelData = buffer.getChannelData(0);
  } else {
    const mixed = new Float32Array(buffer.length);
    for (let ch = 0; ch < numChannels; ch++) {
      const data = buffer.getChannelData(ch);
      for (let i = 0; i < data.length; i++) {
        mixed[i] += data[i];
      }
    }
    for (let i = 0; i < mixed.length; i++) {
      mixed[i] /= numChannels;
    }
    channelData = mixed;
  }
  const sourceSampleRate = buffer.sampleRate;

  // Resample if necessary
  let samples: Float32Array;
  if (sourceSampleRate === targetSampleRate) {
    samples = channelData;
  } else {
    const ratio = sourceSampleRate / targetSampleRate;
    const newLength = Math.round(channelData.length / ratio);
    samples = new Float32Array(newLength);
    for (let i = 0; i < newLength; i++) {
      const srcIndex = i * ratio;
      const low = Math.floor(srcIndex);
      const high = Math.min(low + 1, channelData.length - 1);
      const frac = srcIndex - low;
      samples[i] = channelData[low] * (1 - frac) + channelData[high] * frac;
    }
  }

  const numSamples = samples.length;
  const headerSize = 44;
  const wavBuffer = new ArrayBuffer(headerSize + numSamples * 2);
  const view = new DataView(wavBuffer);

  writeWavHeader(view, numSamples, targetSampleRate);

  // Write PCM samples (clamp to int16 range)
  let offset = headerSize;
  for (let i = 0; i < numSamples; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    const val = s < 0 ? s * 0x8000 : s * 0x7fff;
    view.setInt16(offset, val, true);
    offset += 2;
  }

  return wavBuffer;
}

/**
 * Convert a WebM Blob captured by MediaRecorder into a WAV ArrayBuffer
 * by decoding through the Web Audio API and re-encoding as 16 kHz mono PCM.
 */
async function convertToWav(blob: Blob): Promise<ArrayBuffer> {
  const arrayBuffer = await blob.arrayBuffer();
  let audioCtx: AudioContext;
  try {
    audioCtx = new AudioContext({ sampleRate: 16000 });
  } catch {
    // Fallback to default sample rate if 16k is unsupported
    audioCtx = new AudioContext();
  }

  try {
    const audioBuffer = await audioCtx.decodeAudioData(arrayBuffer);
    return audioBufferToWav(audioBuffer, 16000);
  } finally {
    await audioCtx.close();
  }
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
      const recorder = mediaRecorderRef.current!;

      recorder.onstop = async () => {
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

          const audioData = Array.from(new Uint8Array(wavBuffer));

          // Send to FunASR backend
          const result = await transcribeAudio(audioData);

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
                inputMethod = localStorage.getItem("light-whisper-input-method") as "sendInput" | "clipboard" | null;
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
          setIsRecording(false);
          setIsProcessing(false);
          cleanup();
          resolve(null);
        }
      } catch (stopErr) {
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
