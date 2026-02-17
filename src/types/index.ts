// 与 Rust 后端 funasr_service::TranscriptionResult 对应
export interface TranscriptionResult {
  text: string;
  duration?: number;
  success: boolean;
  error?: string;
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
  timestamp: number;
  timeDisplay: string;
}

