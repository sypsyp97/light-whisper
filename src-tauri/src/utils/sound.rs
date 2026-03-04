use std::sync::OnceLock;

use crate::services::audio_service;

const SAMPLE_RATE: u32 = 22050;
const AMPLITUDE: f32 = 0.25;
const SWEEP_RANGE: f32 = 0.5;

static START_WAV: OnceLock<Vec<u8>> = OnceLock::new();
static STOP_WAV: OnceLock<Vec<u8>> = OnceLock::new();

fn generate_tone(base_freq: f32, duration_ms: u32, ascending: bool) -> Vec<u8> {
    let num_samples = (SAMPLE_RATE as f32 * duration_ms as f32 / 1000.0) as usize;
    let samples: Vec<i16> = (0..num_samples)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let progress = i as f32 / num_samples as f32;

            let freq = if ascending {
                base_freq * (1.0 + progress * SWEEP_RANGE)
            } else {
                base_freq * (1.0 + SWEEP_RANGE - progress * SWEEP_RANGE)
            };

            let envelope = (progress * std::f32::consts::PI).sin();
            (envelope * AMPLITUDE * (2.0 * std::f32::consts::PI * freq * t).sin() * 32767.0) as i16
        })
        .collect();

    audio_service::encode_wav(&samples, SAMPLE_RATE)
}

#[cfg(target_os = "windows")]
pub fn play_start_sound() {
    let wav = START_WAV.get_or_init(|| generate_tone(880.0, 100, true));
    play_wav_async(wav);
}

#[cfg(target_os = "windows")]
pub fn play_stop_sound() {
    let wav = STOP_WAV.get_or_init(|| generate_tone(660.0, 100, false));
    play_wav_async(wav);
}

#[cfg(target_os = "windows")]
fn play_wav_async(wav: &[u8]) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY};

    unsafe {
        PlaySoundW(wav.as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn play_start_sound() {}

#[cfg(not(target_os = "windows"))]
pub fn play_stop_sound() {}
