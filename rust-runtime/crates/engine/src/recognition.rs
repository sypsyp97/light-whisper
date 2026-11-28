#[path = "segmented.rs"]
mod segmented;

use light_whisper_native_asr::{Backend, Family, NativeAsr, Transcript};
use light_whisper_vad::FireRedVad;
use segmented::Segmented;
use std::{
    collections::VecDeque,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

type R<T> = Result<T, String>;
fn language(lang: &str) -> Option<String> {
    (!lang.is_empty()).then(|| lang.to_owned())
}
fn elapsed(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

#[derive(Clone)]
struct Spec {
    resources: PathBuf,
    model: PathBuf,
    family: Family,
    backend: Backend,
    threads: i32,
}
impl Spec {
    fn open(&self) -> R<NativeAsr> {
        let directory = match self.backend {
            Backend::Cuda => "cuda",
            Backend::Vulkan => "vulkan",
            Backend::Cpu => "cpu",
        };
        let dll = self
            .resources
            .join("r2t2-native")
            .join(directory)
            .join("audiocpp.dll");
        let mut native =
            NativeAsr::open(&dll, &self.model, self.family, self.backend, self.threads)?;
        if self.family == Family::R2t2 && self.backend == Backend::Cuda {
            native.start("", None)?;
            native.feed(&vec![0.; native.chunk_samples()])?;
            native.reset()?;
        } else if self.family == Family::Qwen {
            // Warm inference is best-effort in the original Qwen server.
            if let Err(err) = native.transcribe(&[0.; 16000], "", None) {
                eprintln!("Qwen warmup: {err}");
            }
        }
        Ok(native)
    }
}

#[derive(Default)]
pub struct Offline {
    pub output: Transcript,
    pub vad_segments: usize,
    pub speech_samples: usize,
    pub vad_ms: f64,
    pub inference_ms: f64,
}

// One running task and one replaceable pending task per stage. Replacing a
// task drops its PCM and reply sender immediately. Native handles never leave
// the thread on which they were created.
type Task<T> = Box<dyn FnOnce(&mut T) + Send>;
struct Port<T> {
    pending: Arc<Mutex<Option<Task<T>>>>,
    wake: SyncSender<()>,
}
impl<T> Clone for Port<T> {
    fn clone(&self) -> Self {
        Self {
            pending: Arc::clone(&self.pending),
            wake: self.wake.clone(),
        }
    }
}
impl<T> Port<T> {
    fn submit(&self, job: impl FnOnce(&mut T) + Send + 'static) -> R<()> {
        *self
            .pending
            .lock()
            .map_err(|_| "recognition queue poisoned")? = Some(Box::new(job));
        match self.wake.try_send(()) {
            Ok(()) | Err(mpsc::TrySendError::Full(())) => Ok(()),
            Err(mpsc::TrySendError::Disconnected(())) => Err("recognition worker stopped".into()),
        }
    }
    fn call<U: Send + 'static>(&self, job: impl FnOnce(&mut T) -> R<U> + Send + 'static) -> R<U> {
        let (tx, rx) = mpsc::channel();
        self.submit(move |state| {
            let _ = tx.send(job(state));
        })?;
        rx.recv()
            .map_err(|_| "recognition worker stopped before completing its task")?
    }
}

struct Worker<T> {
    port: Option<Port<T>>,
    thread: Option<JoinHandle<()>>,
}
impl<T: 'static> Worker<T> {
    fn new(name: &str, setup: impl FnOnce() -> R<T> + Send + 'static) -> R<Self> {
        let (wake, work) = mpsc::sync_channel(1);
        let (ready, started) = mpsc::channel();
        let pending = Arc::new(Mutex::new(None::<Task<T>>));
        let slot = pending.clone();
        let thread = thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let mut state = match setup() {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready.send(Err(e));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                while work.recv().is_ok() {
                    let task = slot.lock().unwrap().take();
                    if let Some(task) = task {
                        task(&mut state);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let worker = Self {
            port: Some(Port { pending, wake }),
            thread: Some(thread),
        };
        started
            .recv()
            .map_err(|_| "recognition worker stopped during setup")??;
        Ok(worker)
    }
    fn port(&self) -> &Port<T> {
        self.port.as_ref().unwrap()
    }
}
impl<T> Drop for Worker<T> {
    fn drop(&mut self) {
        self.port.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

type VadStats = Arc<Mutex<(u64, u64, f64)>>;
struct Job {
    audio: Vec<f32>,
    context: String,
    language: Option<String>,
    range: Range<usize>,
    metrics: Offline,
    cancelled: Arc<AtomicBool>,
    reply: Sender<R<Offline>>,
}
impl Job {
    fn prepare(&mut self, vad: &mut FireRedVad, stats: &VadStats) -> R<()> {
        let start = Instant::now();
        let regions = vad.speech_timestamps(&self.audio)?;
        self.metrics.vad_ms = elapsed(start);
        self.metrics.vad_segments = regions.len();
        self.range = regions
            .first()
            .zip(regions.last())
            .map_or(0..0, |(first, last)| first.0..last.1);
        self.metrics.speech_samples = self.range.len();
        let mut totals = stats.lock().map_err(|_| "VAD stats poisoned")?;
        totals.0 += 1;
        totals.1 += u64::from(self.range.is_empty());
        totals.2 += self.metrics.vad_ms;
        Ok(())
    }

    fn decode(mut self, state: &mut QwenModel, stats: &VadStats) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let result = (|| {
            if let Some(vad) = state.cpu_vad.as_mut() {
                self.prepare(vad, stats)?;
            }
            if self.range.is_empty() {
                self.metrics.output.language = self.language.take();
            } else {
                let start = Instant::now();
                self.metrics.output = state
                    .native
                    .as_mut()
                    .ok_or("Qwen is suspended")?
                    .transcribe(
                        &self.audio[self.range.clone()],
                        &self.context,
                        self.language.as_deref(),
                    )?;
                self.metrics.inference_ms = elapsed(start);
            }
            Ok(std::mem::take(&mut self.metrics))
        })();
        let _ = self.reply.send(result);
    }
}

struct QwenModel {
    native: Option<NativeAsr>,
    cpu_vad: Option<FireRedVad>,
}
struct Ticket {
    count: usize,
    result: Receiver<R<Offline>>,
    cancelled: Arc<AtomicBool>,
}
#[derive(Default)]
struct Capture {
    pcm: Vec<f32>,
    context: String,
    forced: Option<String>,
    language: Option<String>,
    preview: String,
    published: Option<usize>,
    last_submit: usize,
    had_text: bool,
    jobs: VecDeque<Ticket>,
}
const PREVIEW_WINDOW: usize = 192_000;
impl Capture {
    fn collect(&mut self, wait: bool) -> R<()> {
        while let Some(job) = self.jobs.front() {
            let reply = if wait {
                job.result.recv().ok()
            } else {
                match job.result.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => None,
                }
            };
            if let Some(result) = reply {
                let result = result?.output;
                self.preview = result.text;
                self.had_text |= !self.preview.is_empty();
                self.published = Some(job.count);
                if result.language.is_some() {
                    self.language = result.language;
                }
            }
            self.jobs.pop_front();
        }
        Ok(())
    }
    fn discard(&mut self, keep: Option<usize>) {
        for job in &self.jobs {
            if Some(job.count) != keep {
                job.cancelled.store(true, Ordering::Release);
            }
        }
    }
}

struct Qwen {
    // Drop the preparer before the downstream decoder, releasing cloned ports.
    prepare: Option<Worker<FireRedVad>>,
    decoder: Worker<QwenModel>,
    stats: VadStats,
    stream: Option<Capture>,
}
impl Drop for Qwen {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}

fn open_qwen_vad(resources: &Path) -> R<FireRedVad> {
    let mut vad = FireRedVad::open(resources)?;
    if let Err(error) = vad.probabilities(&[0.; 16_000]) {
        eprintln!("FireRedVAD warmup: {error}");
    }
    Ok(vad)
}

impl Qwen {
    fn open(spec: &Spec) -> R<Self> {
        let gpu = spec.backend != Backend::Cpu;
        let resources = spec.resources.clone();
        let owned = spec.clone();
        let decoder = Worker::new("qwen3-asr-native", move || {
            let native = Some(owned.open()?);
            let cpu_vad = if gpu {
                None
            } else {
                Some(open_qwen_vad(&owned.resources)?)
            };
            Ok(QwenModel { native, cpu_vad })
        })?;
        let prepare = if gpu {
            Some(Worker::new("qwen3-asr-prepare", move || {
                open_qwen_vad(&resources)
            })?)
        } else {
            None
        };
        Ok(Self {
            prepare,
            decoder,
            stats: Default::default(),
            stream: None,
        })
    }

    fn submit(
        &self,
        mut audio: Vec<f32>,
        context: &str,
        language: Option<String>,
        count: usize,
        stream: bool,
    ) -> R<Ticket> {
        if stream && audio.len() < 8000 {
            audio.resize(8000, 0.);
        }
        let (tx, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut job = Job {
            range: 0..audio.len(),
            audio,
            context: context.into(),
            language,
            metrics: Offline::default(),
            cancelled: cancelled.clone(),
            reply: tx,
        };
        let stats = self.stats.clone();
        if let Some(prepare) = &self.prepare {
            let decoder = self.decoder.port().clone();
            prepare.port().submit(move |vad| {
                if job.cancelled.load(Ordering::Acquire) {
                    return;
                }
                if let Err(e) = job.prepare(vad, &stats) {
                    let _ = job.reply.send(Err(e));
                    return;
                }
                if !job.cancelled.load(Ordering::Acquire) {
                    let error = job.reply.clone();
                    if let Err(e) = decoder.submit(move |state| job.decode(state, &stats)) {
                        let _ = error.send(Err(e));
                    }
                }
            })?;
        } else {
            self.decoder
                .port()
                .submit(move |state| job.decode(state, &stats))?;
        }
        Ok(Ticket {
            count,
            result,
            cancelled,
        })
    }

    fn feed(&mut self, pcm: &[f32], preview: bool) -> R<Transcript> {
        let mut stream = self.stream.take().ok_or("No active Qwen stream")?;
        let result = (|| {
            stream.collect(false)?;
            stream.pcm.extend_from_slice(pcm);
            let count = stream.pcm.len();
            let threshold = if !stream.had_text {
                1920
            } else if count < 64_000 {
                3200
            } else {
                5120
            };
            if preview && count - stream.last_submit >= threshold {
                let audio = stream.pcm[count.saturating_sub(PREVIEW_WINDOW)..].to_vec();
                stream.jobs.push_back(self.submit(
                    audio,
                    &stream.context,
                    stream.forced.clone(),
                    count,
                    true,
                )?);
                stream.last_submit = count;
            }
            stream.collect(false)?;
            Ok(Transcript {
                preview: stream.preview.clone(),
                language: stream.language.clone(),
                ..Default::default()
            })
        })();
        self.stream = Some(stream);
        result
    }

    fn finish(&mut self) -> R<Transcript> {
        let mut stream = self.stream.take().ok_or("No active Qwen stream")?;
        let result = (|| {
            stream.collect(false)?;
            let count = stream.pcm.len();
            if count > 0 {
                let exact = count <= PREVIEW_WINDOW
                    && (stream.published == Some(count)
                        || stream.jobs.iter().any(|j| j.count == count));
                stream.discard(exact.then_some(count));
                if !exact {
                    let audio = std::mem::take(&mut stream.pcm);
                    stream.jobs.push_back(self.submit(
                        audio,
                        &stream.context,
                        stream.forced.clone(),
                        count,
                        true,
                    )?);
                }
                stream.collect(true)?;
            }
            Ok(Transcript {
                text: std::mem::take(&mut stream.preview),
                language: stream.language.take(),
                ..Default::default()
            })
        })();
        if result.is_err() {
            self.stream = Some(stream);
        }
        result
    }

    fn cancel(&mut self) -> R<()> {
        if let Some(mut stream) = self.stream.take() {
            stream.discard(None);
            if let Some(prepare) = &self.prepare {
                prepare.port().call(|_| Ok(()))?;
            }
            self.decoder.port().call(|_| Ok(()))?;
        }
        Ok(())
    }
}

enum Runtime {
    R2(Box<Segmented>),
    Qwen(Box<Qwen>),
}
pub struct Recognition {
    spec: Spec,
    runtime: Runtime,
}
impl Recognition {
    pub fn open(
        resources: &Path,
        model: &Path,
        family: Family,
        backend: Backend,
        threads: i32,
    ) -> R<Self> {
        let spec = Spec {
            resources: resources.into(),
            model: model.into(),
            family,
            backend,
            threads,
        };
        let runtime = match family {
            Family::Qwen => Runtime::Qwen(Box::new(Qwen::open(&spec)?)),
            Family::R2t2 => Runtime::R2(Box::new(Segmented::open(&spec)?)),
        };
        Ok(Self { spec, runtime })
    }

    pub fn start(&mut self, context: &str, lang: &str) -> R<()> {
        match &mut self.runtime {
            Runtime::R2(r) => r.start(context, lang),
            Runtime::Qwen(q) => {
                q.cancel()?;
                q.stream = Some(Capture {
                    context: context.into(),
                    forced: language(lang),
                    language: language(lang),
                    ..Default::default()
                });
                Ok(())
            }
        }
    }
    pub fn feed(&mut self, pcm: &[f32], preview: bool) -> R<Transcript> {
        if !pcm.iter().all(|s| s.is_finite()) {
            return Err("Expected finite PCM".into());
        }
        let result = match &mut self.runtime {
            Runtime::Qwen(q) => q.feed(pcm, preview),
            Runtime::R2(_) if !preview => {
                return Err("R2T2 does not support preview suppression".into());
            }
            Runtime::R2(r) => r.feed(pcm),
        };
        if result.is_err() {
            let _ = self.cancel();
        }
        result
    }
    pub fn finish(&mut self) -> R<Transcript> {
        let result = match &mut self.runtime {
            Runtime::Qwen(q) => q.finish(),
            Runtime::R2(r) => r.finish(),
        };
        if result.is_err() {
            let _ = self.cancel();
        }
        result
    }
    pub fn cancel(&mut self) -> R<()> {
        match &mut self.runtime {
            Runtime::Qwen(q) => q.cancel(),
            Runtime::R2(r) => r.cancel(),
        }
    }
    pub fn suspend(&mut self) -> R<()> {
        match &mut self.runtime {
            Runtime::R2(r) => r.suspend(),
            Runtime::Qwen(q) => {
                q.cancel()?;
                q.decoder.port().call(|s| {
                    s.native = None;
                    Ok(())
                })
            }
        }
    }
    pub fn resume(&mut self) -> R<()> {
        let spec = self.spec.clone();
        match &mut self.runtime {
            Runtime::R2(r) => r.resume(&spec),
            Runtime::Qwen(q) => q.decoder.port().call(move |s| {
                if s.native.is_none() {
                    s.native = Some(spec.open()?);
                }
                Ok(())
            }),
        }
    }
    pub fn vad_stats(&self) -> (u64, u64, f64) {
        match &self.runtime {
            Runtime::R2(_) => (0, 0, 0.),
            Runtime::Qwen(q) => *q.stats.lock().unwrap(),
        }
    }
    pub fn transcribe(&mut self, pcm: &[f32], context: &str, lang: &str) -> R<Offline> {
        match &mut self.runtime {
            Runtime::Qwen(q) => {
                if q.stream.is_some() {
                    return Err("stream is active".into());
                }
                if pcm.len() < 8000 {
                    return Ok(Offline::default());
                }
                let ticket = q.submit(pcm.to_vec(), context, language(lang), pcm.len(), false)?;
                let mut result = ticket.result.recv().map_err(|_| "Qwen worker stopped")??;
                result.output.text = result.output.text.trim().into();
                if result.vad_segments == 0 || result.output.language.is_none() {
                    result.output.language = Some("unknown".into());
                }
                Ok(result)
            }
            Runtime::R2(r) => {
                let start = Instant::now();
                r.start(context, lang)?;
                let result = r.feed(pcm).and_then(|_| r.finish());
                if result.is_err() {
                    let _ = r.cancel();
                }
                Ok(Offline {
                    output: result?,
                    inference_ms: elapsed(start),
                    ..Default::default()
                })
            }
        }
    }
}
