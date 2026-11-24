//! Replace the native symbols in this test executable. The production Rust
//! owner, prefix check, smoothing and Drop run normally. The parity executable
//! independently links the real Kaldi and ORT adapter.
use light_whisper_vad::FireRedVad;
use std::{
    cell::RefCell,
    env,
    ffi::{c_char, c_void},
    path::{Path, PathBuf},
    ptr,
};

#[derive(Default)]
struct Calls {
    model_opens: usize,
    model_closes: usize,
    bank_opens: usize,
    bank_closes: usize,
    accepted_samples: Vec<usize>,
    produced_frames: Vec<usize>,
    inferences: usize,
    posteriors: Vec<f32>,
}
thread_local! { static CALLS: RefCell<Calls> = RefCell::default(); }

#[unsafe(no_mangle)]
unsafe extern "C" fn lw_vad_open(_: *const u8, _: usize, out: *mut *mut c_void) -> *const c_char {
    CALLS.with_borrow_mut(|c| c.model_opens += 1);
    unsafe { *out = ptr::dangling_mut::<u8>().cast() };
    ptr::null()
}
#[unsafe(no_mangle)]
extern "C" fn lw_vad_close(_: *mut c_void) {
    CALLS.with_borrow_mut(|c| c.model_closes += 1);
}
#[unsafe(no_mangle)]
unsafe extern "C" fn lw_vad_run(
    _: *mut c_void,
    _: *mut f32,
    _: usize,
    receive: unsafe extern "C" fn(*mut c_void, *const f32, usize),
    dest: *mut c_void,
) -> *const c_char {
    CALLS.with_borrow_mut(|c| {
        c.inferences += 1;
        unsafe { receive(dest, c.posteriors.as_ptr(), c.posteriors.len()) };
    });
    ptr::null()
}
#[unsafe(no_mangle)]
unsafe extern "C" fn lw_fbank_open(out: *mut *mut c_void) -> *const c_char {
    CALLS.with_borrow_mut(|c| c.bank_opens += 1);
    unsafe { *out = ptr::dangling_mut::<u8>().cast() };
    ptr::null()
}
#[unsafe(no_mangle)]
extern "C" fn lw_fbank_close(_: *mut c_void) {
    CALLS.with_borrow_mut(|c| c.bank_closes += 1);
}
#[unsafe(no_mangle)]
extern "C" fn lw_fbank_append(
    _: *mut c_void,
    _: *const f32,
    samples: i32,
    _: *mut f32,
    frames: usize,
) -> *const c_char {
    CALLS.with_borrow_mut(|c| {
        c.accepted_samples.push(samples as usize);
        c.produced_frames.push(frames);
    });
    ptr::null()
}

fn detector() -> FireRedVad {
    CALLS.with_borrow_mut(|calls| *calls = Calls::default());
    let assets = env::var_os("LW_VAD_RESOURCES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src-tauri/resources")
        });
    FireRedVad::open(&assets).unwrap()
}

#[test]
fn feature_cache_limit_and_short_replacement_release_exactly_their_own_handles() {
    let mut vad = detector();
    // Verify one-hop growth before exercising long windows; a broken frame
    // count must fail here instead of attempting to allocate minutes of features.
    vad.probabilities(&[0.; 560]).unwrap();
    CALLS.with_borrow(|c| assert_eq!(c.produced_frames, [2]));
    drop(vad);
    let mut vad = detector();
    let mut audio = vec![0.; 192001];
    // A growing prefix is reused through the exact 12-second boundary.
    // Repeated longer windows are computed afresh, with no retained filterbank.
    for size in [191840, 192000, 192000, 192001, 192001] {
        vad.probabilities(&audio[..size]).unwrap();
    }
    vad.probabilities(&audio[..2640]).unwrap();
    audio[0] = 0.5;
    vad.probabilities(&audio[..2640]).unwrap();
    vad.speech_timestamps(&audio[..2639]).unwrap();
    CALLS.with_borrow(|c| {
        assert_eq!(
            c.accepted_samples,
            [191840, 160, 192001, 192001, 2640, 2640]
        );
        assert_eq!(c.produced_frames, [1197, 1, 1198, 1198, 15, 15]);
        assert_eq!(c.inferences, 7);
        assert_eq!((c.bank_opens, c.bank_closes), (5, 5));
        assert_eq!((c.model_opens, c.model_closes), (1, 0));
    });
    vad.probabilities(&audio[..2640]).unwrap();
    drop(vad);
    CALLS.with_borrow(|c| {
        assert_eq!((c.bank_opens, c.bank_closes), (6, 6));
        assert_eq!((c.model_opens, c.model_closes), (1, 1));
    });
}

#[test]
fn incomplete_smoothing_windows_use_their_own_frame_count() {
    let mut vad = detector();
    let mut probabilities = vec![0.6; 15];
    probabilities[1] = 0.2;
    CALLS.with_borrow_mut(|c| c.posteriors = probabilities);
    // The first four running averages are 0.6, 0.4, 0.466..., 0.5.
    // Losing even one frame prevents this fifteen-frame clip from qualifying.
    assert!(vad.speech_timestamps(&[0.; 2640]).unwrap().is_empty());
}

#[test]
fn five_frame_convolution_retains_float32_threshold_rounding() {
    let mut vad = detector();
    let first = [1056977166, 1056941531, 1056974086, 1056962853, 1056945368];
    let mut probabilities: Vec<f32> = first.map(f32::from_bits).into();
    probabilities.extend([1.; 10]);
    CALLS.with_borrow_mut(|c| c.posteriors = probabilities);
    // The Python reference's weighted sum is 0.49999997 for the fifth frame.
    // Averaging the five raw values after adding them rounds to exactly 0.5.
    assert!(vad.speech_timestamps(&[0.; 2640]).unwrap().is_empty());
}

#[test]
fn posterior_tail_beyond_audio_never_emits_an_empty_region() {
    let mut vad = detector();
    let mut probabilities = vec![0.; 27];
    probabilities.extend([1.; 17]);
    CALLS.with_borrow_mut(|c| c.posteriors = probabilities);
    // A mismatched model output starts speech at frame 29. The padded start
    // and clipped end both land at 2720 samples. Rejection is also acceptable.
    if let Ok(regions) = vad.speech_timestamps(&[0.; 2720]) {
        assert!(
            regions.is_empty(),
            "emitted degenerate sample range: {regions:?}"
        );
    }
}
