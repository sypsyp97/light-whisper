import { invoke } from "@tauri-apps/api/core";
import type {
  TranscriptionResult,
  FunASRStatus,
  ModelCheckResult,
} from "../types";

/**
 * Start the FunASR speech recognition engine.
 * Rust returns Result<String, AppError> — resolves to a plain string on success.
 */
export async function startFunASR(): Promise<string> {
  return invoke<string>("start_funasr");
}

/**
 * Transcribe audio data using FunASR.
 * @param audioData - Raw PCM audio samples as an array of bytes (WAV format).
 */
export async function transcribeAudio(
  audioData: number[]
): Promise<TranscriptionResult> {
  return invoke<TranscriptionResult>("transcribe_audio", {
    audioData,
  });
}

/**
 * Check the current status of the FunASR engine.
 */
export async function checkFunASRStatus(): Promise<FunASRStatus> {
  return invoke<FunASRStatus>("check_funasr_status");
}

/**
 * Check whether required model files are present and complete on disk.
 */
export async function checkModelFiles(): Promise<ModelCheckResult> {
  return invoke<ModelCheckResult>("check_model_files");
}

/**
 * Download missing or incomplete model files.
 * Rust returns Result<String, AppError> — resolves to a plain string on success.
 */
export async function downloadModels(): Promise<string> {
  return invoke<string>("download_models");
}

/**
 * Cancel the current model download task.
 */
export async function cancelModelDownload(): Promise<string> {
  return invoke<string>("cancel_model_download");
}

/**
 * Restart the FunASR engine.
 * Rust returns Result<String, AppError> — resolves to a plain string on success.
 */
export async function restartFunASR(): Promise<string> {
  return invoke<string>("restart_funasr");
}
