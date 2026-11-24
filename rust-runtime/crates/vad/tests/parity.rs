use light_whisper_vad::FireRedVad;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn resources() -> PathBuf {
    env::var_os("LW_VAD_RESOURCES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src-tauri/resources")
        })
}

// The oracle and model consume PCM16 / 32768. Preserve all samples including partial hops.
fn pcm(path: &Path) -> Vec<f32> {
    let wav = fs::read(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    let mut cursor = 12;
    let mut format_ok = false;
    while cursor + 8 <= wav.len() {
        let tag = &wav[cursor..cursor + 4];
        let len = u32::from_le_bytes(wav[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let data = &wav[cursor + 8..cursor + 8 + len];
        if tag == b"fmt " {
            assert_eq!(&data[..8], &[1, 0, 1, 0, 128, 62, 0, 0]);
            assert_eq!(&data[14..16], &[16, 0]);
            format_ok = true;
        } else if tag == b"data" {
            assert!(format_ok && data.len().is_multiple_of(2));
            return data
                .chunks_exact(2)
                .map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.)
                .collect();
        }
        cursor += 8 + len + (len % 2);
    }
    panic!("{} lacks a data chunk", path.display());
}

#[test]
fn first_eligible_speech_window_has_fifteen_complete_frames() {
    let mut vad = FireRedVad::open(&resources()).unwrap();
    let audio = pcm(&resources().join("model_connection.wav"));
    let voiced = &audio[1600..4240];
    for length in [0, 2560, 2639] {
        assert_eq!(vad.speech_timestamps(&voiced[..length]).unwrap(), []);
    }
    assert_eq!(vad.speech_timestamps(voiced).unwrap(), [(0, 2640)]);
    assert_eq!(vad.probabilities(&voiced[..2639]).unwrap().len(), 14);
    assert_eq!(vad.probabilities(voiced).unwrap().len(), 15);
    assert!(vad.probabilities(&voiced[..399]).unwrap().is_empty());
    assert_eq!(vad.probabilities(&voiced[..400]).unwrap().len(), 1);
}

#[test]
fn bundled_clip_matches_every_python_posterior_and_region() {
    let mut vad = FireRedVad::open(&resources()).unwrap();
    let audio = pcm(&resources().join("model_connection.wav"));
    let expected = include_bytes!("fixtures/model_connection.prob.f32le");
    let got = vad.probabilities(&audio).unwrap();
    assert_eq!(got.len() * 4, expected.len());
    for (frame, (value, bytes)) in got.iter().zip(expected.chunks_exact(4)).enumerate() {
        let reference = f32::from_le_bytes(bytes.try_into().unwrap());
        assert!(
            (value - reference).abs() <= 5e-4,
            "frame {frame}: {value} vs {reference}"
        );
    }
    assert_eq!(
        vad.speech_timestamps(&audio).unwrap(),
        [(160, 10560), (19840, 51040)]
    );
    assert_eq!(vad.speech_timestamps(&vec![0.; 16000]).unwrap(), []);
}

#[test]
#[ignore = "uses the external six-fixture natural-quality benchmark, set LW_QUALITY_ROOT to its repository"]
fn six_real_clips_preserve_exact_sample_boundaries() {
    let root = PathBuf::from(env::var_os("LW_QUALITY_ROOT").expect("set LW_QUALITY_ROOT"));
    let mut vad = FireRedVad::open(&resources()).unwrap();
    type Fixture<'a> = (&'a str, usize, &'a [(usize, usize)]);
    let cases: &[Fixture] = &[
        (
            "tmp/asr-opt-20261025/natural-quality/ascend-test-00009.wav",
            64480,
            &[(0, 64480)],
        ),
        (
            "tmp/asr-opt-20261025/natural-quality/ascend-test-00066.wav",
            25439,
            &[(0, 25439)],
        ),
        (
            "tmp/asr-opt-20261025/natural-quality/ascend-test-00068.wav",
            23200,
            &[(1440, 23200)],
        ),
        (
            "tmp/asr-opt-20261025/natural-quality/ascend-test-00081.wav",
            29440,
            &[(320, 29440)],
        ),
        (
            "tmp/asr-engine-20261017/upstream/samples/fleurs-en.wav",
            168960,
            &[(15200, 24480), (31200, 55520), (69600, 140800)],
        ),
        (
            "tmp/asr-caption-20261017/fixtures/asr_zh.wav",
            67263,
            &[(5600, 60960)],
        ),
    ];
    for (path, samples, expected) in cases {
        let audio = pcm(&root.join(path));
        assert_eq!(audio.len(), *samples, "{path}");
        assert_eq!(vad.speech_timestamps(&audio).unwrap(), *expected, "{path}");
    }
}

#[path = "cases/controlled.rs"]
mod controlled;
