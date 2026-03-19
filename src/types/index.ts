// 与 Rust 后端 funasr_service::TranscriptionResult 对应
export interface TranscriptionResult {
  text: string;
  duration?: number;
  success: boolean;
  error?: string;
}

export type RecordingMode = "dictation" | "assistant";

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
  custom_providers?: CustomProvider[];
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
  blocked_hot_words?: string[];
}

