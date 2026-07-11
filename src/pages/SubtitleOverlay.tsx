import {
  useState,
  useEffect,
  useRef,
  useCallback,
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
  type UIEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Copy, ExternalLink, MessageCircle, Send, Sparkles, X } from "lucide-react";
import {
  cancelAssistantConversation,
  continueAssistantConversation,
  copyToClipboard,
  getRecordingSnapshot,
  hideSubtitleWindow,
  openAssistantSource,
  retryAssistantRequest,
  type AssistantConversationTurn,
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

interface AssistantSource {
  title: string;
  url: string;
  publishedDate?: string | null;
}

interface AssistantStreamEvent {
  sessionId?: number;
  chunk?: string;
  status?: string;
  request?: string;
  message?: string;
  source?: AssistantSource;
  sources?: AssistantSource[];
  query?: string;
  searchProvider?: "model_native" | "exa" | "tavily";
  elapsedMs?: number;
  searchElapsedMs?: number | null;
  webSearchEnabled?: boolean;
}

interface ConversationMessage extends AssistantConversationTurn {
  id: number;
  sources?: AssistantSource[];
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

function mergeAssistantSources(
  current: AssistantSource[],
  incoming: AssistantSource[],
): AssistantSource[] {
  const merged = [...current];
  const seen = new Set(current.map((source) => source.url));
  for (const source of incoming) {
    if (!source?.url || seen.has(source.url)) continue;
    seen.add(source.url);
    merged.push(source);
  }
  return merged.slice(0, 10);
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
  const [assistantRequest, setAssistantRequest] = useState("");
  const [assistantSources, setAssistantSources] = useState<AssistantSource[]>([]);
  const [assistantSearchError, setAssistantSearchError] = useState(false);
  const [assistantSearchQuery, setAssistantSearchQuery] = useState("");
  const [assistantSearchProvider, setAssistantSearchProvider] = useState<string | null>(null);
  const [assistantSearchElapsedMs, setAssistantSearchElapsedMs] = useState<number | null>(null);
  const [assistantElapsedMs, setAssistantElapsedMs] = useState<number | null>(null);
  const [assistantRetryBusy, setAssistantRetryBusy] = useState(false);
  const [conversationOpen, setConversationOpen] = useState(false);
  const [conversationMessages, setConversationMessages] = useState<ConversationMessage[]>([]);
  const [conversationInput, setConversationInput] = useState("");
  const [conversationBusy, setConversationBusy] = useState(false);
  const [conversationDraft, setConversationDraft] = useState("");
  const [conversationError, setConversationError] = useState<string | null>(null);
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
  const conversationListRef = useRef<HTMLDivElement | null>(null);
  const conversationInputRef = useRef<HTMLTextAreaElement | null>(null);
  const conversationInitialResponseRef = useRef("");
  const conversationDraftRef = useRef("");
  const conversationTurnSourcesRef = useRef<AssistantSource[]>([]);
  const conversationMessageIdRef = useRef(0);
  const conversationOpenRef = useRef(false);
  const conversationRequestGenerationRef = useRef(0);
  const conversationShouldAutoScrollRef = useRef(true);
  const shouldAutoScrollRef = useRef(true);
  const assistantPanelActive = mode === "assistant" && (
    phase === "searching"
    || phase === "polishing"
    || phase === "result"
    || (phase === "outcome" && text.trim().length > 0)
  );
  const interactiveAssistantResult = mode === "assistant" && phase === "result" && text.trim().length > 0;
  const assistantInteractive = interactiveAssistantResult || conversationOpen;
  const { t, i18n } = useTranslation();

  const updatePhase = useCallback((nextPhase: Phase) => {
    phaseRef.current = nextPhase;
    setPhase(nextPhase);
  }, []);

  const resetConversationState = useCallback(() => {
    conversationOpenRef.current = false;
    conversationRequestGenerationRef.current += 1;
    conversationShouldAutoScrollRef.current = true;
    void cancelAssistantConversation().catch(() => undefined);
    setAssistantRequest("");
    setAssistantSources([]);
    setAssistantSearchError(false);
    setAssistantSearchQuery("");
    setAssistantSearchProvider(null);
    setAssistantSearchElapsedMs(null);
    setAssistantElapsedMs(null);
    setAssistantRetryBusy(false);
    setConversationOpen(false);
    setConversationMessages([]);
    setConversationInput("");
    setConversationBusy(false);
    setConversationDraft("");
    setConversationError(null);
    conversationInitialResponseRef.current = "";
    conversationDraftRef.current = "";
    conversationTurnSourcesRef.current = [];
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
      resetConversationState();
    }
    // Revision-less payloads exist only in legacy frontend tests. Once a real
    // revision fence exists for a session, an unversioned event cannot cross it.
    if (revision === undefined && !isNewSession && latestRevisionRef.current >= 0) return null;
    if (isValidRevision(revision)) {
      if (revision < latestRevisionRef.current) return null;
      if (revision === latestRevisionRef.current && !allowEqualRevision) return null;
    }

    return { sessionId, revision: isValidRevision(revision) ? revision : null, isNewSession };
  }, [resetConversationState]);

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
        unlisten = await listen<AssistantStreamEvent>("assistant-stream", (event) => {
          const {
            sessionId, chunk, status, request, source, sources, query,
            searchProvider, elapsedMs, searchElapsedMs,
          } = event.payload;
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
            setAssistantSearchError(false);
            setAssistantSearchQuery("");
            setAssistantSearchElapsedMs(null);
            setAssistantElapsedMs(null);
            if (request?.trim()) setAssistantRequest(request.trim());
            if (searchProvider) setAssistantSearchProvider(searchProvider);
            shouldAutoScrollRef.current = true;
            updatePhase("polishing");
            return;
          }

          if (status === "searching") {
            clearFadeTimer();
            setFadingOut(false);
            if (query?.trim()) setAssistantSearchQuery(query.trim());
            if (searchProvider) setAssistantSearchProvider(searchProvider);
            updatePhase("searching");
            return;
          }

          if (status === "search_complete" && Array.isArray(sources)) {
            setAssistantSources((current) => mergeAssistantSources(current, sources));
            setAssistantSearchError(false);
            if (query?.trim()) setAssistantSearchQuery(query.trim());
            if (searchProvider) setAssistantSearchProvider(searchProvider);
            if (typeof elapsedMs === "number") setAssistantSearchElapsedMs(elapsedMs);
            return;
          }

          if (status === "citation" && source) {
            setAssistantSources((current) => mergeAssistantSources(current, [source]));
            return;
          }

          if (status === "search_error") {
            setAssistantSearchError(true);
            if (query?.trim()) setAssistantSearchQuery(query.trim());
            if (searchProvider) setAssistantSearchProvider(searchProvider);
            if (typeof elapsedMs === "number") setAssistantSearchElapsedMs(elapsedMs);
            return;
          }

          if (status === "done") {
            if (typeof elapsedMs === "number") setAssistantElapsedMs(elapsedMs);
            if (typeof searchElapsedMs === "number") setAssistantSearchElapsedMs(searchElapsedMs);
            if (searchProvider) setAssistantSearchProvider(searchProvider);
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

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<AssistantStreamEvent>("assistant-chat-stream", (event) => {
          const { sessionId, chunk, status, source, sources, message } = event.payload;
          if (sessionId !== latestSessionIdRef.current || !conversationOpenRef.current) return;

          if (status === "started") {
            conversationDraftRef.current = "";
            conversationTurnSourcesRef.current = [];
            setConversationDraft("");
            setConversationError(null);
            return;
          }

          if (status === "searching") {
            setConversationError(null);
            return;
          }

          if (status === "search_complete" && Array.isArray(sources)) {
            conversationTurnSourcesRef.current = mergeAssistantSources(
              conversationTurnSourcesRef.current,
              sources,
            );
            return;
          }

          if (status === "citation" && source) {
            conversationTurnSourcesRef.current = mergeAssistantSources(
              conversationTurnSourcesRef.current,
              [source],
            );
            return;
          }

          if (status === "search_error") {
            setConversationError(t("subtitle.conversation.searchFailed"));
            return;
          }

          if (status === "error") {
            setConversationError(message || t("subtitle.conversation.sendFailed"));
            return;
          }

          if (chunk) {
            conversationDraftRef.current += chunk;
            setConversationDraft(conversationDraftRef.current);
          }
        });

        if (disposed && unlisten) {
          unlisten();
          unlisten = null;
        }
      } catch {
        setConversationError(t("subtitle.conversation.sendFailed"));
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [t]);

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

  useEffect(() => {
    if (!conversationOpen) return;
    const frame = window.requestAnimationFrame(() => conversationInputRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [conversationOpen]);

  useEffect(() => {
    if (!conversationOpen) return;
    const node = conversationListRef.current;
    if (node && conversationShouldAutoScrollRef.current) {
      node.scrollTop = node.scrollHeight;
    }
  }, [conversationDraft, conversationMessages, conversationOpen]);

  useEffect(() => {
    if (!conversationOpen) return;
    const node = conversationInputRef.current;
    if (!node) return;
    node.style.height = "auto";
    const nextHeight = Math.min(96, Math.max(32, node.scrollHeight));
    node.style.height = `${nextHeight}px`;
    node.style.overflowY = node.scrollHeight > 96 ? "auto" : "hidden";
  }, [conversationInput, conversationOpen]);

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
          if (event.payload.mode === "assistant") {
            conversationInitialResponseRef.current = finalText;
          }
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
    resetConversationState();
    updatePhase("idle");
    void hideSubtitleWindow().catch(() => undefined);
  }, [clearFadeTimer, resetConversationState, updatePhase]);

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

  const handleConversationScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    const node = event.currentTarget;
    const distanceToBottom = node.scrollHeight - node.clientHeight - node.scrollTop;
    conversationShouldAutoScrollRef.current = distanceToBottom <= 24;
  }, []);

  const handleOpenSource = useCallback((
    event: MouseEvent<HTMLButtonElement>,
    source: AssistantSource,
  ) => {
    event.stopPropagation();
    void openAssistantSource(source.url).catch(() => undefined);
  }, []);

  const handleOpenConversation = useCallback((event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    const initialResponse = conversationInitialResponseRef.current || text.trim();
    if (!assistantRequest.trim() || !initialResponse) {
      setConversationError(t("subtitle.conversation.requestMissing"));
      conversationOpenRef.current = true;
      setConversationOpen(true);
      return;
    }
    clearFadeTimer();
    conversationShouldAutoScrollRef.current = true;
    conversationInitialResponseRef.current = initialResponse;
    if (conversationMessages.length === 0) {
      conversationMessageIdRef.current += 1;
      setConversationMessages([{
        id: conversationMessageIdRef.current,
        role: "assistant",
        content: initialResponse,
        sources: assistantSources,
      }]);
    }
    setConversationError(assistantSearchError ? t("subtitle.conversation.searchFailed") : null);
    conversationOpenRef.current = true;
    setConversationOpen(true);
  }, [
    assistantRequest,
    assistantSearchError,
    assistantSources,
    clearFadeTimer,
    conversationMessages.length,
    t,
    text,
  ]);

  const handleCloseConversation = useCallback((event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    conversationOpenRef.current = false;
    conversationRequestGenerationRef.current += 1;
    void cancelAssistantConversation().catch(() => undefined);
    setConversationOpen(false);
    setConversationInput("");
    setConversationDraft("");
    setConversationBusy(false);
    conversationDraftRef.current = "";
  }, []);

  const submitConversationMessage = useCallback(async () => {
    const message = conversationInput.trim();
    const initialResponse = conversationInitialResponseRef.current;
    if (!message || conversationBusy) return;
    if (!assistantRequest.trim() || !initialResponse) {
      setConversationError(t("subtitle.conversation.requestMissing"));
      return;
    }

    const priorHistory: AssistantConversationTurn[] = conversationMessages
      .slice(1)
      .slice(-12)
      .map(({ role, content }) => ({ role, content }));
    const requestSessionId = latestSessionIdRef.current;
    const requestGeneration = conversationRequestGenerationRef.current + 1;
    conversationRequestGenerationRef.current = requestGeneration;
    conversationMessageIdRef.current += 1;
    const userMessage: ConversationMessage = {
      id: conversationMessageIdRef.current,
      role: "user",
      content: message,
    };

    setConversationMessages((current) => [...current, userMessage]);
    conversationShouldAutoScrollRef.current = true;
    setConversationInput("");
    setConversationBusy(true);
    setConversationError(null);
    setConversationDraft("");
    conversationDraftRef.current = "";
    conversationTurnSourcesRef.current = [];

    try {
      const response = await continueAssistantConversation({
        sessionId: requestSessionId,
        initialRequest: assistantRequest,
        initialResponse,
        history: priorHistory,
        message,
      });
      if (
        requestGeneration !== conversationRequestGenerationRef.current
        || requestSessionId !== latestSessionIdRef.current
        || !conversationOpenRef.current
      ) return;
      conversationMessageIdRef.current += 1;
      const assistantMessage: ConversationMessage = {
        id: conversationMessageIdRef.current,
        role: "assistant",
        content: response.trim(),
        sources: conversationTurnSourcesRef.current,
      };
      setConversationMessages((current) => [...current, assistantMessage]);
      setConversationDraft("");
      conversationDraftRef.current = "";
    } catch (error) {
      if (
        requestGeneration !== conversationRequestGenerationRef.current
        || requestSessionId !== latestSessionIdRef.current
        || !conversationOpenRef.current
      ) return;
      setConversationError(
        error instanceof Error && error.message.trim()
          ? error.message
          : t("subtitle.conversation.sendFailed"),
      );
    } finally {
      if (requestGeneration === conversationRequestGenerationRef.current) {
        setConversationBusy(false);
      }
    }
  }, [assistantRequest, conversationBusy, conversationInput, conversationMessages, t]);

  const handleConversationSubmit = useCallback((event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    void submitConversationMessage();
  }, [submitConversationMessage]);

  const handleConversationKeyDown = useCallback((event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submitConversationMessage();
    }
  }, [submitConversationMessage]);

  const handleRetryAssistant = useCallback(async (event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    const request = assistantRequest.trim();
    const sessionId = latestSessionIdRef.current;
    if (!request || !isValidSessionId(sessionId) || assistantRetryBusy) return;

    setAssistantRetryBusy(true);
    setAssistantSearchError(false);
    setAssistantSources([]);
    setAssistantSearchQuery("");
    setAssistantSearchElapsedMs(null);
    setAssistantElapsedMs(null);
    setText("");
    updatePhase("polishing");
    try {
      const result = await retryAssistantRequest({ sessionId, request });
      const finalText = result.trim();
      setText(finalText);
      conversationInitialResponseRef.current = finalText;
      updatePhase(finalText ? "result" : "outcome");
    } catch {
      setAssistantSearchError(true);
      updatePhase("result");
    } finally {
      setAssistantRetryBusy(false);
    }
  }, [assistantRequest, assistantRetryBusy, updatePhase]);

  const assistantOverlayDismissible = interactiveAssistantResult || conversationOpen;

  const renderSources = (sources: AssistantSource[] | undefined) => {
    if (!sources?.length) return null;
    return (
      <div className="subtitle-source-list" aria-label={t("subtitle.conversation.sources")}>
        {sources.map((source, index) => (
          <button
            key={`${source.url}-${index}`}
            type="button"
            className="subtitle-source-link"
            title={source.url}
            onClick={(event) => handleOpenSource(event, source)}
          >
            <span>{source.title || source.url}</span>
            <ExternalLink size={11} aria-hidden="true" />
          </button>
        ))}
      </div>
    );
  };

  return (
    <div
      className={`subtitle-root${assistantInteractive ? " subtitle-root-interactive" : ""}`}
      role="presentation"
      onClick={assistantOverlayDismissible ? closeAssistantOverlay : undefined}
    >
      <div
        className={
          `subtitle-capsule${fadingOut ? " subtitle-fade-out" : ""}${assistantPanelActive ? " subtitle-capsule-assistant" : ""}${assistantInteractive ? " subtitle-capsule-interactive" : ""}${conversationOpen ? " subtitle-capsule-conversation" : ""}`
        }
        role="presentation"
        onClick={assistantInteractive ? (event) => event.stopPropagation() : undefined}
      >
        {conversationOpen ? (
          <section
            className="subtitle-conversation"
            role="dialog"
            aria-label={t("subtitle.conversation.title")}
          >
            <header className="subtitle-conversation-header">
              <div className="subtitle-conversation-title">
                <Sparkles size={15} aria-hidden="true" />
                <span>{t("subtitle.conversation.title")}</span>
              </div>
              <button
                type="button"
                className="subtitle-conversation-close"
                onClick={handleCloseConversation}
                aria-label={t("subtitle.conversation.close")}
                title={t("subtitle.conversation.close")}
              >
                <X size={15} aria-hidden="true" />
              </button>
            </header>

            <div
              ref={conversationListRef}
              className="subtitle-conversation-messages"
              aria-label={t("subtitle.conversation.history")}
              role="log"
              aria-live="polite"
              aria-busy={conversationBusy}
              tabIndex={0}
              onScroll={handleConversationScroll}
            >
              {conversationMessages.map((message) => (
                <article
                  key={message.id}
                  className={`subtitle-conversation-message is-${message.role}`}
                >
                  <div className="subtitle-conversation-row">
                    {message.role === "assistant" && (
                      <span className="subtitle-conversation-assistant-mark" aria-hidden="true">
                        <Sparkles size={12} />
                      </span>
                    )}
                    <div className="subtitle-conversation-bubble">{message.content}</div>
                  </div>
                  {message.role === "assistant" && renderSources(message.sources)}
                </article>
              ))}
              {conversationBusy && (
                <article className="subtitle-conversation-message is-assistant is-streaming">
                  <div className="subtitle-conversation-row">
                    <span className="subtitle-conversation-assistant-mark" aria-hidden="true">
                      <Sparkles size={12} />
                    </span>
                    <div className="subtitle-conversation-bubble">
                      {conversationDraft || t("subtitle.conversation.thinking")}
                    </div>
                  </div>
                </article>
              )}
            </div>

            {conversationError && (
              <div className="subtitle-conversation-error" role="status">{conversationError}</div>
            )}

            <form className="subtitle-conversation-composer" onSubmit={handleConversationSubmit}>
              <textarea
                ref={conversationInputRef}
                value={conversationInput}
                onChange={(event) => setConversationInput(event.target.value)}
                onKeyDown={handleConversationKeyDown}
                placeholder={t("subtitle.conversation.placeholder")}
                aria-label={t("subtitle.conversation.placeholder")}
                rows={1}
                maxLength={6000}
                disabled={conversationBusy || !assistantRequest.trim()}
              />
              <button
                type="submit"
                className="subtitle-conversation-send"
                disabled={conversationBusy || !assistantRequest.trim() || !conversationInput.trim()}
                aria-label={t("subtitle.conversation.send")}
                title={t("subtitle.conversation.send")}
              >
                <Send size={15} aria-hidden="true" />
              </button>
            </form>
          </section>
        ) : (
          <>
            {interactiveAssistantResult && (
              <div className="subtitle-assistant-actions">
                <span className={`subtitle-copy-status${assistantCopied ? " is-visible" : ""}`}>
                  {t("common.copied")}
                </span>
                <button
                  type="button"
                  className={`subtitle-action-button${interactiveAssistantResult ? " is-ready" : ""}`}
                  onClick={handleAssistantCopy}
                  tabIndex={interactiveAssistantResult ? 0 : -1}
                  aria-label={t("common.copy")}
                  title={t("common.copy")}
                >
                  <Copy size={13} aria-hidden="true" />
                  <span>{t("common.copy")}</span>
                </button>
                <button
                  type="button"
                  className={`subtitle-action-button is-conversation${interactiveAssistantResult ? " is-ready" : ""}`}
                  onClick={handleOpenConversation}
                  tabIndex={interactiveAssistantResult ? 0 : -1}
                  aria-label={t("subtitle.conversation.open")}
                  title={t("subtitle.conversation.open")}
                >
                  <MessageCircle size={13} aria-hidden="true" />
                  <span>{t("subtitle.conversation.open")}</span>
                </button>
              </div>
            )}
            {(phase === "starting" || phase === "recording" || indicatorClass) && (
              <span className="subtitle-status-indicator" aria-hidden="true">
                {(phase === "starting" || phase === "recording") ? (
                  <span className={`subtitle-waveform-indicator${mode === "assistant" ? " is-assistant" : ""}`}>
                    {waveformBars.map((h, i) => (
                      <span
                        key={i}
                        className="subtitle-waveform-indicator-bar"
                        style={{ height: `${Math.max(2, h * 16)}px` }}
                      />
                    ))}
                  </span>
                ) : indicatorClass ? (
                  <span className={indicatorClass} />
                ) : null}
              </span>
            )}
            {hasText && (
              <div
                ref={assistantTextRef}
                className={`subtitle-text${polishFlash ? " subtitle-polish-flash" : ""}${isStreaming ? " subtitle-text-streaming" : ""}`}
                role="status"
                aria-live="polite"
                onScroll={assistantPanelActive ? handleAssistantScroll : undefined}
                onAnimationEnd={(e) => {
                  if (e.target === e.currentTarget) setPolishFlash(false);
                }}
              >
                {segmentGraphemes(smoothText).map((g, i) => (
                  <span key={i} className="stream-char">{g}</span>
                ))}
              </div>
            )}
            {isAssistant && phase === "result" && renderSources(assistantSources)}
            {isAssistant && phase === "result" && (assistantSearchQuery || assistantElapsedMs !== null) && (
              <div className="subtitle-search-meta" role="status">
                {assistantSearchQuery && (
                  <span className="subtitle-search-query" title={assistantSearchQuery}>
                    {assistantSearchQuery}
                  </span>
                )}
                {assistantSearchProvider && (
                  <span>{t("subtitle.conversation.searchProvider", { provider: assistantSearchProvider })}</span>
                )}
                {assistantSearchElapsedMs !== null && (
                  <span>{t("subtitle.conversation.searchTiming", { ms: assistantSearchElapsedMs })}</span>
                )}
                {assistantElapsedMs !== null && (
                  <span>{t("subtitle.conversation.totalTiming", { ms: assistantElapsedMs })}</span>
                )}
              </div>
            )}
            {assistantSearchError && isAssistant && phase === "result" && (
              <div className="subtitle-search-warning" role="status">
                <span>{t("subtitle.conversation.searchFailed")}</span>
                <button
                  type="button"
                  onClick={handleRetryAssistant}
                  disabled={assistantRetryBusy}
                >
                  {assistantRetryBusy ? t("subtitle.conversation.retrying") : t("common.retry")}
                </button>
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
          </>
        )}
      </div>
    </div>
  );
}
