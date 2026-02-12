import { useState, useEffect, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import "../styles/theme.css";
import "../styles/subtitle.css";

interface RecordingState {
  sessionId?: number;
  isRecording: boolean;
  isProcessing: boolean;
}

interface TranscriptionResult {
  sessionId?: number;
  text: string;
  interim?: boolean;
}

type Phase = "idle" | "recording" | "processing" | "result";
const RESULT_FADE_DELAY_MS = 2000;

export default function SubtitleOverlay() {
  // 初始 "idle"：窗口预创建后隐藏，等待录音事件时切换状态
  const [phase, setPhase] = useState<Phase>("idle");
  const [text, setText] = useState("");
  const [fadingOut, setFadingOut] = useState(false);
  const latestSessionIdRef = useRef(0);
  const fadeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearFadeTimer = useCallback(() => {
    if (fadeTimerRef.current) {
      clearTimeout(fadeTimerRef.current);
      fadeTimerRef.current = null;
    }
  }, []);

  // 主题
  useEffect(() => {
    try {
      const stored = localStorage.getItem("light-whisper-theme");
      if (stored === "dark") {
        document.documentElement.setAttribute("data-theme", "dark");
      } else if (stored === "light") {
        document.documentElement.setAttribute("data-theme", "light");
      } else {
        const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        document.documentElement.setAttribute("data-theme", prefersDark ? "dark" : "light");
      }
    } catch {}
  }, []);

  // 监听录音状态
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<RecordingState>("recording-state", (event) => {
          const { sessionId, isRecording, isProcessing } = event.payload;
          if (typeof sessionId === "number") {
            if (sessionId < latestSessionIdRef.current) return;
            latestSessionIdRef.current = sessionId;
          }

          if (isRecording) {
            clearFadeTimer();
            setFadingOut(false);
            setText("");
            setPhase("recording");
            return;
          }

          if (isProcessing) {
            clearFadeTimer();
            setFadingOut(false);
            setPhase("processing");
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
      clearFadeTimer();
    };
  }, [clearFadeTimer]);

  // 监听转写结果（中间结果 interim=true，最终结果 interim=false）
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<TranscriptionResult>("transcription-result", (event) => {
          const { sessionId, interim } = event.payload;
          if (typeof sessionId === "number") {
            if (sessionId < latestSessionIdRef.current) return;
            latestSessionIdRef.current = sessionId;
          }

          setText(event.payload.text || "");

          if (interim) {
            setFadingOut(false);
            return;
          }

          clearFadeTimer();
          setPhase("result");
          setFadingOut(false);

          const expectedSessionId = latestSessionIdRef.current;
          fadeTimerRef.current = setTimeout(() => {
            if (latestSessionIdRef.current !== expectedSessionId) return;
            setFadingOut(true);
            fadeTimerRef.current = null;
          }, RESULT_FADE_DELAY_MS);
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
      clearFadeTimer();
    };
  }, [clearFadeTimer]);

  return (
    <div className="subtitle-root">
      <div className={`subtitle-capsule${fadingOut ? " subtitle-fade-out" : ""}`}>
        {phase === "recording" && !text && (
          <>
            <div className="subtitle-dot-recording" />
            <span className="subtitle-hint">正在聆听...</span>
          </>
        )}
        {phase === "recording" && text && (
          <>
            <div className="subtitle-dot-recording" />
            <span className="subtitle-text">{text}</span>
          </>
        )}
        {phase === "processing" && !text && (
          <>
            <div className="subtitle-dot-processing" />
            <span className="subtitle-hint">识别中...</span>
          </>
        )}
        {phase === "processing" && text && (
          <>
            <div className="subtitle-dot-processing" />
            <span className="subtitle-text">{text}</span>
          </>
        )}
        {phase === "result" && (
          <span className="subtitle-text">{text}</span>
        )}
      </div>
    </div>
  );
}
