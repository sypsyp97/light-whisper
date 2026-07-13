// 与 Rust 后端 funasr_service::TranscriptionResult 对应
export interface TranscriptionResult {
  text: string;
  duration?: number;
  success: boolean;
  error?: string;
}

export type RecordingMode = "dictation" | "assistant";
export type EditGrabStatus = "ok" | "timeout" | "empty" | "unsupported";
export type TranscriptionResultStage = "raw" | "polished";

export interface TranscriptionTiming {
  asrMs?: number;
  polishMs?: number;
  totalMs?: number;
  rawFirst?: {
    status:
      | "preview_only"
      | "pasted"
      | "replaced"
      | "kept_raw"
      | "final_fallback"
      | "unchanged";
  };
}

// 与 Rust 后端 funasr_service::FunASRStatus 对应
export interface FunASRStatus {
  running: boolean;
  ready: boolean;
  model_loaded: boolean;
  device?: string;
  gpu_name?: string;
  gpu_memory_total?: number;
  message: string;
  engine?: string;
  models_present?: boolean;
  missing_models?: string[];
}

// 与 Rust 后端 funasr_service::ModelCheckResult 对应
export interface ModelCheckResult {
  all_present: boolean;
  asr_model: boolean;
  vad_model: boolean;
  punc_model: boolean;
  engine?: string;
  cache_path: string;
  missing_models: string[];
}

// 转录历史记录
export interface HistoryItem {
  id: string;
  text: string;
  originalText: string;
  timestamp: number;
  timeDisplay: string;
  editGrabStatus?: EditGrabStatus;
  resultStage?: TranscriptionResultStage;
  timing?: TranscriptionTiming;
}

export interface InputDeviceInfo {
  name: string;
  isDefault: boolean;
}

export interface InputDeviceListPayload {
  devices: InputDeviceInfo[];
  selectedDeviceName?: string | null;
}

export interface AppUpdateInfo {
  available: boolean;
  currentVersion: string;
  latestVersion?: string | null;
  notes?: string | null;
  publishedAt?: string | null;
  releaseUrl?: string | null;
}

export interface HotkeyDiagnostic {
  shortcut: string;
  registered: boolean;
  backend: string;
  isPressed: boolean;
  lastError?: string | null;
  warning?: string | null;
  /** Non-empty when another program has registered the same hotkey */
  systemConflict?: string | null;
  lastEvent?: string | null;
  lastEventAtMs?: number | null;
  lastRegisteredAtMs?: number | null;
  lastPressedAtMs?: number | null;
  lastReleasedAtMs?: number | null;
}

export interface AiModelInfo {
  id: string;
  ownedBy?: string | null;
}

export interface AiModelListPayload {
  models: AiModelInfo[];
  sourceUrl: string;
}

export interface OpenaiCodexOauthStatus {
  loggedIn: boolean;
  email?: string | null;
  planType?: string | null;
  accountId?: string | null;
  expiresAtMs?: number | null;
}

export interface OpenaiCodexOauthDeviceCodeChallenge {
  verificationUrl: string;
  userCode: string;
  deviceAuthId: string;
  intervalSecs: number;
}

// 热词来源
export type HotWordSource = "user" | "learned";

// 热词条目
export interface HotWord {
  text: string;
  weight: number;
  source: HotWordSource;
  use_count: number;
  last_used: number;
}

// 纠错来源
export type CorrectionSource = "ai" | "user";

// 纠错模式
export interface CorrectionPattern {
  original: string;
  corrected: string;
  count: number;
  last_seen: number;
  source: CorrectionSource;
}

// 联网搜索方式
export type WebSearchProvider = "model_native" | "exa" | "tavily" | "google";

// 联网搜索配置
export interface WebSearchConfig {
  enabled: boolean;
  provider: WebSearchProvider;
  max_results: number;
}

// API 协议格式
export type ApiFormat = "openai_compat" | "anthropic";
export type LlmReasoningMode =
  | "provider_default"
  | "off"
  | "light"
  | "balanced"
  | "deep";

export interface LlmReasoningSupport {
  supported: boolean;
  strategy?: string | null;
  summary: string;
}

// 用户自定义 LLM 服务商
export interface CustomProvider {
  id: string;
  name: string;
  base_url: string;
  model: string;
  api_format: ApiFormat;
}

// OpenAI 认证方式（仅当 active / assistant provider 为 openai 时生效）
export type OpenaiAuthMode = "api_key" | "oauth";

// LLM 后端配置
export interface LlmProviderConfig {
  active: string;
  custom_base_url?: string;
  custom_model?: string;
  reasoning_mode?: LlmReasoningMode;
  polish_reasoning_mode?: LlmReasoningMode;
  assistant_reasoning_mode?: LlmReasoningMode;
  assistant_use_separate_model?: boolean;
  assistant_model?: string;
  assistant_provider?: string;
  selection_reasoning_mode?: LlmReasoningMode;
  selection_use_separate_model?: boolean;
  selection_model?: string;
  selection_provider?: string;
  custom_providers?: CustomProvider[];
  validation_use_separate_model?: boolean;
  validation_provider?: string | null;
  validation_model?: string | null;
  openai_auth_mode?: OpenaiAuthMode | null;
  openai_fast_mode?: boolean;
}

// 用户画像
export interface UserProfile {
  hot_words: HotWord[];
  correction_patterns: CorrectionPattern[];
  vocab_frequency: Record<string, { count: number; last_seen: number }>;
  total_transcriptions: number;
  last_updated: number;
  llm_provider: LlmProviderConfig;
  translation_target?: string | null;
  translation_hotkey?: string | null;
  custom_prompt?: string | null;
  assistant_hotkey?: string | null;
  assistant_system_prompt?: string | null;
  assistant_screen_context_enabled?: boolean;
  ai_polish_screen_context_enabled?: boolean;
  selection_assistant?: SelectionAssistantConfig;
  blocked_hot_words?: string[];
  web_search?: WebSearchConfig;
  correction_validation_enabled?: boolean;
  last_correction_validation?: number;
  history_settings?: HistorySettings;
  app_profile_rules?: AppProfileRule[];
}

export interface HistorySettings {
  enabled: boolean;
  save_audio: boolean;
  retention_days: number;
}

export type AppRuleOverride = "inherit" | "enabled" | "disabled";
export type AppTranslationOverride = "inherit" | "disabled" | "target";

export interface AppProfileRule {
  id: string;
  name: string;
  enabled: boolean;
  process_name: string;
  window_title_contains?: string | null;
  ai_polish: AppRuleOverride;
  translation: AppTranslationOverride;
  translation_target?: string | null;
  screen_context: AppRuleOverride;
  history: AppRuleOverride;
  custom_prompt?: string | null;
}

export type PersistentHistoryStatus =
  | "success"
  | "asr_error"
  | "processing_error"
  | "no_speech";

export interface PersistentHistoryRecord {
  id: number;
  sessionId: number;
  createdAt: number;
  updatedAt: number;
  mode: RecordingMode;
  workflow: "dictation" | "assistant" | "edit";
  status: PersistentHistoryStatus;
  text: string;
  originalText: string;
  sourceText?: string | null;
  durationSec?: number | null;
  language?: string | null;
  engine: string;
  provider?: string | null;
  model?: string | null;
  appProcess?: string | null;
  appWindowTitle?: string | null;
  appRuleName?: string | null;
  audioAvailable: boolean;
  asrMs?: number | null;
  polishMs?: number | null;
  totalMs?: number | null;
  rawFirstStatus?: string | null;
  error?: string | null;
  reprocessedFromId?: number | null;
}

export interface PersistentHistoryFilter {
  query?: string;
  mode?: "" | RecordingMode;
  status?: "" | "success" | "failed";
  limit?: number;
  offset?: number;
}

export interface PersistentHistoryPage {
  items: PersistentHistoryRecord[];
  total: number;
  hasMore: boolean;
}

export interface LatencyStats {
  p50Ms?: number | null;
  p95Ms?: number | null;
}

export interface PersistentHistoryStats {
  total: number;
  success: number;
  failed: number;
  totalCharacters: number;
  asr: LatencyStats;
  polish: LatencyStats;
  totalLatency: LatencyStats;
}

export interface SelectionAssistantConfig {
  enabled: boolean;
  auto_screenshot?: boolean;
  min_chars: number;
  max_chars: number;
  translation_target: string;
  excluded_apps: string[];
}

