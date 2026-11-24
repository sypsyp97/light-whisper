//! FireRedVAD for mono 16 kHz floating-point samples. Native build dependencies
//! are documented in build.rs. No Python interpreter is needed at runtime.
// Adapted from FireRedTeam/FireRedVAD (Apache-2.0), see THIRD_PARTY_NOTICES.md.

mod native;

use native::{Fbank, Model};
use serde::Deserialize;
use std::{fs, path::Path};

const RATE: usize = 16_000;
const FRAME: usize = 400;
const SHIFT: usize = 160;
const BINS: usize = 80;
const MIN_SPEECH: usize = 15;
const MIN_SILENCE: usize = 30;
const PAD: usize = 1_920;
const SMOOTH: usize = 5;

#[derive(Deserialize)]
struct Cmvn {
    mean: Vec<f32>,
    inverse_std: Vec<f32>,
}

struct FeatureCache {
    pcm: Vec<f32>,
    bank: Fbank,
    features: Vec<f32>,
}

pub struct FireRedVad {
    model: Model,
    cmvn: Cmvn,
    cache: Option<FeatureCache>,
}

impl FireRedVad {
    pub fn open(resources: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            let path = resources.join(name);
            fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
        };
        let cmvn: Cmvn = serde_json::from_slice(&read("fireredvad_cmvn.json")?)
            .map_err(|e| format!("invalid FireRedVAD CMVN: {e}"))?;
        if cmvn.mean.len() != BINS || cmvn.inverse_std.len() != BINS {
            return Err("FireRedVAD CMVN requires 80 mean and inverse_std values".into());
        }
        let model = Model::new(&read("fireredvad_vad.onnx")?)?;
        Ok(Self {
            model,
            cmvn,
            cache: None,
        })
    }

    pub fn probabilities(&mut self, audio: &[f32]) -> Result<Vec<f32>, String> {
        let previous = self.cache.take();
        let cacheable = audio.len() <= 12 * RATE;
        let pcm: Vec<_> = audio
            .iter()
            .map(|v| (v * 32768.).clamp(-32768., 32767.))
            .collect();
        let mut state = match previous.filter(|s| cacheable && pcm.starts_with(&s.pcm)) {
            Some(state) => state,
            None => FeatureCache {
                pcm: Vec::new(),
                bank: Fbank::new()?,
                features: Vec::new(),
            },
        };
        let suffix = &pcm[state.pcm.len()..];
        if !suffix.is_empty() {
            let frames = pcm.len().checked_sub(FRAME).map_or(0, |n| 1 + n / SHIFT);
            let old = state.features.len();
            state.features.resize(frames * BINS, 0.);
            let added = &mut state.features[old..];
            state.bank.append(suffix, added)?;
            for frame in added.chunks_exact_mut(BINS) {
                for (i, bin) in frame.iter_mut().enumerate() {
                    *bin = (*bin - self.cmvn.mean[i]) * self.cmvn.inverse_std[i];
                }
            }
        }
        // Feature extraction succeeded. A subsequent inference error does not
        // invalidate the already computed, normalized prefix.
        let features = if cacheable {
            state.pcm = pcm;
            &mut self.cache.insert(state).features
        } else {
            &mut state.features
        };
        if features.is_empty() {
            Ok(Vec::new())
        } else {
            self.model.run(features)
        }
    }

    pub fn speech_timestamps(&mut self, audio: &[f32]) -> Result<Vec<(usize, usize)>, String> {
        if audio.len() < FRAME + (MIN_SPEECH - 1) * SHIFT {
            self.cache = None;
            return Ok(Vec::new());
        }
        let probabilities = self.probabilities(audio)?;
        Ok(timestamps(&probabilities, audio.len()))
    }
}

fn timestamps(probabilities: &[f32], samples: usize) -> Vec<(usize, usize)> {
    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut add_region = |start: usize, end: usize| {
        let start = (start * SHIFT).saturating_sub(PAD);
        let end = (end * SHIFT + PAD).min(samples);
        if start < end {
            if let Some(last) = regions.last_mut().filter(|last| start <= last.1) {
                last.1 = last.1.max(end);
            } else {
                regions.push((start, end));
            }
        }
    };
    let mut candidate = None;
    let mut speech = None;
    let mut silence = None;
    for i in 0..probabilities.len() {
        let window = &probabilities[(i + 1).saturating_sub(SMOOTH)..=i];
        let average = if window.len() < SMOOTH {
            window.iter().sum::<f32>() / window.len() as f32
        } else {
            window.iter().map(|p| p * (1. / SMOOTH as f32)).sum::<f32>()
        };
        let voiced = average >= 0.5;
        if let Some(start) = speech {
            if voiced {
                silence = None;
            } else {
                let quiet = *silence.get_or_insert(i);
                if i + 1 - quiet >= MIN_SILENCE {
                    add_region(start, quiet);
                    speech = None;
                    silence = None;
                    candidate = None;
                }
            }
        } else if voiced {
            let start = *candidate.get_or_insert(i);
            if i + 1 - start >= MIN_SPEECH {
                speech = Some(start);
            }
        } else {
            candidate = None;
        }
    }
    if let Some(start) = speech {
        add_region(start, probabilities.len());
    }
    regions
}
