import {
  lazy,
  Suspense,
  useState,
  useEffect,
  useLayoutEffect,
  useRef,
  useCallback,
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
  type UIEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Copy, ExternalLink, MessageCircle, Send, X } from "lucide-react";
import { searchNoticeKey } from "@/lib/searchNotice";
import GoogleSearchEntryPoint from "@/features/assistant/GoogleSearchEntryPoint";
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
import { prefersReducedMotion } from "@/lib/motion";
import "@/i18n";
import "../styles/theme.css";
import "../styles/subtitle.css";

const MarkdownContent = lazy(() => import("@/components/MarkdownContent"));

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
  stableText?: string;
  tentativeText?: string;
  polished?: boolean;
  mode?: "dictation" | "assistant";
}

interface InterimSegments {
  stableText: string;
  tentativeText: string;
}

interface LiveInterimSegments extends InterimSegments {
  sessionId: number;
  legacy?: boolean;
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
  googleSearchEntryPoint?: string | null;
  query?: string;
  searchProvider?: "model_native" | "exa" | "tavily" | "google" | "bing";
  elapsedMs?: number;
  searchElapsedMs?: number | null;
  webSearchEnabled?: boolean;
}

interface ConversationMessage extends AssistantConversationTurn {
  id: number;
  sources?: AssistantSource[];
  googleSearchEntryPoint?: string;
}

type Phase = "idle" | "starting" | "recording" | "processing" | "searching" | "polishing" | "result" | "outcome";
const RESULT_FADE_DELAY_MS = 2000;
const API_WARNING_FADE_DELAY_MS = 8000;


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

function SubtitleWaveform({ sessionId, assistant, acceptsSample }: {
  sessionId: number;
  assistant: boolean;
  acceptsSample: (sessionId: number) => boolean;
}) {
  const [bars, setBars] = useState(EMPTY_WAVEFORM_BARS);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<{ sessionId?: number; bars: number[] }>("waveform", ({ payload }) => {
      if (!disposed && sessionId > 0 && payload.sessionId === sessionId && acceptsSample(sessionId)) {
        setBars(normalizeWaveformBars(payload.bars));
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch(() => undefined);
    return () => { disposed = true; unlisten?.(); };
  }, [acceptsSample, sessionId]);

  return (
    <span className={`subtitle-waveform-indicator${assistant ? " is-assistant" : ""}`}>
      {bars.map((level, index) => (
        <span key={index} className="subtitle-waveform-indicator-bar" style={{ transform: `scaleY(${Math.max(0.125, level)})` }} />
      ))}
    </span>
  );
}

function legacyInterimSegments(previousText: string, text: string): InterimSegments {
  let boundary = 0;
  if (previousText) {
    const previous = segmentGraphemes(previousText);
    for (const [index, grapheme] of segmentGraphemes(text).entries()) {
      if (grapheme !== previous[index]) break;
      boundary += grapheme.length;
    }
  }
  return { stableText: text.slice(0, boundary), tentativeText: text.slice(boundary) };
}

function validInterimSegments(payload: TranscriptionResult): InterimSegments | null {
  const { stableText, tentativeText, text } = payload;
  if (typeof stableText !== "string" || typeof tentativeText !== "string") return null;
  if (`${stableText}${tentativeText}` !== text) return null;



  let stableBoundary = 0;
  for (const grapheme of segmentGraphemes(text)) {
    const nextBoundary = stableBoundary + grapheme.length;
    if (nextBoundary > stableText.length) break;
    stableBoundary = nextBoundary;
  }
  return {
    stableText: text.slice(0, stableBoundary),
    tentativeText: text.slice(stableBoundary),
  };
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

  const [phase, setPhase] = useState<Phase>("idle");
  const [text, setText] = useState("");
  const [polishFlash, setPolishFlash] = useState(false);
  const [interimSegments, setInterimSegments] = useState<LiveInterimSegments | null>(null);
  const [fadingOut, setFadingOut] = useState(false);
  const [outcome, setOutcome] = useState<RecordingOutcomeKind | null>(null);
  const [streamTokens, setStreamTokens] = useState(0);
  const [mode, setMode] = useState<"dictation" | "assistant">("dictation");
  const [capsuleSessionId, setCapsuleSessionId] = useState(0);
  const [assistantCopied, setAssistantCopied] = useState(false);
  const [assistantRequest, setAssistantRequest] = useState("");
  const [assistantSources, setAssistantSources] = useState<AssistantSource[]>([]);
  const [assistantGoogleSearchEntryPoint, setAssistantGoogleSearchEntryPoint] = useState("");
  const [assistantSearchError, setAssistantSearchError] = useState<string | null>(null);
  const [apiWarning, setApiWarning] = useState<string | null>(null);
  const apiWarningRef = useRef<string | null>(null);
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



  const smoothText = useSmoothText(mode === "assistant" ? text : "", {
    enabled: mode === "assistant" && !interimSegments && phase !== "result",
  });
  const tentativeRef = useRef<HTMLSpanElement>(null);
  const latestSessionIdRef = useRef(0);
  const finalTranscriptionSessionIdRef = useRef(0);
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
  const conversationTurnGoogleSearchEntryPointRef = useRef("");
  const conversationMessageIdRef = useRef(0);
  const conversationOpenRef = useRef(false);
  const conversationRequestGenerationRef = useRef(0);
  const assistantRetryGenerationRef = useRef(0);
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
    if (nextPhase !== "result") setPolishFlash(false);
    phaseRef.current = nextPhase;
    setPhase(nextPhase);
  }, []);

  const acceptsWaveformSample = useCallback((sessionId: number) => (
    sessionId === latestSessionIdRef.current
    && sessionId !== terminalSessionIdRef.current
    && (phaseRef.current === "starting" || phaseRef.current === "recording")
  ), []);

  const resetConversationState = useCallback(() => {
    conversationOpenRef.current = false;
    conversationRequestGenerationRef.current += 1;
    assistantRetryGenerationRef.current += 1;
    conversationShouldAutoScrollRef.current = true;
    void cancelAssistantConversation().catch(() => undefined);
    setAssistantRequest("");
    setAssistantSources([]);
    setAssistantGoogleSearchEntryPoint("");
    setAssistantSearchError(null);
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
    conversationTurnGoogleSearchEntryPointRef.current = "";
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
      finalTranscriptionSessionIdRef.current = 0;
      latestRevisionRef.current = -1;
      pairedOutcomeRevisionRef.current = null;
      terminalSessionIdRef.current = 0;
      setCapsuleSessionId(sessionId);
      apiWarningRef.current = null;
      setApiWarning(null);
      resetConversationState();
    }


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


    if (cleanupTimerRef.current) {
      clearTimeout(cleanupTimerRef.current);
      cleanupTimerRef.current = null;
    }
  }, []);

  const scheduleResultFade = useCallback((delay: number) => {
    clearFadeTimer();
    const expectedSessionId = latestSessionIdRef.current;
    fadeTimerRef.current = setTimeout(() => {
      if (latestSessionIdRef.current !== expectedSessionId) return;
      setFadingOut(true);
      fadeTimerRef.current = null;
    }, delay);
    cleanupTimerRef.current = setTimeout(() => {
      if (latestSessionIdRef.current !== expectedSessionId) return;
      setText("");
      setInterimSegments(null);
      setOutcome(null);
      apiWarningRef.current = null;
      setApiWarning(null);
      updatePhase("idle");
      setFadingOut(false);
      setStreamTokens(0);
      cleanupTimerRef.current = null;
    }, delay + 400);
  }, [clearFadeTimer, updatePhase]);

  const showApiWarning = useCallback((message: unknown, sessionId?: number) => {
    if (typeof message !== "string" || !message.trim()) return;
    if (sessionId !== undefined && (!isValidSessionId(sessionId) || sessionId < latestSessionIdRef.current)) return;
    if (phaseRef.current === "idle") return;
    apiWarningRef.current = message;
    setApiWarning(message);
    setFadingOut(false);
    if (phaseRef.current === "result" || phaseRef.current === "outcome") {
      scheduleResultFade(API_WARNING_FADE_DELAY_MS);
    }
  }, [scheduleResultFade]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<{ message: string; sessionId?: number }>("api-warning", ({ payload }) => {
      if (!disposed) showApiWarning(payload.message, payload.sessionId);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => { disposed = true; unlisten?.(); };
  }, [showApiWarning]);


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


    const onStorage = (e: StorageEvent) => {
      if (e.key === THEME_STORAGE_KEY) applyTheme();
      if (e.key === LANGUAGE_STORAGE_KEY && e.newValue) {
        void i18n.changeLanguage(e.newValue);
      }
    };


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
      // Interim text can advance the session before the recording event arrives.
      setCapsuleSessionId(liveEvent.sessionId);
      pairedOutcomeRevisionRef.current = null;
      terminalSessionIdRef.current = 0;
      clearFadeTimer();
      setFadingOut(false);
      setText("");
      setInterimSegments(null);
      setOutcome(null);
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
      updatePhase("processing");
      return;
    }




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




    if (!isNewSession && phaseRef.current === "starting") {
      terminalSessionIdRef.current = liveEvent.sessionId;
      clearFadeTimer();
      setFadingOut(true);
      const expectedSessionId = liveEvent.sessionId;
      cleanupTimerRef.current = setTimeout(() => {
        if (latestSessionIdRef.current !== expectedSessionId) return;
        setOutcome(null);
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
    setInterimSegments(null);
    setStreamTokens(0);
    setAssistantCopied(false);
    setOutcome(nextOutcome);
    updatePhase("outcome");
    setFadingOut(false);

    const expectedSessionId = liveEvent.sessionId;
    const fadeDelay = apiWarningRef.current ? API_WARNING_FADE_DELAY_MS : OUTCOME_FADE_DELAY_MS;
    fadeTimerRef.current = setTimeout(() => {
      if (latestSessionIdRef.current !== expectedSessionId) return;
      setFadingOut(true);
      fadeTimerRef.current = null;
    }, fadeDelay);
    cleanupTimerRef.current = setTimeout(() => {
      if (latestSessionIdRef.current !== expectedSessionId) return;
      setText("");
      setOutcome(null);
      updatePhase("idle");
      cleanupTimerRef.current = null;
    }, apiWarningRef.current ? API_WARNING_FADE_DELAY_MS + 400 : OUTCOME_CLEANUP_DELAY_MS);
  }, [clearFadeTimer, prepareLiveEvent, updatePhase]);

  const hydrateRecordingSnapshot = useCallback((snapshot: RecordingSnapshot | null) => {
    if (!snapshot || !isValidSessionId(snapshot.sessionId) || !isValidRevision(snapshot.revision)) return;


    if (snapshot.sessionId <= latestSessionIdRef.current) return;

    if (snapshot.phase === "idle") {
      latestSessionIdRef.current = snapshot.sessionId;
      latestRevisionRef.current = snapshot.revision;
      terminalSessionIdRef.current = snapshot.sessionId;
      clearFadeTimer();
      setMode(snapshot.mode);
      setText("");
      setInterimSegments(null);
      setOutcome(null);
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



  useEffect(() => {
    let disposed = false;
    let unlistenState: (() => void) | null = null;
    let unlistenOutcome: (() => void) | null = null;

    void (async () => {
      try {


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

        }
      } catch {

      }
    })();

    return () => {
      disposed = true;
      unlistenState?.();
      unlistenOutcome?.();
      clearFadeTimer();
    };
  }, [applyRecordingOutcome, applyRecordingState, clearFadeTimer, hydrateRecordingSnapshot]);


  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<{ status: string; tokens?: number; sessionId?: number; error?: string; message?: string }>("ai-polish-status", (event) => {
          const { status, tokens, sessionId } = event.payload;
          if (typeof sessionId === "number" && sessionId < latestSessionIdRef.current) return;
          if (sessionId === terminalSessionIdRef.current) return;
          if (status === "error") {
            showApiWarning(event.payload.error || event.payload.message, sessionId);
          } else if (status === "polishing") {
            clearFadeTimer();
            setFadingOut(false);
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

      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [clearFadeTimer, showApiWarning, updatePhase]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void (async () => {
      try {
        unlisten = await listen<AssistantStreamEvent>("assistant-stream", (event) => {
          const {
            sessionId, chunk, status, request, source, sources, query,
            searchProvider, elapsedMs, searchElapsedMs, googleSearchEntryPoint, message,
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
            setInterimSegments(null);
            setOutcome(null);
            setAssistantCopied(false);
            setAssistantSearchError(null);
            setAssistantGoogleSearchEntryPoint("");
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
            if (typeof googleSearchEntryPoint === "string") {
              setAssistantGoogleSearchEntryPoint(googleSearchEntryPoint);
            }
            setAssistantSearchError(sources.length === 0 ? t("subtitle.conversation.searchNoResults") : null);
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
            setAssistantSearchError(t(searchNoticeKey(message)));
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
          const {
            sessionId, chunk, status, source, sources, message, googleSearchEntryPoint,
          } = event.payload;
          if (sessionId !== latestSessionIdRef.current || !conversationOpenRef.current) return;

          if (status === "started") {
            conversationDraftRef.current = "";
            conversationTurnSourcesRef.current = [];
            conversationTurnGoogleSearchEntryPointRef.current = "";
            setConversationDraft("");
            setConversationError(null);
            return;
          }

          if (status === "searching") {
            setConversationError(null);
            return;
          }

          if (status === "search_complete" && Array.isArray(sources)) {
            setConversationError(sources.length === 0 ? t("subtitle.conversation.searchNoResults") : null);
            conversationTurnSourcesRef.current = mergeAssistantSources(
              conversationTurnSourcesRef.current,
              sources,
            );
            if (typeof googleSearchEntryPoint === "string") {
              conversationTurnGoogleSearchEntryPointRef.current = googleSearchEntryPoint;
            }
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
            setConversationError(t(searchNoticeKey(message)));
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
            if (sessionId > latestSessionIdRef.current) {
              finalTranscriptionSessionIdRef.current = 0;
              apiWarningRef.current = null;
              setApiWarning(null);
            }
            if (interim && sessionId === finalTranscriptionSessionIdRef.current) return;
            latestSessionIdRef.current = sessionId;
          }
          setMode(event.payload.mode ?? "dictation");

          const incomingText = event.payload.text || "";
          setOutcome(null);
          setText(incomingText);
          if (interim) {
            const owner = latestSessionIdRef.current;
            const segments = validInterimSegments(event.payload);
            setInterimSegments((previous) => {
              if (segments) return { ...segments, sessionId: owner };
              if (event.payload.mode === "assistant") return null;
              const previousText = previous?.sessionId === owner
                ? previous.stableText + previous.tentativeText : "";
              if (previous?.legacy && previousText === incomingText) return previous;
              return { ...legacyInterimSegments(previousText, incomingText), sessionId: owner, legacy: true };
            });
            setFadingOut(false);
            return;
          }
          setInterimSegments(null);
          if (typeof sessionId === "number") finalTranscriptionSessionIdRef.current = sessionId;

          const finalText = incomingText.trim();
          clearFadeTimer();



          if (!finalText) {
            setText("");
            updatePhase("processing");
            setFadingOut(true);
            return;
          }

          setText(finalText);
          setPolishFlash(Boolean(event.payload.polished));
          updatePhase("result");
          setFadingOut(false);
          if (event.payload.mode === "assistant") {
            conversationInitialResponseRef.current = finalText;
          }

          if (event.payload.mode !== "assistant") {
            scheduleResultFade(apiWarningRef.current ? API_WARNING_FADE_DELAY_MS : RESULT_FADE_DELAY_MS);
          }
        });

        if (disposed && unlisten) {
          unlisten();
          unlisten = null;
        }
      } catch {

      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
      clearFadeTimer();
    };
  }, [clearFadeTimer, scheduleResultFade, updatePhase]);

  const isAssistant = mode === "assistant";
  const isStreaming = isAssistant && !interimSegments && text.length > 0 && smoothText.length < text.length;
  const hasText = (isAssistant && !interimSegments ? smoothText : text).length > 0;

  useLayoutEffect(() => {
    if (!interimSegments?.legacy || !interimSegments.tentativeText || prefersReducedMotion()) return;
    const animation = tentativeRef.current?.animate?.(
      [{ opacity: 0.64 }, { opacity: 1 }],
      { duration: 100, easing: "ease-out" },
    );
    return () => animation?.cancel();
  }, [interimSegments]);

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
    setInterimSegments(null);
    setOutcome(null);
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

  const handleOpenAssistantUrl = useCallback((url: string) => {
    void openAssistantSource(url).catch(() => undefined);
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
        googleSearchEntryPoint: assistantGoogleSearchEntryPoint || undefined,
      }]);
    }
    setConversationError(assistantSearchError);
    conversationOpenRef.current = true;
    setConversationOpen(true);
  }, [
    assistantRequest,
    assistantGoogleSearchEntryPoint,
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
        googleSearchEntryPoint: conversationTurnGoogleSearchEntryPointRef.current || undefined,
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
    const generation = ++assistantRetryGenerationRef.current;
    const current = () => sessionId === latestSessionIdRef.current
      && generation === assistantRetryGenerationRef.current;

    setAssistantRetryBusy(true);
    setAssistantSearchError(null);
    setAssistantSources([]);
    setAssistantSearchQuery("");
    setAssistantSearchElapsedMs(null);
    setAssistantElapsedMs(null);
    setText("");
    updatePhase("polishing");
    try {
      const result = await retryAssistantRequest({ sessionId, request });
      if (!current()) return;
      const finalText = result.trim();
      setText(finalText);
      conversationInitialResponseRef.current = finalText;
      updatePhase(finalText ? "result" : "outcome");
    } catch {
      if (!current()) return;
      setAssistantSearchError(t("subtitle.conversation.searchFailed"));
      updatePhase("result");
    } finally {
      if (current()) setAssistantRetryBusy(false);
    }
  }, [assistantRequest, assistantRetryBusy, updatePhase, t]);

  const assistantOverlayDismissible = interactiveAssistantResult || conversationOpen;

  const scrollRenderedAssistantContent = useCallback(() => {
    const assistant = assistantTextRef.current;
    if (assistant && shouldAutoScrollRef.current) assistant.scrollTop = assistant.scrollHeight;
    const conversation = conversationListRef.current;
    if (conversation && conversationShouldAutoScrollRef.current) {
      conversation.scrollTop = conversation.scrollHeight;
    }
  }, []);

  const renderAssistantContent = (content: string, streaming = false) => (
    <Suspense fallback={content}>
      <MarkdownContent
        content={content}
        streaming={streaming}
        onOpenLink={handleOpenAssistantUrl}
        onContentRendered={scrollRenderedAssistantContent}
      />
    </Suspense>
  );

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
        key={capsuleSessionId}
        className={
          `subtitle-capsule${apiWarning ? " subtitle-capsule-warning" : ""}${fadingOut ? " subtitle-fade-out" : ""}${assistantPanelActive ? " subtitle-capsule-assistant" : ""}${assistantInteractive ? " subtitle-capsule-interactive" : ""}${conversationOpen ? " subtitle-capsule-conversation" : ""}`
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
                <MessageCircle size={15} aria-hidden="true" />
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
                        <MessageCircle size={12} />
                      </span>
                    )}
                    <div className="subtitle-conversation-bubble">
                      {message.role === "assistant" ? renderAssistantContent(message.content) : message.content}
                    </div>
                  </div>
                  {message.role === "assistant" && renderSources(message.sources)}
                  {message.role === "assistant" && message.googleSearchEntryPoint && (
                    <GoogleSearchEntryPoint
                      html={message.googleSearchEntryPoint}
                      label="Google Search"
                      onOpen={handleOpenAssistantUrl}
                    />
                  )}
                </article>
              ))}
              {conversationBusy && (
                <article className="subtitle-conversation-message is-assistant is-streaming">
                  <div className="subtitle-conversation-row">
                    <span className="subtitle-conversation-assistant-mark" aria-hidden="true">
                      <MessageCircle size={12} />
                    </span>
                    <div className="subtitle-conversation-bubble">
                      {conversationDraft ? renderAssistantContent(conversationDraft, true) : t("subtitle.conversation.thinking")}
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
            {(hasText || phase === "starting" || phase === "recording" || indicatorClass) && (
              <span className="subtitle-status-indicator" data-visible={phase === "starting" || phase === "recording" || !!indicatorClass} aria-hidden="true">
                {(phase === "starting" || phase === "recording") ? (
                  <SubtitleWaveform sessionId={capsuleSessionId} assistant={isAssistant} acceptsSample={acceptsWaveformSample} />
                ) : indicatorClass ? (
                  <span className={indicatorClass} />
                ) : null}
              </span>
            )}
            {hasText && (
              <div
                ref={assistantTextRef}
                className={`subtitle-text${isStreaming ? " subtitle-text-streaming" : ""}${polishFlash ? " subtitle-polish-flash" : ""}`}
                onAnimationEnd={(event) => { if (event.target === event.currentTarget) setPolishFlash(false); }}
                role="status"
                aria-live="polite"
                onScroll={assistantPanelActive ? handleAssistantScroll : undefined}
              >
                {interimSegments ? (
                  <>
                    <span className="subtitle-interim-stable">{interimSegments.stableText}</span>
                    <span className="subtitle-interim-tentative" ref={tentativeRef}>
                      {interimSegments.tentativeText}
                    </span>
                  </>
                ) : assistantPanelActive ? renderAssistantContent(phase === "result" ? text : smoothText, phase !== "result")
                  : isAssistant ? smoothText : text}
              </div>
            )}
            {isAssistant && phase === "result" && renderSources(assistantSources)}
            {isAssistant && phase === "result" && assistantGoogleSearchEntryPoint && (
              <GoogleSearchEntryPoint
                html={assistantGoogleSearchEntryPoint}
                label="Google Search"
                onOpen={handleOpenAssistantUrl}
              />
            )}
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
                <span>{assistantSearchError}</span>
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
            {outcomeText && <span key={outcome} className="subtitle-hint" role="status">{outcomeText}</span>}
            {!hasText && !outcomeText && hintText && <span key={phase} className="subtitle-hint">{hintText}</span>}
          </>
        )}
        {apiWarning && <div className="subtitle-api-warning" role="status">{apiWarning}</div>}
      </div>
    </div>
  );
}
