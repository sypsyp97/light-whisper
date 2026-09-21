use std::{future::Future, sync::Arc, time::Duration};

use tokio::sync::{oneshot, oneshot::error::TryRecvError};
use tokio::time::MissedTickBehavior;

use super::resample::ChunkedResampler;
use crate::services::funasr_service::{native_stream::NativeStreamUpdate, TranscriptionResult};
use crate::utils::AppError;

// Poll more often than the decode cadence to avoid an extra full-chunk wait
// when capture packets arrive just after a poll. Partial audio stays local.
const CAPTURE_INTERVAL: Duration = Duration::from_millis(40);
const NATIVE_CHUNK_SAMPLES: usize = 2560; // 160 ms at 16 kHz.

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

#[derive(Default)]
struct PendingOutput {
    samples: Vec<i16>,
    offset: usize,
}

impl PendingOutput {
    fn append(&mut self, output: &mut Vec<i16>) {
        self.samples.append(output);
    }

    fn remaining(&self) -> &[i16] {
        &self.samples[self.offset..]
    }

    fn consume(&mut self, count: usize) {
        self.offset += count;
        debug_assert!(self.offset <= self.samples.len());
    }

    fn compact(&mut self) {
        if self.offset == 0 {
            return;
        }
        if self.offset == self.samples.len() {
            self.samples.clear();
            self.offset = 0;
            return;
        }
        self.samples.drain(..self.offset);
        self.offset = 0;
    }
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
    let mut pending = PendingOutput::default();
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
                        &mut pending,
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
                            &mut pending,
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

                pending.append(&mut output);
                match feed_ready(&mut sink, &mut pending, &mut control, &mut on_update).await {
                    Ok(None) => {}
                    Ok(Some(NativeCaptureControl::Cancel)) => {
                        return cancel_capture(&mut sink).await;
                    }
                    Ok(Some(NativeCaptureControl::Finish)) => {
                        return finish_capture(
                            &mut sink,
                            &samples,
                            &mut raw_offset,
                            &mut resampler,
                            &mut pending,
                            &mut on_update,
                        )
                        .await;
                    }
                    Err(error) => {
                        let _ = sink.cancel().await;
                        return Err(error);
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

async fn feed_ready<S: NativeCaptureSink, F: Fn(NativeStreamUpdate) + Send>(
    sink: &mut S,
    pending: &mut PendingOutput,
    control: &mut oneshot::Receiver<NativeCaptureControl>,
    on_update: &mut F,
) -> Result<Option<NativeCaptureControl>, AppError> {
    loop {
        if let Some(action) = poll_control(control) {
            pending.compact();
            return Ok(Some(action));
        }
        if pending.remaining().len() < NATIVE_CHUNK_SAMPLES {
            pending.compact();
            return Ok(None);
        }

        let update = sink
            .feed(&pending.remaining()[..NATIVE_CHUNK_SAMPLES])
            .await?;
        pending.consume(NATIVE_CHUNK_SAMPLES);
        on_update(update);
    }
}

async fn feed_full<S: NativeCaptureSink, F: Fn(NativeStreamUpdate) + Send>(
    sink: &mut S,
    pending: &mut PendingOutput,
    on_update: &mut F,
) -> Result<(), AppError> {
    while pending.remaining().len() >= NATIVE_CHUNK_SAMPLES {
        let update = sink
            .feed(&pending.remaining()[..NATIVE_CHUNK_SAMPLES])
            .await?;
        pending.consume(NATIVE_CHUNK_SAMPLES);
        on_update(update);
    }
    pending.compact();
    Ok(())
}

async fn feed_all<S: NativeCaptureSink, F: Fn(NativeStreamUpdate) + Send>(
    sink: &mut S,
    pending: &mut PendingOutput,
    on_update: &mut F,
) -> Result<(), AppError> {
    while !pending.remaining().is_empty() {
        let count = pending.remaining().len().min(NATIVE_CHUNK_SAMPLES);
        let update = sink.feed(&pending.remaining()[..count]).await?;
        pending.consume(count);
        on_update(update);
    }
    pending.compact();
    Ok(())
}

async fn finish_capture<S: NativeCaptureSink, F: Fn(NativeStreamUpdate) + Send>(
    sink: &mut S,
    samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    raw_offset: &mut usize,
    resampler: &mut ChunkedResampler,
    pending: &mut PendingOutput,
    on_update: &mut F,
) -> Result<Option<TranscriptionResult>, AppError> {
    if let Err(error) = feed_full(sink, pending, on_update).await {
        let _ = sink.cancel().await;
        return Err(error);
    }

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
    pending.append(&mut output);
    if let Err(error) = resampler.finish(&mut output) {
        let _ = sink.cancel().await;
        return Err(AppError::Asr(format!(
            "R2T2 capture resampler flush failed: {error}"
        )));
    }
    pending.append(&mut output);
    if let Err(error) = feed_all(sink, pending, on_update).await {
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
