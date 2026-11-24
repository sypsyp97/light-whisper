#![cfg(windows)]
use light_whisper_native_asr::{Backend, Family, NativeAsr};
use std::{env, fs, path::PathBuf};

fn path(key: &str) -> PathBuf {
    env::var_os(key)
        .unwrap_or_else(|| panic!("set {key} to run the native oracle"))
        .into()
}

fn audio() -> Vec<f32> {
    let bytes = fs::read(path("LW_NATIVE_ORACLE").join("input.f32le")).unwrap();
    assert_eq!(bytes.len() % 4, 0);
    bytes
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect()
}

fn expected(name: &str) -> Vec<String> {
    let value = fs::read_to_string(path("LW_NATIVE_ORACLE").join(name)).unwrap();
    assert!(value.ends_with('\0'));
    value.split_terminator('\0').map(str::to_owned).collect()
}

#[test]
#[ignore = "requires Python oracle corpus and both matching native assets"]
fn qwen_cpu_matches_python_offline_text_and_detected_language() {
    let mut native = NativeAsr::open(
        &path("LW_NATIVE_LIBRARY"),
        &path("LW_QWEN_MODEL"),
        Family::Qwen,
        Backend::Cpu,
        4,
    )
    .expect("open the same CPU Qwen library/model pair accepted by Python");
    let result = native.transcribe(&audio(), "", None).unwrap();
    assert_eq!(
        vec![result.text, result.language.unwrap()],
        expected("qwen.expected")
    );
}

#[test]
#[ignore = "requires Python oracle corpus and both matching native assets"]
fn r2t2_cpu_matches_every_python_packet_and_final_tail() {
    let mut native = NativeAsr::open(
        &path("LW_NATIVE_LIBRARY"),
        &path("LW_R2T2_MODEL"),
        Family::R2t2,
        Backend::Cpu,
        4,
    )
    .expect("open the same CPU R2T2 library/model pair accepted by Python");
    native.start("", None).unwrap();
    let mut actual = Vec::new();
    for packet in audio().chunks(5120) {
        let result = native.feed(packet).unwrap();
        actual.extend([
            result.text,
            result.preview,
            result.language.unwrap_or_default(),
        ]);
    }
    let result = native.finish().unwrap();
    actual.extend([result.text, result.language.unwrap_or_default()]);
    assert_eq!(actual, expected("r2t2.expected"));
}

#[test]
#[ignore = "requires Python oracle corpus and both matching native assets"]
fn cpu_models_release_and_reopen_after_inference() {
    for _ in 0..2 {
        qwen_cpu_matches_python_offline_text_and_detected_language();
        r2t2_cpu_matches_every_python_packet_and_final_tail();
    }
}
