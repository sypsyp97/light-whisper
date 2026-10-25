use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

use super::native_capture::{run_capture, NativeCaptureControl, NativeCaptureSink};
use crate::services::funasr_service::{
    native_stream::{NativeStreamClient, NativeStreamUpdate},
    TranscriptionResult,
};
use crate::state::AppState;
use crate::utils::AppError;

static NEXT_NATIVE_SESSION: AtomicU64 = AtomicU64::new(1);

pub(super) fn stream_config(
    engine: &str,
    r2_config: &crate::state::user_profile::R2T2Config,
) -> crate::state::user_profile::R2T2Config {
    if engine == "confucius4-r2t2" {
        r2_config.clone()
    } else {
        crate::state::user_profile::R2T2Config::default()
    }
}

fn next_caption_update(
    previous: &mut Option<(String, String)>,
    update: &NativeStreamUpdate,
    recording_id: u64,
) -> Option<serde_json::Value> {
    let next = (update.text.clone(), update.tentative_text.clone());
    if previous.as_ref() == Some(&next)
        || (previous.is_none() && next.0.is_empty() && next.1.is_empty())
    {
        return None;
    }
    *previous = Some(next);
    Some(serde_json::json!({
        "sessionId": recording_id,
        "text": format!("{}{}", update.text, update.tentative_text),
        "stableText": update.text,
        "tentativeText": update.tentative_text,
        "interim": !update.is_final,
        "language": update.language,
    }))
}

pub struct NativeRecording {
    control: Option<oneshot::Sender<NativeCaptureControl>>,
    task: tokio::task::JoinHandle<Result<Option<TranscriptionResult>, AppError>>,
}

fn finished_caption_update(
    result: &TranscriptionResult,
    recording_id: u64,
) -> Option<serde_json::Value> {
    if !result.success || result.text.trim().is_empty() {
        return None;
    }
    // ASR has finished, but polishing/history work is still pending. Publish
    // the flushed tail now without starting the overlay's final-result timer.
    Some(serde_json::json!({
        "sessionId": recording_id,
        "text": result.text,
        "stableText": result.text,
        "tentativeText": "",
        "interim": true,
        "language": result.language,
    }))
}

impl NativeRecording {
    pub(crate) async fn finish(mut self) -> Result<TranscriptionResult, AppError> {
        if let Some(control) = self.control.take() {
            let _ = control.send(NativeCaptureControl::Finish);
        }
        self.task
            .await
            .map_err(|_| AppError::Asr("原生 ASR 录音任务异常结束".into()))??
            .ok_or_else(|| AppError::Asr("原生 ASR 录音已取消".into()))
    }

    pub(crate) async fn cancel(mut self) -> Result<(), AppError> {
        if let Some(control) = self.control.take() {
            let _ = control.send(NativeCaptureControl::Cancel);
        }
        self.task
            .await
            .map_err(|_| AppError::Asr("原生 ASR 录音任务异常结束".into()))??;
        Ok(())
    }
}

struct AppSink<'a> {
    state: &'a AppState,
    app: Option<&'a tauri::AppHandle>,
    client: NativeStreamClient,
    chunk_samples: usize,
    coalesce_feeds: bool,
}

impl<'a> AppSink<'a> {
    fn new(
        state: &'a AppState,
        app: Option<&'a tauri::AppHandle>,
        client: NativeStreamClient,
        engine: &str,
    ) -> Self {
        let qwen = engine == "qwen3-asr-0.6b";
        Self {
            state,
            app,
            client,
            // Qwen owns its preview cadence; forward capture every 40 ms so
            // its first hypothesis need not wait for an R2T2-sized batch.
            chunk_samples: if qwen { 640 } else { 2560 },
            coalesce_feeds: qwen,
        }
    }
}

impl NativeCaptureSink for AppSink<'_> {
    fn coalesce_feeds(&self) -> bool {
        self.coalesce_feeds
    }

    fn chunk_samples(&self) -> usize {
        self.chunk_samples
    }

    async fn feed(&mut self, samples: &[i16]) -> Result<NativeStreamUpdate, AppError> {
        if self.coalesce_feeds {
            self.client
                .feed_coalesced(self.state, self.app, samples, true)
                .await
        } else {
            self.client.feed(self.state, self.app, samples).await
        }
    }
    async fn feed_final(&mut self, samples: &[i16]) -> Result<NativeStreamUpdate, AppError> {
        if self.coalesce_feeds {
            self.client
                .feed_coalesced(self.state, self.app, samples, false)
                .await
        } else {
            self.feed(samples).await
        }
    }
    async fn finish(&mut self) -> Result<TranscriptionResult, AppError> {
        self.client.finish(self.state, self.app).await
    }
    async fn cancel(&mut self) -> Result<(), AppError> {
        self.client.cancel(self.state, self.app).await
    }
}

pub(crate) fn spawn_native_recording(
    app: tauri::AppHandle,
    recording_id: u64,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    sample_rate: u32,
) -> NativeRecording {
    let state = app.state::<AppState>();
    let owner = state.engine.native_asr_owner.clone();
    let engine = crate::utils::paths::read_engine_config();
    let config = state.with_profile(|profile| stream_config(&engine, &profile.r2t2));
    let (sender, mut control) = oneshot::channel();
    let task = tokio::spawn(async move {
        // Preserve the mutex queue position if Finish arrives before ownership.
        let lease = owner.lock_owned();
        tokio::pin!(lease);
        let mut finish_waiting = false;
        let _guard = tokio::select! {
            biased;
            command = &mut control => match command {
                Ok(NativeCaptureControl::Finish) => {
                    finish_waiting = true;
                    lease.await
                }
                Ok(NativeCaptureControl::Cancel) | Err(_) => return Ok(None),
            },
            guard = &mut lease => guard,
        };
        let control = if finish_waiting {
            let (sender, receiver) = oneshot::channel();
            let _ = sender.send(NativeCaptureControl::Finish);
            receiver
        } else {
            control
        };
        // Backend identities follow actual ownership order, independently of
        // UI recording ids, so a queued short recording cannot become stale.
        let native_id = NEXT_NATIVE_SESSION.fetch_add(1, Ordering::Relaxed);
        let client = NativeStreamClient::start(
            app.state::<AppState>().inner(),
            Some(&app),
            native_id,
            config.context,
            config.language,
        )
        .await?;
        let state = app.state::<AppState>();
        let sink = AppSink::new(state.inner(), Some(&app), client, &engine);
        let previous_caption = parking_lot::Mutex::new(None);
        let caption_app = app.clone();
        let result = run_capture(sink, samples, sample_rate, control, move |update| {
            if let Some(payload) =
                next_caption_update(&mut previous_caption.lock(), &update, recording_id)
            {
                let _ = caption_app.emit("transcription-result", payload);
            }
        })
        .await?;
        if let Some(payload) = result
            .as_ref()
            .and_then(|result| finished_caption_update(result, recording_id))
        {
            let _ = app.emit("transcription-result", payload);
        }
        Ok(result)
    });
    NativeRecording {
        control: Some(sender),
        task,
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn finished_asr_publishes_full_tail_without_ending_caption_session() {
        let result = TranscriptionResult {
            text: "明天去上海开会".into(),
            duration: Some(2.0),
            success: true,
            error: None,
            language: Some("Chinese".into()),
        };
        let payload = finished_caption_update(&result, 7).unwrap();
        assert_eq!(payload["text"], result.text);
        assert_eq!(payload["stableText"], result.text);
        assert_eq!(payload["tentativeText"], "");
        assert_eq!(payload["sessionId"], 7);
        assert_eq!(payload["language"], "Chinese");
        assert_eq!(payload["interim"], true);
    }

    #[test]
    fn finished_asr_does_not_replace_caption_with_empty_or_failed_output() {
        for (text, success) in [("", true), ("  \n", true), ("partial", false)] {
            let result = TranscriptionResult {
                text: text.into(),
                duration: None,
                success,
                error: None,
                language: None,
            };
            assert!(finished_caption_update(&result, 7).is_none());
        }
    }

    #[test]
    fn captions_publish_tentative_revisions_shrink_and_clear() {
        let mut previous = None;
        let mut update = NativeStreamUpdate {
            text: String::new(),
            tentative_text: "明天去上海".into(),
            language: Some("Chinese".into()),
            sample_count: 5120,
            is_final: false,
        };
        let first = next_caption_update(&mut previous, &update, 7).unwrap();
        assert_eq!(first["text"], "明天去上海");
        assert_eq!(first["stableText"], "");
        assert_eq!(first["tentativeText"], "明天去上海");
        assert_eq!(first["sessionId"], 7);
        assert!(next_caption_update(&mut previous, &update, 7).is_none());
        for preview in ["明天去上班", "明天", ""] {
            update.tentative_text = preview.into();
            assert_eq!(
                next_caption_update(&mut previous, &update, 7).unwrap()["text"],
                preview
            );
        }
        update.text = "明天".into();
        update.tentative_text = "去上班".into();
        let stable = next_caption_update(&mut previous, &update, 7).unwrap();
        assert_eq!(stable["text"], "明天去上班");
        assert_eq!(stable["stableText"], "明天");
        assert_eq!(stable["tentativeText"], "去上班");
    }
}

#[cfg(all(test, target_os = "windows"))]
mod capture_route_tests {
    use super::*;
    use crate::services::funasr_service::ipc_recovery_tests::{
        assert_stream_packet_audit, test_process,
    };

    async fn check_app_sink_route(engine: &str, stop_after_live_feed: bool) {
        let state = AppState::default();
        *state.engine.funasr_process.lock().await = Some(test_process("native_stream_audit").await);
        state.set_funasr_ready(true);
        let client = NativeStreamClient::start(&state, None, 91, String::new(), None)
            .await
            .unwrap();
        // Exercise the same constructor and adapter used by spawn_native_recording.
        // The optional app handle only suppresses GUI error notifications.
        let sink = AppSink::new(&state, None, client, engine);
        let qwen = engine == "qwen3-asr-0.6b";
        let chunk_samples = sink.chunk_samples();
        let coalesces = sink.coalesce_feeds();
        let pcm = (0..160_001)
            .map(|index| (index as i16).wrapping_mul(17))
            .collect::<Vec<_>>();
        let captured = Arc::new(parking_lot::Mutex::new(if stop_after_live_feed {
            pcm[..160_000].to_vec()
        } else {
            pcm.clone()
        }));
        let (control_tx, control_rx) = oneshot::channel();
        let finish_control = parking_lot::Mutex::new(Some(control_tx));
        if !stop_after_live_feed {
            finish_control
                .lock()
                .take()
                .unwrap()
                .send(NativeCaptureControl::Finish)
                .ok()
                .unwrap();
        }
        let updates = parking_lot::Mutex::new(Vec::new());
        let final_sample = pcm[160_000];
        let result = run_capture(sink, captured.clone(), 16_000, control_rx, |update| {
            updates.lock().push(update);
            // A completed real IPC feed is the barrier. Append the final sample
            // and request Stop before the capture loop can send another feed.
            if let Some(sender) = finish_control.lock().take() {
                captured.lock().push(final_sample);
                sender.send(NativeCaptureControl::Finish).ok().unwrap();
            }
        })
        .await
        .unwrap()
        .unwrap();
        let updates = updates.lock();
        let expected_updates = if qwen {
            if stop_after_live_feed {
                2
            } else {
                1
            }
        } else {
            63 // 62 full 160 ms chunks, followed by the exact 1,281-sample tail.
        };
        assert_eq!(
            updates.len(),
            expected_updates,
            "{engine}: capture feed policy"
        );
        assert_eq!(coalesces, qwen);
        assert_eq!(chunk_samples, if qwen { 640 } else { 2560 });
        let expected_modes = if qwen {
            let mut modes = vec![Some(false); if stop_after_live_feed { 33 } else { 32 }];
            if stop_after_live_feed {
                modes[31] = Some(true);
                assert_eq!(updates[0].sample_count, 160_000);
            }
            modes
        } else {
            assert_eq!(
                updates
                    .iter()
                    .map(|update| update.sample_count)
                    .collect::<Vec<_>>(),
                (1..=62)
                    .map(|index| index * 2560)
                    .chain([160_001])
                    .collect::<Vec<_>>()
            );
            vec![None; 63]
        };
        assert_stream_packet_audit(updates.last().unwrap(), &pcm, &expected_modes);
        assert!(result.success);
        assert_eq!(result.text, "finished");
        assert_eq!(result.duration, Some(pcm.len() as f64 / 16_000.0));
    }

    #[tokio::test]
    async fn production_qwen_sink_coalesces_live_backlog_then_disables_preview_for_stop_tail() {
        check_app_sink_route("qwen3-asr-0.6b", true).await;
    }

    #[tokio::test]
    async fn production_qwen_sink_stop_before_capture_never_requests_preview() {
        check_app_sink_route("qwen3-asr-0.6b", false).await;
    }

    #[tokio::test]
    async fn production_r2t2_sink_keeps_legacy_live_and_stop_packets() {
        check_app_sink_route("confucius4-r2t2", true).await;
    }
}
