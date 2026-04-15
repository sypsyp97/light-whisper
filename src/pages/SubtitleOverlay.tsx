import { useState, useEffect, useRef, useCallback, type UIEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { copyToClipboard, hideSubtitleWindow } from "@/api/tauri";
import { readLocalStorage } from "@/lib/storage";
import { THEME_STORAGE_KEY, LANGUAGE_STORAGE_KEY } from "@/lib/constants";
import { useSmoothText, segmentGraphemes } from "@/hooks/useSmoothText";
import "@/i18n";
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

type Phase = "idle" | "recording" | "processing" | "searching" | "polishing" | "result";
const RESULT_FADE_DELAY_MS = 2000;

export default function SubtitleOverlay() {
  // 初始 "idle"：窗口预创建后隐藏，等待录音事件时切换状态
  const [phase, setPhase] = useState<Phase>("idle");
  const [text, setText] = useState("");
  const [fadingOut, setFadingOut] = useState(false);
  const [polishFlash, setPolishFlash] = useState(false);
  const [waveformBars, setWaveformBars] = useState<number[]>([]);
  const [mode, setMode] = useState<"dictation" | "assistant">("dictation");
  const [assistantCopied, setAssistantCopied] = useState(false);
  // Smoothly drain the streaming source so chunks never snap in.
  // Works for both assistant streaming (polishing phase) and interim dictation.
  const smoothText = useSmoothText(text);
  const latestSessionIdRef = useRef(0);
  const fadeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const assistantTextRef = useRef<HTMLDivElement | null>(null);
  const shouldAutoScrollRef = useRef(true);
  const assistantPanelActive = mode === "assistant" && (phase === "searching" || phase === "polishing" || phase === "result");
  const interactiveAssistantResult = mode === "assistant" && phase === "result" && text.trim().length > 0;
  const { t, i18n } = useTranslation();

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
      if (e.key === LANGUAGE_STORAGE_KEY && e.newValue) {
        void i18n.changeLanguage(e.newValue);
      }
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
            setWaveformBars([]);
            setAssistantCopied(false);
            shouldAutoScrollRef.current = true;
            setPhase("recording");
            return;
          }

          if (isProcessing) {
            clearFadeTimer();
            setFadingOut(false);
            setWaveformBars([]);
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
            shouldAutoScrollRef.current = true;
            setPhase("polishing");
            return;
          }

          if (status === "searching") {
            clearFadeTimer();
            setFadingOut(false);
            setPhase("searching");
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

  // 监听录音波形数据
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<{ sessionId?: number; bars: number[] }>("waveform", (event) => {
          const { sessionId, bars } = event.payload;
          if (typeof sessionId === "number" && sessionId < latestSessionIdRef.current) return;
          setWaveformBars(bars);
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
  }, []);

  useEffect(() => {
    if (!assistantPanelActive) {
      shouldAutoScrollRef.current = true;
      return;
    }

    const node = assistantTextRef.current;
    if (!node || !shouldAutoScrollRef.current) return;
    node.scrollTop = node.scrollHeight;
  }, [assistantPanelActive, smoothText]);

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

  const isStreaming = text.length > 0 && smoothText.length < text.length;
  const hasText = smoothText.length > 0;
  const isAssistant = mode === "assistant";

  let indicatorClass: string | null = null;
  switch (phase) {
    case "recording":  indicatorClass = isAssistant ? "subtitle-dot-assistant" : "subtitle-dot-recording"; break;
    case "processing": indicatorClass = isAssistant ? "subtitle-dot-assistant-processing" : "subtitle-dot-processing"; break;
    case "searching":  indicatorClass = "subtitle-dot-assistant"; break;
    case "polishing":  indicatorClass = isAssistant ? "subtitle-dot-assistant" : "subtitle-dot-polishing"; break;
  }

  let hintText: string | null = null;
  switch (phase) {
    case "recording":  hintText = isAssistant ? t("subtitle.aiListening") : t("subtitle.listening"); break;
    case "processing": hintText = isAssistant ? t("subtitle.aiGenerating") : t("subtitle.recognizing"); break;
    case "searching":  hintText = t("subtitle.webSearching"); break;
    case "polishing":
      if (isAssistant) { hintText = hasText ? null : t("subtitle.aiGenerating"); }
      else { hintText = streamTokens > 0 ? t("subtitle.polishingWithTokens", { tokens: streamTokens }) : t("subtitle.polishing"); }
      break;
  }

  const closeAssistantOverlay = useCallback(() => {
    clearFadeTimer();
    setFadingOut(false);
    setText("");
    setWaveformBars([]);
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

  const handleAssistantScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    const node = event.currentTarget;
    const distanceToBottom = node.scrollHeight - node.clientHeight - node.scrollTop;
    shouldAutoScrollRef.current = distanceToBottom <= 24;
  }, []);

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
              {t("common.copied")}
            </span>
            <button
              type="button"
              className={`subtitle-copy-button${interactiveAssistantResult ? " is-ready" : ""}`}
              onClick={handleAssistantCopy}
              tabIndex={interactiveAssistantResult ? 0 : -1}
            >
              {t("common.copy")}
            </button>
          </div>
        )}
        {phase === "recording" && waveformBars.length > 0 ? (
          <div className={`subtitle-waveform-indicator${mode === "assistant" ? " is-assistant" : ""}`}>
            {waveformBars.map((h, i) => (
              <div
                key={i}
                className="subtitle-waveform-indicator-bar"
                style={{ height: `${Math.max(2, h * 16)}px` }}
              />
            ))}
          </div>
        ) : indicatorClass ? (
          <div className={indicatorClass} />
        ) : null}
        {hasText && (
          <div
            ref={assistantTextRef}
            className={`subtitle-text${polishFlash ? " subtitle-polish-flash" : ""}${isStreaming ? " subtitle-text-streaming" : ""}`}
            role="status"
            aria-live="polite"
            onScroll={assistantPanelActive ? handleAssistantScroll : undefined}
            onAnimationEnd={(e) => {
              // Only react to the div's own polish-flash animation, not to
              // animationend events bubbling up from per-char .stream-char spans.
              if (e.target === e.currentTarget) setPolishFlash(false);
            }}
          >
            {segmentGraphemes(smoothText).map((g, i) => (
              <span key={i} className="stream-char">{g}</span>
            ))}
          </div>
        )}
        {hasText && phase === "polishing" && streamTokens > 0 && (
          <span className="subtitle-stream-badge">{streamTokens}</span>
        )}
        {!hasText && hintText && <span key={phase} className="subtitle-hint">{hintText}</span>}
      </div>
    </div>
  );
}
