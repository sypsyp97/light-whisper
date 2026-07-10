import { useState, useEffect, useRef, useCallback, type UIEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import {
  copyToClipboard,
  getRecordingSnapshot,
  hideSubtitleWindow,
  type RecordingOutcomeKind,
  type RecordingSnapshot,
} from "@/api/tauri";
import { readLocalStorage } from "@/lib/storage";
import { THEME_STORAGE_KEY, LANGUAGE_STORAGE_KEY } from "@/lib/constants";
import { useSmoothText, segmentGraphemes } from "@/hooks/useSmoothText";
import "@/i18n";
import "../styles/theme.css";
import "../styles/subtitle.css";

interface RecordingState {
  sessionId?: number;
  revision?: number;
  phase?: RecordingSnapshot["phase"];
  isStarting?: boolean;
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
  resultStage?: "raw" | "polished";
  timing?: {
    rawFirst?: {
      status:
        | "preview_only"
        | "pasted"
        | "replaced"
        | "kept_raw"
        | "final_fallback"
        | "unchanged";
    };
  };
}

interface RecordingOutcome {
  sessionId?: number;
  revision?: number;
  outcome: RecordingOutcomeKind;
  mode?: "dictation" | "assistant";
  detail?: string;
}

type Phase = "idle" | "starting" | "recording" | "processing" | "searching" | "polishing" | "result" | "outcome";
const RESULT_FADE_DELAY_MS = 2000;
// Fade animation is ~300ms; add ~100ms buffer. Total ~2400ms — fires before the
// backend hides the subtitle window at 2500ms, so we clear stale state in time
// to prevent a one-frame flash of previous text on the next recording.
const RESULT_CLEANUP_DELAY_MS = RESULT_FADE_DELAY_MS + 400;
const OUTCOME_FADE_DELAY_MS = 1500;
const OUTCOME_CLEANUP_DELAY_MS = OUTCOME_FADE_DELAY_MS + 400;
const QUICK_CANCEL_CLEANUP_DELAY_MS = 400;
const WAVEFORM_BAR_COUNT = 9;
const EMPTY_WAVEFORM_BARS = Array.from({ length: WAVEFORM_BAR_COUNT }, () => 0);

function isValidSessionId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

function isValidRevision(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isRecordingOutcomeKind(value: unknown): value is RecordingOutcomeKind {
  return value === "too_short"
    || value === "no_speech"
    || value === "asr_error"
    || value === "processing_error"
    || value === "start_error";
}

function normalizeWaveformBars(bars: number[]): number[] {
  return Array.from({ length: WAVEFORM_BAR_COUNT }, (_, index) => {
    const value = bars[index];
    return Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
  });
}

export default function SubtitleOverlay() {
  // 初始 "idle"：窗口预创建后隐藏，等待录音事件时切换状态
  const [phase, setPhase] = useState<Phase>("idle");
  const [text, setText] = useState("");
  const [fadingOut, setFadingOut] = useState(false);
  const [polishFlash, setPolishFlash] = useState(false);
  const [rawFirstStatus, setRawFirstStatus] = useState<string | null>(null);
  const [resultStage, setResultStage] = useState<TranscriptionResult["resultStage"] | null>(null);
  const [outcome, setOutcome] = useState<RecordingOutcomeKind | null>(null);
  const [streamTokens, setStreamTokens] = useState(0);
  const [waveformBars, setWaveformBars] = useState<number[]>(EMPTY_WAVEFORM_BARS);
  const [mode, setMode] = useState<"dictation" | "assistant">("dictation");
  const [assistantCopied, setAssistantCopied] = useState(false);
  // Smoothly drain the streaming source so chunks never snap in.
  // Works for both assistant streaming (polishing phase) and interim dictation.
  const smoothText = useSmoothText(text);
  const latestSessionIdRef = useRef(0);
  const latestRevisionRef = useRef(-1);
  const pairedOutcomeRevisionRef = useRef<{ sessionId: number; revision: number } | null>(null);
  const terminalSessionIdRef = useRef(0);
  const phaseRef = useRef<Phase>("idle");
  const fadeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const cleanupTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const assistantTextRef = useRef<HTMLDivElement | null>(null);
  const shouldAutoScrollRef = useRef(true);
  const assistantPanelActive = mode === "assistant" && (
    phase === "searching"
    || phase === "polishing"
    || phase === "result"
    || (phase === "outcome" && text.trim().length > 0)
  );
  const interactiveAssistantResult = mode === "assistant" && phase === "result" && text.trim().length > 0;
  const { t, i18n } = useTranslation();

  const updatePhase = useCallback((nextPhase: Phase) => {
    phaseRef.current = nextPhase;
    setPhase(nextPhase);
  }, []);

  const prepareLiveEvent = useCallback((
    sessionId: unknown,
    revision: unknown,
    allowEqualRevision = false,
  ) => {
    if (!isValidSessionId(sessionId)) return null;
    if (revision !== undefined && !isValidRevision(revision)) return null;
    if (sessionId < latestSessionIdRef.current) return null;

    const isNewSession = sessionId > latestSessionIdRef.current;
    if (isNewSession) {
      latestSessionIdRef.current = sessionId;
      latestRevisionRef.current = -1;
      pairedOutcomeRevisionRef.current = null;
      terminalSessionIdRef.current = 0;
    }
    // Revision-less payloads exist only in legacy frontend tests. Once a real
    // revision fence exists for a session, an unversioned event cannot cross it.
    if (revision === undefined && !isNewSession && latestRevisionRef.current >= 0) return null;
    if (isValidRevision(revision)) {
      if (revision < latestRevisionRef.current) return null;
      if (revision === latestRevisionRef.current && !allowEqualRevision) return null;
    }

    return { sessionId, revision: isValidRevision(revision) ? revision : null, isNewSession };
  }, []);

  const clearFadeTimer = useCallback(() => {
    if (fadeTimerRef.current) {
      clearTimeout(fadeTimerRef.current);
      fadeTimerRef.current = null;
    }
    // Also cancel the post-fade cleanup so a newer session / assistant takeover
    // doesn't get its fresh state stomped by the old cleanup firing late.
    if (cleanupTimerRef.current) {
      clearTimeout(cleanupTimerRef.current);
      cleanupTimerRef.current = null;
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

  const applyRecordingState = useCallback((payload: RecordingState) => {
    const {
      sessionId,
      revision,
      phase: lifecyclePhase,
      isStarting,
      isRecording,
      isProcessing,
      mode: nextMode,
    } = payload;
    const liveEvent = prepareLiveEvent(sessionId, revision);
    if (!liveEvent) return;

    const { isNewSession } = liveEvent;
    setMode(nextMode ?? "dictation");

    if (isStarting || isRecording) {
      if (liveEvent.revision !== null) latestRevisionRef.current = liveEvent.revision;
      pairedOutcomeRevisionRef.current = null;
      terminalSessionIdRef.current = 0;
      clearFadeTimer();
      setFadingOut(false);
      setText("");
      setRawFirstStatus(null);
      setResultStage(null);
      setOutcome(null);
      setWaveformBars(EMPTY_WAVEFORM_BARS);
      setPolishFlash(false);
      setStreamTokens(0);
      setAssistantCopied(false);
      shouldAutoScrollRef.current = true;
      updatePhase(isStarting ? "starting" : "recording");
      return;
    }

    if (isProcessing) {
      if (liveEvent.revision !== null) latestRevisionRef.current = liveEvent.revision;
      pairedOutcomeRevisionRef.current = null;
      clearFadeTimer();
      setFadingOut(false);
      setOutcome(null);
      setWaveformBars(EMPTY_WAVEFORM_BARS);
      updatePhase("processing");
      return;
    }

    // Outcome emits a terminal recording-state and a recording-outcome with
    // one revision. Fence older lifecycle events immediately, then allow the
    // paired outcome event to consume that exact revision once.
    if (lifecyclePhase === "outcome" && liveEvent.revision !== null) {
      latestRevisionRef.current = liveEvent.revision;
      terminalSessionIdRef.current = liveEvent.sessionId;
      pairedOutcomeRevisionRef.current = {
        sessionId: liveEvent.sessionId,
        revision: liveEvent.revision,
      };
      return;
    }

    if (lifecyclePhase === "idle" && liveEvent.revision !== null) {
      latestRevisionRef.current = liveEvent.revision;
      pairedOutcomeRevisionRef.current = null;
    }

    // A press/release can cancel while microphone startup is still in flight.
    // Keep the fixed indicator mounted for its short fade, then leave the
    // capsule fully transparent until the backend's guarded hide completes.
    if (!isNewSession && phaseRef.current === "starting") {
      terminalSessionIdRef.current = liveEvent.sessionId;
      clearFadeTimer();
      setFadingOut(true);
      const expectedSessionId = liveEvent.sessionId;
      cleanupTimerRef.current = setTimeout(() => {
        if (latestSessionIdRef.current !== expectedSessionId) return;
        setOutcome(null);
        setWaveformBars(EMPTY_WAVEFORM_BARS);
        updatePhase("idle");
        cleanupTimerRef.current = null;
      }, QUICK_CANCEL_CLEANUP_DELAY_MS);
    }
  }, [clearFadeTimer, prepareLiveEvent, updatePhase]);

  const applyRecordingOutcome = useCallback((payload: RecordingOutcome) => {
    const { sessionId, revision, outcome: nextOutcome, mode: nextMode } = payload;
    if (!isRecordingOutcomeKind(nextOutcome)) return;
    const allowPairedRevision = isValidSessionId(sessionId)
      && isValidRevision(revision)
      && pairedOutcomeRevisionRef.current?.sessionId === sessionId
      && pairedOutcomeRevisionRef.current.revision === revision;
    const liveEvent = prepareLiveEvent(sessionId, revision, allowPairedRevision);
    if (!liveEvent) return;

    if (liveEvent.revision !== null) latestRevisionRef.current = liveEvent.revision;
    pairedOutcomeRevisionRef.current = null;
    terminalSessionIdRef.current = liveEvent.sessionId;
    const isCurrentSession = !liveEvent.isNewSession;
    clearFadeTimer();
    setMode(nextMode ?? "dictation");
    setText((currentText) => (
      isCurrentSession && nextMode === "assistant" && nextOutcome === "processing_error"
        ? currentText
        : ""
    ));
    setRawFirstStatus(null);
    setResultStage(null);
    setWaveformBars(EMPTY_WAVEFORM_BARS);
    setPolishFlash(false);
    setStreamTokens(0);
    setAssistantCopied(false);
    setOutcome(nextOutcome);
    updatePhase("outcome");
    setFadingOut(false);

    const expectedSessionId = liveEvent.sessionId;
    fadeTimerRef.current = setTimeout(() => {
      if (latestSessionIdRef.current !== expectedSessionId) return;
      setFadingOut(true);
      fadeTimerRef.current = null;
    }, OUTCOME_FADE_DELAY_MS);
    cleanupTimerRef.current = setTimeout(() => {
      if (latestSessionIdRef.current !== expectedSessionId) return;
      setText("");
      setOutcome(null);
      updatePhase("idle");
      cleanupTimerRef.current = null;
    }, OUTCOME_CLEANUP_DELAY_MS);
  }, [clearFadeTimer, prepareLiveEvent, updatePhase]);

  const hydrateRecordingSnapshot = useCallback((snapshot: RecordingSnapshot | null) => {
    if (!snapshot || !isValidSessionId(snapshot.sessionId) || !isValidRevision(snapshot.revision)) return;
    // Live listeners own an observed session. A late snapshot may only hydrate
    // a session the overlay has never seen, so Active can never regress to Starting.
    if (snapshot.sessionId <= latestSessionIdRef.current) return;

    if (snapshot.phase === "idle") {
      latestSessionIdRef.current = snapshot.sessionId;
      latestRevisionRef.current = snapshot.revision;
      terminalSessionIdRef.current = snapshot.sessionId;
      clearFadeTimer();
      setMode(snapshot.mode);
      setText("");
      setRawFirstStatus(null);
      setResultStage(null);
      setOutcome(null);
      setWaveformBars(EMPTY_WAVEFORM_BARS);
      setPolishFlash(false);
      setStreamTokens(0);
      setAssistantCopied(false);
      setFadingOut(true);
      updatePhase("idle");
      return;
    }

    if (snapshot.phase === "outcome") {
      if (!isRecordingOutcomeKind(snapshot.outcome)) return;
      applyRecordingOutcome({
        sessionId: snapshot.sessionId,
        revision: snapshot.revision,
        mode: snapshot.mode,
        outcome: snapshot.outcome,
        detail: snapshot.detail,
      });
      return;
    }

    applyRecordingState({
      sessionId: snapshot.sessionId,
      revision: snapshot.revision,
      mode: snapshot.mode,
      isStarting: snapshot.phase === "starting",
      isRecording: snapshot.phase === "recording",
      isProcessing: snapshot.phase === "processing",
    });
  }, [applyRecordingOutcome, applyRecordingState, clearFadeTimer, updatePhase]);

  // Close the cold-mount race: subscribe to both lifecycle channels first,
  // then hydrate any state that was already active before this webview loaded.
  useEffect(() => {
    let disposed = false;
    let unlistenState: (() => void) | null = null;
    let unlistenOutcome: (() => void) | null = null;

    void (async () => {
      try {
        // Start both subscriptions before awaiting either promise, so there is
        // no gap where a fast start_error can land between the two listeners.
        const [stateListener, outcomeListener] = await Promise.allSettled([
          listen<RecordingState>("recording-state", (event) => {
            applyRecordingState(event.payload);
          }),
          listen<RecordingOutcome>("recording-outcome", (event) => {
            applyRecordingOutcome(event.payload);
          }),
        ]);
        if (stateListener.status !== "fulfilled" || outcomeListener.status !== "fulfilled") {
          if (stateListener.status === "fulfilled") stateListener.value();
          if (outcomeListener.status === "fulfilled") outcomeListener.value();
          return;
        }

        if (disposed) {
          stateListener.value();
          outcomeListener.value();
          return;
        }
        unlistenState = stateListener.value;
        unlistenOutcome = outcomeListener.value;

        try {
          const snapshot = await getRecordingSnapshot();
          if (!disposed) hydrateRecordingSnapshot(snapshot);
        } catch {
          // Live events remain authoritative when snapshot hydration is unavailable.
        }
      } catch {
        // 忽略事件监听初始化失败
      }
    })();

    return () => {
      disposed = true;
      unlistenState?.();
      unlistenOutcome?.();
      clearFadeTimer();
    };
  }, [applyRecordingOutcome, applyRecordingState, clearFadeTimer, hydrateRecordingSnapshot]);

  // 监听 AI 润色状态（含流式进度）
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<{ status: string; tokens?: number; sessionId?: number }>("ai-polish-status", (event) => {
          const { status, tokens, sessionId } = event.payload;
          if (typeof sessionId === "number" && sessionId < latestSessionIdRef.current) return;
          if (sessionId === terminalSessionIdRef.current) return;
          if (status === "polishing") {
            clearFadeTimer();
            setFadingOut(false);
            setPolishFlash(false);
            setStreamTokens(0);
            updatePhase("polishing");
          } else if (status === "fallback") {
            clearFadeTimer();
            setFadingOut(false);
            setStreamTokens(0);
            updatePhase("polishing");
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
  }, [clearFadeTimer, updatePhase]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<{ sessionId?: number; chunk?: string; status?: string }>("assistant-stream", (event) => {
          const { sessionId, chunk, status } = event.payload;
          if (sessionId === terminalSessionIdRef.current) return;
          if (typeof sessionId === "number") {
            if (sessionId < latestSessionIdRef.current) return;
            latestSessionIdRef.current = sessionId;
          }

          setMode("assistant");

          if (status === "started") {
            clearFadeTimer();
            setFadingOut(false);
            setText("");
            setRawFirstStatus(null);
            setResultStage(null);
            setOutcome(null);
            setAssistantCopied(false);
            shouldAutoScrollRef.current = true;
            updatePhase("polishing");
            return;
          }

          if (status === "searching") {
            clearFadeTimer();
            setFadingOut(false);
            updatePhase("searching");
            return;
          }

          if (chunk) {
            clearFadeTimer();
            setFadingOut(false);
            updatePhase("polishing");
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
  }, [clearFadeTimer, updatePhase]);

  // 监听录音波形数据
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<{ sessionId?: number; bars: number[] }>("waveform", (event) => {
          const { sessionId, bars } = event.payload;
          if (!isValidSessionId(sessionId) || sessionId !== latestSessionIdRef.current) return;
          if (phaseRef.current !== "starting" && phaseRef.current !== "recording") return;
          setWaveformBars(normalizeWaveformBars(bars));
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
          if (sessionId === terminalSessionIdRef.current) return;
          if (typeof sessionId === "number") {
            if (sessionId < latestSessionIdRef.current) return;
            latestSessionIdRef.current = sessionId;
          }
          setMode(event.payload.mode ?? "dictation");

          const incomingText = event.payload.text || "";
          setOutcome(null);
          setText(incomingText);
          setRawFirstStatus(event.payload.timing?.rawFirst?.status ?? null);
          setResultStage(event.payload.resultStage ?? null);

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
            setRawFirstStatus(null);
            setResultStage(null);
            updatePhase("processing");
            setFadingOut(true);
            return;
          }

          setText(finalText);
          updatePhase("result");
          setFadingOut(false);
          setPolishFlash(!!event.payload.polished);
          if (event.payload.resultStage === "raw") {
            return;
          }

          if (event.payload.mode !== "assistant") {
            const expectedSessionId = latestSessionIdRef.current;
            fadeTimerRef.current = setTimeout(() => {
              if (latestSessionIdRef.current !== expectedSessionId) return;
              setFadingOut(true);
              fadeTimerRef.current = null;
            }, RESULT_FADE_DELAY_MS);
            // After the fade animation finishes, clear stale state so the next
            // recording does not flash the previous result for one frame when
            // the still-alive subtitle window is re-shown.
            cleanupTimerRef.current = setTimeout(() => {
              if (latestSessionIdRef.current !== expectedSessionId) return;
              setText("");
              setRawFirstStatus(null);
              setResultStage(null);
              setOutcome(null);
              updatePhase("idle");
              setFadingOut(false);
              setPolishFlash(false);
              setStreamTokens(0);
              setWaveformBars(EMPTY_WAVEFORM_BARS);
              cleanupTimerRef.current = null;
            }, RESULT_CLEANUP_DELAY_MS);
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
  }, [clearFadeTimer, updatePhase]);

  const isStreaming = text.length > 0 && smoothText.length < text.length;
  const hasText = smoothText.length > 0;
  const rawFirstLabelKey = rawFirstStatus === "preview_only" && resultStage === "polished"
    ? "polished_preview"
    : rawFirstStatus;
  const isAssistant = mode === "assistant";

  let indicatorClass: string | null = null;
  switch (phase) {
    case "processing": indicatorClass = isAssistant ? "subtitle-dot-assistant-processing" : "subtitle-dot-processing"; break;
    case "searching":  indicatorClass = "subtitle-dot-assistant"; break;
    case "polishing":  indicatorClass = isAssistant ? "subtitle-dot-assistant" : "subtitle-dot-polishing"; break;
  }

  let hintText: string | null = null;
  switch (phase) {
    case "starting":   hintText = t("subtitle.connectingMicrophone"); break;
    case "recording":  hintText = isAssistant ? t("subtitle.aiListening") : t("subtitle.listening"); break;
    case "processing": hintText = isAssistant ? t("subtitle.aiGenerating") : t("subtitle.recognizing"); break;
    case "searching":  hintText = t("subtitle.webSearching"); break;
    case "polishing":
      if (isAssistant) { hintText = hasText ? null : t("subtitle.aiGenerating"); }
      else { hintText = streamTokens > 0 ? t("subtitle.polishingWithTokens", { tokens: streamTokens }) : t("subtitle.polishing"); }
      break;
  }
  const outcomeText = outcome ? t(`recording.outcome.${outcome}`) : null;

  const closeAssistantOverlay = useCallback(() => {
    clearFadeTimer();
    setFadingOut(false);
    setText("");
    setRawFirstStatus(null);
    setResultStage(null);
    setOutcome(null);
    setWaveformBars(EMPTY_WAVEFORM_BARS);
    setAssistantCopied(false);
    updatePhase("idle");
    void hideSubtitleWindow().catch(() => undefined);
  }, [clearFadeTimer, updatePhase]);

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
        {assistantPanelActive && phase !== "outcome" && (
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
        {(phase === "starting" || phase === "recording") ? (
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
        {hasText && rawFirstLabelKey && !isAssistant && (
          <span className="subtitle-raw-first-badge">{t(`subtitle.rawFirst.${rawFirstLabelKey}`)}</span>
        )}
        {outcomeText && <span key={outcome} className="subtitle-hint" role="status">{outcomeText}</span>}
        {!hasText && !outcomeText && hintText && <span key={phase} className="subtitle-hint">{hintText}</span>}
      </div>
    </div>
  );
}
