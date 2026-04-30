use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::{Emitter, Manager};

use super::resample::ResamplerState;
use super::{
    INTERIM_HEAVY_COST_MS, INTERIM_INTERVAL_BASE_MS, INTERIM_INTERVAL_DOWN_STEP_MS,
    INTERIM_INTERVAL_MAX_MS, INTERIM_INTERVAL_MIN_MS, INTERIM_INTERVAL_UP_STEP_MS,
    INTERIM_LIGHT_COST_MS, INTERIM_MAX_AUDIO_WINDOW_SEC, MIN_INTERIM_DURATION_SEC,
    MIN_SAMPLES_GROWTH, TARGET_SAMPLE_RATE,
};
use crate::services::funasr_service;
use crate::state::AppState;
use crate::utils::paths;

// ---------- 中间转写循环 ----------

pub fn spawn_interim_loop(
    app_handle: tauri::AppHandle,
    session_id: u64,
    stop_flag: Arc<AtomicBool>,
    stop_notify: Arc<tokio::sync::Notify>,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    sample_rate: u32,
    interim_cache: Arc<parking_lot::Mutex<Option<crate::state::InterimCache>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let state = app_handle.state::<AppState>();
        let mut interval_ms = INTERIM_INTERVAL_BASE_MS;
        let mut last_sample_count: usize = 0;
        // 会话级重采样缓存：只对新增的原始增量执行一次重采样，结果追加到这里
        // 原生 16k 设备时与原始数据相同（resample_to_16k 走零拷贝路径）
        let mut resampled_cache: Vec<i16> = Vec::new();
        // 已写入 resampled_cache 的原始样本数（raw sample index）
        let mut raw_processed: usize = 0;
        let needs_resample = sample_rate != 0 && sample_rate != TARGET_SAMPLE_RATE;
        let mut resample_failed = false;
        let mut resampler = None;
        let max_output_tail = (TARGET_SAMPLE_RATE as f64 * INTERIM_MAX_AUDIO_WINDOW_SEC) as usize;

        if sample_rate == 0 {
            log::error!("中间转写启动失败：采样率为 0 (session {})", session_id);
            return;
        }
        if needs_resample {
            match ResamplerState::new(sample_rate) {
                Ok(value) => {
                    resampler = Some(value);
                }
                Err(err) => {
                    resample_failed = true;
                    log::warn!(
                        "中间转写重采样初始化失败，保留原始采样率 {}Hz: {}",
                        sample_rate,
                        err
                    );
                }
            }
        }

        // 在线引擎不做中间转写（每次请求花钱且有网络延迟）
        if paths::is_online_engine(&paths::read_engine_config()) {
            log::info!("在线引擎，跳过中间转写 (session {})", session_id);
            stop_notify.notified().await;
            return;
        }

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(interval_ms)) => {}
                _ = stop_notify.notified() => { break; }
            }
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            // 只把新增的原始样本拷贝出来，锁持有时间最短
            let (delta, current_count) = {
                let guard = samples.lock();
                let count = guard.len();
                if count.saturating_sub(last_sample_count) < MIN_SAMPLES_GROWTH {
                    interval_ms = adjust_interval(interval_ms, false, 0);
                    continue;
                }
                if (count as f64 / sample_rate as f64) < MIN_INTERIM_DURATION_SEC {
                    continue;
                }
                let delta: Vec<i16> = guard[raw_processed..count].to_vec();
                (delta, count)
            };

            let start = std::time::Instant::now();

            // 只对增量重采样（原生 16k 时是零拷贝），追加到会话缓存
            if !delta.is_empty() {
                if needs_resample && !resample_failed {
                    if let Some(resampler) = resampler.as_mut() {
                        match resampler.push_i16(&delta) {
                            Ok(delta_resampled) => {
                                resampled_cache.extend_from_slice(delta_resampled.as_ref());
                            }
                            Err(err) => {
                                resample_failed = true;
                                resampled_cache.clear();
                                log::warn!(
                                    "中间转写重采样失败，后续保留原始采样率 {}Hz: {}",
                                    sample_rate,
                                    err
                                );
                            }
                        }
                    } else {
                        resample_failed = true;
                        resampled_cache.clear();
                    }
                } else if resample_failed {
                    // fallback 分支会直接从原始 samples 取尾部，避免把非 16k 音频送成 16k。
                } else {
                    resampled_cache.extend_from_slice(&delta);
                }
                raw_processed = current_count;

                // 限制缓存增长：超过 2 倍窗口时丢弃最老的部分，只保留尾部一倍多一点
                if resampled_cache.len() > 2 * max_output_tail {
                    let drop_n = resampled_cache.len() - max_output_tail;
                    resampled_cache.drain(..drop_n);
                }
            }

            // 把最后 12s 送给 Python；重采样失败时保留原始采样率。
            let raw_tail;
            let (interim_samples, interim_sample_rate) = if resample_failed {
                let max_raw_tail = (sample_rate as f64 * INTERIM_MAX_AUDIO_WINDOW_SEC) as usize;
                let guard = samples.lock();
                let tail_start = guard.len().saturating_sub(max_raw_tail);
                raw_tail = guard[tail_start..].to_vec();
                (raw_tail.as_slice(), sample_rate)
            } else {
                let tail_start = resampled_cache.len().saturating_sub(max_output_tail);
                (&resampled_cache[tail_start..], TARGET_SAMPLE_RATE)
            };
            let covered_sample_count =
                current_count.min((sample_rate as f64 * INTERIM_MAX_AUDIO_WINDOW_SEC) as usize);

            match funasr_service::transcribe_pcm16(
                state.inner(),
                interim_samples,
                interim_sample_rate,
                &app_handle,
            )
            .await
            {
                Ok(result) if result.success && !result.text.is_empty() => {
                    let _ = app_handle.emit(
                        "transcription-result",
                        serde_json::json!({
                            "sessionId": session_id,
                            "text": &result.text,
                            "interim": true,
                            "language": &result.language,
                        }),
                    );
                    *interim_cache.lock() = Some(crate::state::InterimCache {
                        text: result.text,
                        language: result.language,
                        sample_count: covered_sample_count,
                    });
                    last_sample_count = current_count;
                }
                _ => {}
            }

            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            interval_ms = adjust_interval(interval_ms, true, start.elapsed().as_millis() as u64);
        }
        log::info!("中间转写循环结束 (session {})", session_id);
    })
}

fn adjust_interval(current: u64, executed: bool, elapsed_ms: u64) -> u64 {
    if !executed {
        return current
            .saturating_sub(8)
            .clamp(INTERIM_INTERVAL_MIN_MS, INTERIM_INTERVAL_BASE_MS);
    }
    if elapsed_ms >= INTERIM_HEAVY_COST_MS {
        (current + INTERIM_INTERVAL_UP_STEP_MS).min(INTERIM_INTERVAL_MAX_MS)
    } else if elapsed_ms <= INTERIM_LIGHT_COST_MS {
        current
            .saturating_sub(INTERIM_INTERVAL_DOWN_STEP_MS)
            .max(INTERIM_INTERVAL_MIN_MS)
    } else {
        match current.cmp(&INTERIM_INTERVAL_BASE_MS) {
            std::cmp::Ordering::Greater => current.saturating_sub(8).max(INTERIM_INTERVAL_BASE_MS),
            std::cmp::Ordering::Less => (current + 4).min(INTERIM_INTERVAL_BASE_MS),
            std::cmp::Ordering::Equal => current,
        }
    }
}
