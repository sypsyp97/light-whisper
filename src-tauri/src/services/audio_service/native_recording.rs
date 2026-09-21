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

impl NativeRecording {
    pub(crate) async fn finish(mut self) -> Result<TranscriptionResult, AppError> {
        if let Some(control) = self.control.take() {
            let _ = control.send(NativeCaptureControl::Finish);
        }
        self.task
            .await
            .map_err(|_| AppError::Asr("R2T2 录音任务异常结束".into()))??
            .ok_or_else(|| AppError::Asr("R2T2 录音已取消".into()))
    }

    pub(crate) async fn cancel(mut self) -> Result<(), AppError> {
        if let Some(control) = self.control.take() {
            let _ = control.send(NativeCaptureControl::Cancel);
        }
        self.task
            .await
            .map_err(|_| AppError::Asr("R2T2 录音任务异常结束".into()))??;
        Ok(())
    }
}

struct AppSink {
    app: tauri::AppHandle,
    client: NativeStreamClient,
}

impl NativeCaptureSink for AppSink {
    async fn feed(&mut self, samples: &[i16]) -> Result<NativeStreamUpdate, AppError> {
        self.client
            .feed(
                self.app.state::<AppState>().inner(),
                Some(&self.app),
                samples,
            )
            .await
    }
    async fn finish(&mut self) -> Result<TranscriptionResult, AppError> {
        self.client
            .finish(self.app.state::<AppState>().inner(), Some(&self.app))
            .await
    }
    async fn cancel(&mut self) -> Result<(), AppError> {
        self.client
            .cancel(self.app.state::<AppState>().inner(), Some(&self.app))
            .await
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
    let config = state.with_profile(|profile| profile.r2t2.clone());
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
        let sink = AppSink {
            app: app.clone(),
            client,
        };
        let previous_caption = parking_lot::Mutex::new(None);
        run_capture(sink, samples, sample_rate, control, move |update| {
            if let Some(payload) =
                next_caption_update(&mut previous_caption.lock(), &update, recording_id)
            {
                let _ = app.emit("transcription-result", payload);
            }
        })
        .await
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
