import { useState, useEffect, useRef, useCallback, type MouseEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { copyToClipboard, hideSubtitleWindow } from "@/api/tauri";
import { readLocalStorage } from "@/lib/storage";
import { THEME_STORAGE_KEY } from "@/lib/constants";
import "../styles/theme.css";
import "../styles/subtitle.css";

interface RecordingState {
  sessionId?: number;
  isRecording: boolean;
  isProcessing: boolean;
  mode?: "dictation" | "assistant";
}

interface TranscriptionResult {
  sessionId?: number;
  text: string;
  interim?: boolean;
  polished?: boolean;
  mode?: "dictation" | "assistant";
}

type Phase = "idle" | "recording" | "processing" | "polishing" | "result";
const RESULT_FADE_DELAY_MS = 2000;

export default function SubtitleOverlay() {
  // 初始 "idle"：窗口预创建后隐藏，等待录音事件时切换状态
  const [phase, setPhase] = useState<Phase>("idle");
  const [text, setText] = useState("");
  const [fadingOut, setFadingOut] = useState(false);
  const [polishFlash, setPolishFlash] = useState(false);
  const [mode, setMode] = useState<"dictation" | "assistant">("dictation");
  const [assistantCopied, setAssistantCopied] = useState(false);
  const latestSessionIdRef = useRef(0);
  const fadeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const assistantPanelActive = mode === "assistant" && (phase === "polishing" || phase === "result");
  const interactiveAssistantResult = mode === "assistant" && phase === "result" && text.trim().length > 0;

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
          const { sessionId, isRecording, isProcessing, mode } = event.payload;
          if (typeof sessionId === "number") {
            if (sessionId < latestSessionIdRef.current) return;
            latestSessionIdRef.current = sessionId;
          }
          setMode(mode ?? "dictation");

          if (isRecording) {
            clearFadeTimer();
            setFadingOut(false);
            setText("");
            setAssistantCopied(false);
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

  // 监听 AI 润色状态（含流式进度）
  const [streamTokens, setStreamTokens] = useState(0);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<{ status: string; tokens?: number; sessionId?: number }>("ai-polish-status", (event) => {
          const { status, tokens, sessionId } = event.payload;
          if (typeof sessionId === "number" && sessionId < latestSessionIdRef.current) return;
          if (status === "polishing") {
            clearFadeTimer();
            setFadingOut(false);
            setPolishFlash(false);
            setStreamTokens(0);
            setPhase("polishing");
          } else if (status === "fallback") {
            clearFadeTimer();
            setFadingOut(false);
            setStreamTokens(0);
            setPhase("polishing");
          } else if (status === "streaming" && typeof tokens === "number" && tokens > 0) {
            setStreamTokens(tokens);
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
  }, [clearFadeTimer]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<{ sessionId?: number; chunk?: string; status?: string }>("assistant-stream", (event) => {
          const { sessionId, chunk, status } = event.payload;
          if (typeof sessionId === "number") {
            if (sessionId < latestSessionIdRef.current) return;
            latestSessionIdRef.current = sessionId;
          }

          setMode("assistant");

          if (status === "started") {
            clearFadeTimer();
            setFadingOut(false);
            setText("");
            setAssistantCopied(false);
            setPhase("polishing");
            return;
          }

          if (chunk) {
            clearFadeTimer();
            setFadingOut(false);
            setPhase("polishing");
            setText((prev) => prev + chunk);
          }
        });

        if (disposed && unlisten) {
          unlisten();
          unlisten = null;
        }
      } catch {
        // ignore
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
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
          setMode(event.payload.mode ?? "dictation");

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
          setPolishFlash(!!event.payload.polished);

          if (event.payload.mode !== "assistant") {
            const expectedSessionId = latestSessionIdRef.current;
            fadeTimerRef.current = setTimeout(() => {
              if (latestSessionIdRef.current !== expectedSessionId) return;
              setFadingOut(true);
              fadeTimerRef.current = null;
            }, RESULT_FADE_DELAY_MS);
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

  const hasText = text.length > 0;
  const indicatorClass =
    phase === "recording"
      ? mode === "assistant"
        ? "subtitle-dot-assistant"
        : "subtitle-dot-recording"
      : phase === "processing"
        ? "subtitle-dot-processing"
        : phase === "polishing"
          ? mode === "assistant"
            ? "subtitle-dot-assistant"
            : "subtitle-dot-polishing"
          : null;
  const hintText =
    phase === "recording"
      ? mode === "assistant"
        ? "AI 助手聆听中..."
        : "正在聆听..."
      : phase === "processing"
        ? mode === "assistant"
          ? "AI 生成中..."
          : "识别中..."
        : phase === "polishing"
          ? mode === "assistant"
            ? text
              ? null
              : "AI 生成中..."
            : streamTokens > 0
              ? `优化中... ${streamTokens} tokens`
              : "优化中..."
          : null;

  const closeAssistantOverlay = useCallback(() => {
    clearFadeTimer();
    setFadingOut(false);
    setText("");
    setAssistantCopied(false);
    setPhase("idle");
    void hideSubtitleWindow().catch(() => undefined);
  }, [clearFadeTimer]);

  const handleAssistantCopy = useCallback((event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    if (!text.trim()) return;
    void copyToClipboard(text)
      .then(() => setAssistantCopied(true))
      .catch(() => undefined);
  }, [text]);

  return (
    <div
      className={`subtitle-root${interactiveAssistantResult ? " subtitle-root-interactive" : ""}`}
      onClick={interactiveAssistantResult ? closeAssistantOverlay : undefined}
    >
      <div
        className={
          `subtitle-capsule${fadingOut ? " subtitle-fade-out" : ""}${assistantPanelActive ? " subtitle-capsule-assistant" : ""}${interactiveAssistantResult ? " subtitle-capsule-interactive" : ""}`
        }
        onClick={interactiveAssistantResult ? (event) => event.stopPropagation() : undefined}
      >
        {assistantPanelActive && (
          <div className="subtitle-assistant-actions">
            <span className={`subtitle-copy-status${assistantCopied ? " is-visible" : ""}`}>
              已复制
            </span>
            <button
              type="button"
              className={`subtitle-copy-button${interactiveAssistantResult ? " is-ready" : ""}`}
              onClick={handleAssistantCopy}
              tabIndex={interactiveAssistantResult ? 0 : -1}
            >
              复制
            </button>
          </div>
        )}
        {indicatorClass && <div className={indicatorClass} />}
        {hasText && (
          <span
            className={`subtitle-text${polishFlash ? " subtitle-polish-flash" : ""}`}
            onAnimationEnd={() => setPolishFlash(false)}
          >
            {text}
          </span>
        )}
        {hasText && phase === "polishing" && streamTokens > 0 && (
          <span className="subtitle-stream-badge">{streamTokens}</span>
        )}
        {!hasText && hintText && <span className="subtitle-hint">{hintText}</span>}
      </div>
    </div>
  );
}
