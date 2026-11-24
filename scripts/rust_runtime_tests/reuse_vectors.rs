use light_whisper_native_asr::Backend;
use light_whisper_reuse::RealtimeEnhancer;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn samples(file: &Path) -> Vec<f32> {
    let data = fs::read(file).unwrap();
    assert_eq!(data.len() % 4, 0);
    data.chunks_exact(4)
        .map(|value| f32::from_le_bytes(value.try_into().unwrap()))
        .collect()
}

#[test]
fn installed_reference_vectors() {
    let get = |key| PathBuf::from(env::var_os(key).unwrap());
    let fixtures = get("LW_REUSE_TEST_VECTORS");
    let mut enhancer = RealtimeEnhancer::open(
        &get("LW_REUSE_TEST_RESOURCES"),
        &get("LW_REUSE_TEST_MODEL"),
        Backend::Cpu,
        2,
    )
    .expect("open real-time RE-USE with authentic CPU weights and DLLs");
    let trace = fs::read_to_string(fixtures.join("trace.txt")).unwrap();
    for (i, line) in trace.lines().enumerate() {
        let step: Vec<_> = line.split_whitespace().collect();
        let input = || samples(&fixtures.join(step[1]));
        let actual = match step[0] {
            "enhance" => enhancer.enhance(&input()),
            "feed" => enhancer.feed(&input()),
            "flush" => enhancer.flush(),
            "reset" => {
                enhancer.reset().expect("reset");
                continue;
            }
            other => panic!("unknown oracle operation {other}"),
        }
        .unwrap_or_else(|err| panic!("{i} {}: {err}", step[0]));
        let expected = samples(&fixtures.join(step[2]));
        assert_eq!(
            actual.len(),
            expected.len(),
            "step {i} {} emitted samples",
            step[0]
        );
        let mut squared = 0.0;
        for (n, (&a, &b)) in actual.iter().zip(&expected).enumerate() {
            let error = (a - b).abs();
            assert!(
                a.is_finite() && error <= 0.0001,
                "step {i} {} sample {n}: expected {b}, got {a}",
                step[0]
            );
            squared += f64::from(error).powi(2);
        }
        assert!(
            (squared / expected.len().max(1) as f64).sqrt() <= 0.00002,
            "step {i} {} waveform RMS error",
            step[0]
        );
    }
}
