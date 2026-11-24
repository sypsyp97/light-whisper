mod fft;
mod native;
use light_whisper_native_asr::Backend;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, BufReader},
    path::Path,
};
const FFT: usize = 640;
const HOP: usize = 320;
const BINS: usize = 321;
pub const MODEL_BYTES: u64 = 54_055_764;
pub const MODEL_SHA: &str = "6eac4e8cc721f99a22cde7eac3c9f8e2826ced5dc8a4234fd1977de1ec0d3ade";
pub fn valid_model(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    if file.metadata().map(|m| m.len()).ok() != Some(MODEL_BYTES) {
        return false;
    }
    let mut hash = Sha256::new();
    io::copy(&mut BufReader::with_capacity(1024 * 1024, file), &mut hash).is_ok()
        && format!("{:x}", hash.finalize()) == MODEL_SHA
}
#[derive(Default)]
struct Stream {
    pending: Vec<f32>,
    previous: f32,
    left_padded: bool,
    started: bool,
    produced: bool,
    samples: usize,
    emitted: usize,
    first: Option<(Vec<f32>, Vec<f32>)>,
    failed: Option<String>,
}
pub struct RealtimeEnhancer {
    native: native::Native,
    plan: fft::RealFft,
    window: [f32; FFT],
    overlap: [f64; FFT],
    weights: [f64; FFT],
    stream: Stream,
    fft: [f64; FFT],
}
impl RealtimeEnhancer {
    pub fn open(
        resources: &Path,
        model: &Path,
        backend: Backend,
        threads: i32,
    ) -> Result<Self, String> {
        if !valid_model(model) {
            return Err("Real-time RE-USE model version or checksum mismatch".into());
        }
        let native = native::Native::open(resources, model, backend, threads)?;
        let plan = fft::RealFft::new()?;
        let window = std::array::from_fn(|i| {
            ((1. - (std::f64::consts::TAU * i as f64 / FFT as f64).cos()) * 0.5) as f32
        });
        Ok(Self {
            native,
            plan,
            window,
            overlap: [0.; FFT],
            weights: [0.; FFT],
            stream: Stream::default(),
            fft: [0.; FFT],
        })
    }
    pub fn reset(&mut self) -> Result<(), String> {
        if self.stream.started {
            self.native.reset()?
        }
        self.stream = Stream::default();
        self.overlap.fill(0.);
        self.weights.fill(0.);
        Ok(())
    }
    fn spectrum(
        &mut self,
        mut mag: Vec<f32>,
        mut phase: Vec<f32>,
        out: &mut Vec<f32>,
    ) -> Result<(), String> {
        if !self.stream.started {
            let Some((mut m, mut p)) = self.stream.first.take() else {
                self.stream.first = Some((mag, phase));
                return Ok(());
            };
            m.append(&mut mag);
            p.append(&mut phase);
            mag = m;
            phase = p;
            self.stream.started = true;
        }
        let [mut magnitude, real, imag] = self
            .native
            .run(&mag, &phase)
            .inspect_err(|e| self.stream.failed = Some(e.clone()))?;
        for m in &mut magnitude {
            *m = m.max(0.).exp_m1()
        }
        if magnitude.iter().filter(|m| **m == 0.).count() > BINS / 2 {
            magnitude.fill(0.)
        }
        for i in 0..BINS {
            let (sin, cos) = imag[i].atan2(real[i]).sin_cos();
            self.fft[(2 * i).saturating_sub(1)] = (magnitude[i] * cos) as f64;
            if i > 0 && i < HOP {
                self.fft[2 * i] = (magnitude[i] * sin) as f64;
            }
        }
        self.plan.inverse(&mut self.fft)?;
        for i in 0..FFT {
            self.overlap[i] += self.fft[i] * self.window[i] as f64;
            self.weights[i] += (self.window[i] * self.window[i]) as f64;
        }
        if self.stream.produced {
            let count = HOP.min(self.stream.samples - self.stream.emitted);
            out.extend((0..count).map(|i| (self.overlap[i] / self.weights[i]) as f32));
            self.stream.emitted += count;
        }
        self.stream.produced = true;
        for v in [&mut self.overlap, &mut self.weights] {
            v.copy_within(HOP.., 0);
            v[HOP..].fill(0.);
        }
        Ok(())
    }
    fn frame(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        for (i, s) in self.fft.iter_mut().enumerate() {
            *s = (self.stream.pending[i] * self.window[i]) as f64
        }
        self.plan.forward(&mut self.fft)?;
        let mut mag = Vec::with_capacity(BINS);
        let mut phase = Vec::with_capacity(BINS);
        for i in 0..BINS {
            let x = self.fft[(2 * i).saturating_sub(1)] as f32;
            let y = if i > 0 && i < HOP {
                self.fft[2 * i] as f32
            } else {
                0.
            };
            mag.push(x.hypot(y).ln_1p());
            phase.push(y.atan2(x));
        }
        self.spectrum(mag, phase, out)
    }
    fn append(&mut self, mut pcm: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        while !pcm.is_empty() {
            let len = (FFT - self.stream.pending.len()).min(pcm.len());
            self.stream.pending.extend_from_slice(&pcm[..len]);
            pcm = &pcm[len..];
            if self.stream.pending.len() < FFT {
                continue;
            }
            if !self.stream.left_padded {
                let mut padded = Vec::with_capacity(FFT + HOP);
                padded.extend(self.stream.pending[1..=HOP].iter().rev().copied());
                padded.append(&mut self.stream.pending);
                self.stream.pending = padded;
                self.stream.left_padded = true;
            }
            while self.stream.pending.len() >= FFT {
                self.frame(out)?;
                self.stream.previous = self.stream.pending[HOP - 1];
                self.stream.pending.drain(..HOP);
            }
        }
        Ok(())
    }
    pub fn feed(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String> {
        if let Some(e) = &self.stream.failed {
            return Err(e.clone());
        }
        if pcm.iter().any(|x| !x.is_finite()) {
            return Err("nonfinite audio".into());
        }
        self.stream.samples = self
            .stream
            .samples
            .checked_add(pcm.len())
            .ok_or("audio length overflow")?;
        let mut out = Vec::with_capacity(pcm.len());
        self.append(pcm, &mut out)?;
        Ok(out)
    }
    fn tail(&mut self) -> Result<Vec<f32>, String> {
        let pad = self.stream.samples.next_multiple_of(HOP).max(FFT) - self.stream.samples;
        let mut out = Vec::new();
        self.append(&vec![1e-8; pad], &mut out)?;
        let p = &mut self.stream.pending;
        p.extend_from_within(..HOP - 1);
        p[HOP..].reverse();
        p.push(self.stream.previous);
        self.frame(&mut out)?;
        self.spectrum(vec![0.; BINS], vec![0.; BINS], &mut out)?;
        Ok(out)
    }
    pub fn flush(&mut self) -> Result<Vec<f32>, String> {
        if self.stream.samples == 0 {
            return Ok(vec![]);
        }
        let result = match &self.stream.failed {
            Some(e) => Err(e.clone()),
            None => self.tail(),
        };
        self.reset()?;
        result
    }
    pub fn enhance(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String> {
        self.reset()?;
        let result = self.feed(pcm).and_then(|mut out| {
            out.extend(self.flush()?);
            Ok(out)
        });
        self.reset()?;
        result
    }
}
