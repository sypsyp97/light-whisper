use std::{future::Future, sync::Arc, time::Duration};

use tokio::sync::{oneshot, oneshot::error::TryRecvError};
use tokio::time::MissedTickBehavior;

use super::resample::ChunkedResampler;
use crate::services::funasr_service::{native_stream::NativeStreamUpdate, TranscriptionResult};
use crate::utils::AppError;

const CAPTURE_INTERVAL: Duration = Duration::from_millis(160);
const NATIVE_CHUNK_SAMPLES: usize = 5120;

pub(crate) enum NativeCaptureControl {
    Finish,
    Cancel,
}

pub(super) trait NativeCaptureSink: Send {
    fn feed(
        &mut self,
        samples: &[i16],
    ) -> impl Future<Output = Result<NativeStreamUpdate, AppError>> + Send;
    fn finish(&mut self) -> impl Future<Output = Result<TranscriptionResult, AppError>> + Send;
    fn cancel(&mut self) -> impl Future<Output = Result<(), AppError>> + Send;
}

pub(super) async fn run_capture<S: NativeCaptureSink>(
    mut sink: S,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    sample_rate: u32,
    mut control: oneshot::Receiver<NativeCaptureControl>,
    on_update: impl Fn(NativeStreamUpdate) + Send,
) -> Result<Option<TranscriptionResult>, AppError> {
    let mut on_update = on_update;
    let mut resampler = match ChunkedResampler::new(sample_rate) {
        Ok(resampler) => resampler,
        Err(error) => {
            let _ = sink.cancel().await;
            return Err(AppError::Asr(format!(
                "R2T2 capture resampler initialization failed: {error}"
            )));
        }
    };
    let mut raw_offset = 0usize;
    let mut interval = tokio::time::interval(CAPTURE_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        if let Some(action) = poll_control(&mut control) {
            match action {
                NativeCaptureControl::Cancel => return cancel_capture(&mut sink).await,
                NativeCaptureControl::Finish => {
                    return finish_capture(
                        &mut sink,
                        &samples,
                        &mut raw_offset,
                        &mut resampler,
                        &mut on_update,
                    )
                    .await;
                }
            }
        }

        tokio::select! {
            biased;
            command = &mut control => {
                match command {
                    Ok(NativeCaptureControl::Cancel) | Err(_) => {
                        return cancel_capture(&mut sink).await;
                    }
                    Ok(NativeCaptureControl::Finish) => {
                        return finish_capture(
                            &mut sink,
                            &samples,
                            &mut raw_offset,
                            &mut resampler,
                            &mut on_update,
                        )
                        .await;
                    }
                }
            }
            _ = interval.tick() => {
                let raw = match copy_new_samples(&samples, &mut raw_offset) {
                    Ok(raw) => raw,
                    Err(error) => {
                        let _ = sink.cancel().await;
                        return Err(error);
                    }
                };
                let mut output = Vec::new();
                if let Err(error) = resampler.process_chunk(&raw, &mut output) {
                    let _ = sink.cancel().await;
                    return Err(AppError::Asr(format!("R2T2 capture resampling failed: {error}")));
                }

                let mut offset = 0usize;
                while offset < output.len() {
                    let end = (offset + NATIVE_CHUNK_SAMPLES).min(output.len());
                    if let Err(error) = feed_chunk(&mut sink, &output[offset..end], &mut on_update).await {
                        let _ = sink.cancel().await;
                        return Err(error);
                    }
                    offset = end;

                    match poll_control(&mut control) {
                        None => {}
                        Some(NativeCaptureControl::Cancel) => {
                            return cancel_capture(&mut sink).await;
                        }
                        Some(NativeCaptureControl::Finish) => {
                            // Finish drains the output already produced from
                            // the copied batch before reading any newly
                            // appended raw samples. It never polls control
                            // again after consuming Finish.
                            if offset < output.len() {
                                if let Err(error) = feed_output(
                                    &mut sink,
                                    &output[offset..],
                                    &mut on_update,
                                )
                                .await
                                {
                                    let _ = sink.cancel().await;
                                    return Err(error);
                                }
                            }
                            return finish_capture(
                                &mut sink,
                                &samples,
                                &mut raw_offset,
                                &mut resampler,
                                &mut on_update,
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }
}

fn poll_control(
    control: &mut oneshot::Receiver<NativeCaptureControl>,
) -> Option<NativeCaptureControl> {
    match control.try_recv() {
        Ok(action) => Some(action),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Closed) => Some(NativeCaptureControl::Cancel),
    }
}

fn copy_new_samples(
    samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    raw_offset: &mut usize,
) -> Result<Vec<i16>, AppError> {
    let captured = samples.lock();
    if captured.len() < *raw_offset {
        return Err(AppError::Asr(
            "R2T2 capture sample buffer regressed".to_string(),
        ));
    }
    let new_samples = captured[*raw_offset..].to_vec();
    *raw_offset = captured.len();
    Ok(new_samples)
}

async fn feed_chunk<S: NativeCaptureSink, F: Fn(NativeStreamUpdate) + Send>(
    sink: &mut S,
    samples: &[i16],
    on_update: &mut F,
) -> Result<(), AppError> {
    if samples.is_empty() {
        return Ok(());
    }
    let update = sink.feed(samples).await?;
    on_update(update);
    Ok(())
}

async fn feed_output<S: NativeCaptureSink, F: Fn(NativeStreamUpdate) + Send>(
    sink: &mut S,
    output: &[i16],
    on_update: &mut F,
) -> Result<(), AppError> {
    for chunk in output.chunks(NATIVE_CHUNK_SAMPLES) {
        feed_chunk(sink, chunk, on_update).await?;
    }
    Ok(())
}

async fn finish_capture<S: NativeCaptureSink, F: Fn(NativeStreamUpdate) + Send>(
    sink: &mut S,
    samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    raw_offset: &mut usize,
    resampler: &mut ChunkedResampler,
    on_update: &mut F,
) -> Result<Option<TranscriptionResult>, AppError> {
    let raw = match copy_new_samples(samples, raw_offset) {
        Ok(raw) => raw,
        Err(error) => {
            let _ = sink.cancel().await;
            return Err(error);
        }
    };
    let mut output = Vec::new();
    if let Err(error) = resampler.process_chunk(&raw, &mut output) {
        let _ = sink.cancel().await;
        return Err(AppError::Asr(format!(
            "R2T2 capture resampling failed: {error}"
        )));
    }
    if let Err(error) = resampler.finish(&mut output) {
        let _ = sink.cancel().await;
        return Err(AppError::Asr(format!(
            "R2T2 capture resampler flush failed: {error}"
        )));
    }
    if let Err(error) = feed_output(sink, &output, on_update).await {
        let _ = sink.cancel().await;
        return Err(error);
    }
    let result = sink.finish().await?;
    Ok(Some(result))
}

async fn cancel_capture<S: NativeCaptureSink>(
    sink: &mut S,
) -> Result<Option<TranscriptionResult>, AppError> {
    sink.cancel().await?;
    Ok(None)
}
