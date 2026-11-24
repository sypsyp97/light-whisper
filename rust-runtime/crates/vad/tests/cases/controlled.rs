use super::{pcm, resources};
use light_whisper_vad::FireRedVad;
use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct ModelDirectory(PathBuf);
impl Drop for ModelDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn model_directory(name: &str, model: &[u8]) -> ModelDirectory {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = env::temp_dir().join(format!(
        "light-whisper-vad-{name}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&path).unwrap();
    let directory = ModelDirectory(path);
    fs::write(directory.0.join("fireredvad_vad.onnx"), model).unwrap();
    fs::copy(
        resources().join("fireredvad_cmvn.json"),
        directory.0.join("fireredvad_cmvn.json"),
    )
    .unwrap();
    directory
}

fn with_graph(name: &str, model: &[u8], check: impl FnOnce(&mut FireRedVad)) {
    let directory = model_directory(name, model);
    let mut vad = FireRedVad::open(&directory.0).unwrap();
    check(&mut vad);
}

#[test]
fn ineligible_window_skips_inference_and_equality_is_speech() {
    // This ONNX model requires exactly 15 frames and emits 0.5 for each frame.
    // Calling it with 14 frames fails, so returning [] exercises the early exit.
    with_graph(
        "fixed15",
        include_bytes!("../fixtures/first_window.onnx"),
        |vad| {
            let audio = [0.; 2640];
            assert_eq!(vad.speech_timestamps(&audio[..2639]).unwrap(), []);
            assert_eq!(vad.speech_timestamps(&audio).unwrap(), [(0, 2640)]);
            assert!(vad.probabilities(&audio[..2639]).is_err());
        },
    );
}

#[test]
fn candidate_preroll_and_consecutive_silence_are_measured_from_their_first_frame() {
    // Raw probabilities alternate 0 and 1 across runs of 20,14,20,15,29,15,30,15,20 frames.
    // The 14-frame burst is rejected, 29 silent frames bridge, 30 close at the first quiet frame.
    // The EOF segment retains its unconfirmed final silence and clamps to the original sample count.
    with_graph(
        "gaps",
        include_bytes!("../fixtures/postprocess.onnx"),
        |vad| {
            let audio = vec![0.; 28720];
            assert_eq!(
                vad.speech_timestamps(&audio).unwrap(),
                [(7040, 20320), (21280, 28720)]
            );
        },
    );
}

#[test]
fn all_eighty_cmvn_bins_match_kaldi_on_real_audio() {
    // The graph returns sigmoid(feature[frame, frame]) for 80 consecutive real frames.
    // Diagonal indexing observes each CMVN bin with the normal model input shape.
    with_graph(
        "bins",
        include_bytes!("../fixtures/cmvn_bins.onnx"),
        |vad| {
            let audio = pcm(&resources().join("model_connection.wav"));
            let actual = vad.probabilities(&audio[1600..14640]).unwrap();
            let expected = include_bytes!("../fixtures/cmvn_bins.f32le");
            assert_eq!(actual.len(), 80);
            for (bin, (value, bytes)) in actual.iter().zip(expected.chunks_exact(4)).enumerate() {
                let feature = f32::from_le_bytes(bytes.try_into().unwrap());
                let probability = 1. / (1. + (-feature).exp());
                assert!(
                    (value - probability).abs() <= 1e-4,
                    "feature bin {bin}: {value} versus sigmoid({feature}) = {probability}"
                );
            }
        },
    );
}

#[test]
fn each_cmvn_vector_must_independently_contain_eighty_bins() {
    let folder = model_directory("cmvn", include_bytes!("../fixtures/first_window.onnx"));
    let file = folder.0.join("fireredvad_cmvn.json");
    for (means, scales) in [(79, 80), (80, 81)] {
        let cmvn = serde_json::json!({"mean": vec![0.;means], "inverse_std": vec![1.;scales]});
        fs::write(&file, cmvn.to_string()).unwrap();
        assert!(
            FireRedVad::open(&folder.0).is_err(),
            "accepted dimensions {means}/{scales}"
        );
    }
}

#[test]
fn open_rejects_missing_or_unreadable_model_data() {
    let folder = model_directory(
        "bad_assets",
        include_bytes!("../fixtures/first_window.onnx"),
    );
    let cmvn = folder.0.join("fireredvad_cmvn.json");
    let model = folder.0.join("fireredvad_vad.onnx");
    let original = fs::read(&cmvn).unwrap();
    fs::remove_file(&cmvn).unwrap();
    assert!(FireRedVad::open(&folder.0).is_err());
    fs::write(&cmvn, b"{").unwrap();
    assert!(FireRedVad::open(&folder.0).is_err());
    fs::write(&cmvn, original).unwrap();
    fs::remove_file(&model).unwrap();
    assert!(FireRedVad::open(&folder.0).is_err());
    fs::write(&model, b"invalid ONNX model").unwrap();
    assert!(FireRedVad::open(&folder.0).is_err());
}
