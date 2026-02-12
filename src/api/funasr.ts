import type {
  TranscriptionResult,
  FunASRStatus,
  ModelCheckResult,
} from "../types";
import { invokeCommand } from "./invoke";

/**
 * Start the FunASR speech recognition engine.
 * Rust returns Result<String, AppError> — resolves to a plain string on success.
 */
export async function startFunASR(): Promise<string> {
  return invokeCommand<string>("start_funasr");
}

/**
 * Transcribe audio data using FunASR.
 * @param audioBase64 - WAV audio data encoded as a base64 string.
 */
export async function transcribeAudio(
  audioBase64: string
): Promise<TranscriptionResult> {
  return invokeCommand<TranscriptionResult>("transcribe_audio", {
    audioBase64,
  });
}

/**
 * Check the current status of the FunASR engine.
 */
export async function checkFunASRStatus(): Promise<FunASRStatus> {
  return invokeCommand<FunASRStatus>("check_funasr_status");
}

/**
 * Check whether required model files are present and complete on disk.
 */
export async function checkModelFiles(): Promise<ModelCheckResult> {
  return invokeCommand<ModelCheckResult>("check_model_files");
}

/**
 * Download missing or incomplete model files.
 * Rust returns Result<String, AppError> — resolves to a plain string on success.
 */
export async function downloadModels(): Promise<string> {
  return invokeCommand<string>("download_models");
}

/**
 * Cancel the current model download task.
 */
export async function cancelModelDownload(): Promise<string> {
  return invokeCommand<string>("cancel_model_download");
}

/**
 * Restart the FunASR engine.
 * Rust returns Result<String, AppError> — resolves to a plain string on success.
 */
export async function restartFunASR(): Promise<string> {
  return invokeCommand<string>("restart_funasr");
}

/**
 * Get the current speech recognition engine ("sensevoice" or "whisper").
 */
export async function getEngine(): Promise<string> {
  return invokeCommand<string>("get_engine");
}

/**
 * Set the speech recognition engine and stop the current service.
 * After calling this, use retryModel() to restart with the new engine.
 */
export async function setEngine(engine: string): Promise<string> {
  return invokeCommand<string>("set_engine", { engine });
}
