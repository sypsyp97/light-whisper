import { useState, useEffect, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { readLocalStorage } from "@/lib/storage";
import { THEME_STORAGE_KEY } from "@/lib/constants";
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

  // 主题同步：读取初始值 + 监听主窗口的 localStorage 变更
  useEffect(() => {
    const applyTheme = () => {
      const stored = readLocalStorage(THEME_STORAGE_KEY);
      if (stored === "dark") {
        document.documentElement.setAttribute("data-theme", "dark");
      } else if (stored === "light") {
        document.documentElement.setAttribute("data-theme", "light");
      } else {
        const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        document.documentElement.setAttribute("data-theme", prefersDark ? "dark" : "light");
      }
    };

    applyTheme();

    // 监听 localStorage 变更（跨窗口同步）
    const onStorage = (e: StorageEvent) => {
      if (e.key === THEME_STORAGE_KEY) applyTheme();
    };

    // 监听系统主题变更（当设置为"跟随系统"时）
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const onSystemThemeChange = () => applyTheme();

    window.addEventListener("storage", onStorage);
    mediaQuery.addEventListener("change", onSystemThemeChange);
    return () => {
      window.removeEventListener("storage", onStorage);
      mediaQuery.removeEventListener("change", onSystemThemeChange);
    };
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

          const incomingText = event.payload.text || "";
          setText(incomingText);

          if (interim) {
            setFadingOut(false);
            return;
          }

          const finalText = incomingText.trim();
          clearFadeTimer();

          // Empty final text: keep processing hint and fade out directly
          // to avoid the capsule shrinking into a tiny blank pill.
          if (!finalText) {
            setText("");
            setPhase("processing");
            setFadingOut(true);
            return;
          }

          setText(finalText);
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

  const hasText = text.length > 0;
  const indicatorClass =
    phase === "recording"
      ? "subtitle-dot-recording"
      : phase === "processing"
        ? "subtitle-dot-processing"
        : null;
  const hintText =
    phase === "recording"
      ? "正在聆听..."
      : phase === "processing"
        ? "识别中..."
        : null;

  return (
    <div className="subtitle-root">
      <div className={`subtitle-capsule${fadingOut ? " subtitle-fade-out" : ""}`}>
        {indicatorClass && <div className={indicatorClass} />}
        {hasText && <span className="subtitle-text">{text}</span>}
        {!hasText && hintText && <span className="subtitle-hint">{hintText}</span>}
      </div>
    </div>
  );
}
