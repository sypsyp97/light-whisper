use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::{Duration, Instant};

use rubato::{FastFixedIn, PolynomialDegree, Resampler};

const TARGET_SAMPLE_RATE: u32 = 16_000;
const INPUT_RATES: [u32; 9] = [
    100, 8_000, 16_000, 44_100, 48_000, 96_000, 409_599, 409_600, 480_000,
];

#[expect(dead_code, reason = "this isolated harness only exercises resampling")]
#[path = "../src/services/audio_service/resample.rs"]
mod resample;

// This allocator belongs only to this integration-test executable. Const TLS
// isolates the measured thread from the test runner without allocating itself.
struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

fn record_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}

// SAFETY: Counting observes calls only. System owns every allocation and receives
// the original arguments for allocation, reallocation, and deallocation.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: Forward the caller's valid layout unchanged to System.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: Forward the caller's valid layout unchanged to System.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The pointer and its original layout come from this System wrapper.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: Preserve the System pointer, its layout, and the valid new size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn count_allocations<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    struct StopCounting;
    impl Drop for StopCounting {
        fn drop(&mut self) {
            ALLOCATIONS.with(|count| count.set(None));
        }
    }

    ALLOCATIONS.with(|count| {
        assert_eq!(count.get(), None, "allocation measurements cannot nest");
        count.set(Some(0));
    });
    let stop = StopCounting;
    let result = operation();
    let count = ALLOCATIONS.with(|count| count.get().unwrap());
    drop(stop);
    (result, count)
}

fn chunk_size(rate: u32) -> usize {
    (rate as usize / 100).clamp(1, 4096)
}

fn signal(len: usize) -> Vec<i16> {
    (0..len)
        .map(|index| match index % 16 {
            0 => i16::MIN,
            1 => i16::MAX,
            2 => 0,
            3 => -1,
            4 => 1,
            _ => (index as u32).wrapping_mul(25_173).wrapping_add(13_849) as i16,
        })
        .collect()
}

// Freeze the allocating rubato flow independently of production buffering and
// conversion helpers. Partial processing must retain rubato's zero-padding,
// output length, interpolation history, and asymmetric i16 quantization.
struct LegacyReference {
    inner: Option<FastFixedIn<f32>>,
    pending: Vec<f32>,
    chunk_size: usize,
}

impl LegacyReference {
    fn new(rate: u32) -> Self {
        let chunk_size = chunk_size(rate);
        Self {
            inner: (rate != TARGET_SAMPLE_RATE).then(|| {
                FastFixedIn::new(
                    TARGET_SAMPLE_RATE as f64 / rate as f64,
                    1.1,
                    PolynomialDegree::Cubic,
                    chunk_size,
                    1,
                )
                .unwrap()
            }),
            pending: Vec::new(),
            chunk_size,
        }
    }

    fn process(&mut self, input: &[i16], output: &mut Vec<i16>) {
        let Some(inner) = self.inner.as_mut() else {
            output.extend_from_slice(input);
            return;
        };
        self.pending
            .extend(input.iter().map(|&sample| sample as f32 / 32768.0));
        while self.pending.len() >= self.chunk_size {
            let chunk: Vec<_> = self.pending.drain(..self.chunk_size).collect();
            let channels = inner.process(&[chunk], None).unwrap();
            Self::append(&channels[0], output);
        }
    }

    fn finish(&mut self, output: &mut Vec<i16>) {
        if !self.pending.is_empty() {
            let chunk = std::mem::take(&mut self.pending);
            let channels = self
                .inner
                .as_mut()
                .unwrap()
                .process_partial(Some(&[chunk]), None)
                .unwrap();
            Self::append(&channels[0], output);
        }
    }

    fn append(input: &[f32], output: &mut Vec<i16>) {
        output.extend(input.iter().map(|&sample| {
            let sample = sample.clamp(-1.0, 1.0);
            (sample * if sample < 0.0 { 32768.0 } else { 32767.0 }) as i16
        }));
    }
}

fn assert_samples(actual: &[i16], expected: &[i16], rate: u32) {
    assert_eq!(actual.len(), expected.len(), "output length at {rate} Hz");
    if let Some(index) = actual.iter().zip(expected).position(|(a, b)| a != b) {
        panic!(
            "{rate} Hz sample {index}: actual {}, expected {}",
            actual[index], expected[index]
        );
    }
}

#[test]
fn chunked_output_matches_legacy_for_irregular_packets_and_repeated_flushes() {
    for rate in INPUT_RATES {
        let chunk = chunk_size(rate);
        let sizes = [0, 1, chunk - 1, chunk, chunk + 1, 3, chunk * 3 + 7, 0];
        let input = signal(sizes.iter().sum());
        for flush_each_packet in [false, true] {
            let mut subject = resample::ChunkedResampler::new(rate).unwrap();
            let mut reference = LegacyReference::new(rate);
            let (mut actual, mut expected) = (Vec::new(), Vec::new());
            subject.finish(&mut actual).unwrap();
            reference.finish(&mut expected);
            assert_samples(&actual, &expected, rate);
            let mut offset = 0;
            for size in sizes {
                let packet = &input[offset..offset + size];
                offset += size;
                subject.process_chunk(packet, &mut actual).unwrap();
                reference.process(packet, &mut expected);
                assert_samples(&actual, &expected, rate);
                if flush_each_packet {
                    for _ in 0..2 {
                        subject.finish(&mut actual).unwrap();
                        reference.finish(&mut expected);
                        assert_samples(&actual, &expected, rate);
                    }
                }
            }
            for _ in 0..2 {
                subject.finish(&mut actual).unwrap();
                reference.finish(&mut expected);
                assert_samples(&actual, &expected, rate);
            }
        }
        println!("parity rate={rate} irregular_packets=8 flush_modes=2");
    }
}

#[test]
fn stateful_output_matches_legacy_with_a_partial_flush_after_each_delta() {
    for rate in INPUT_RATES
        .into_iter()
        .filter(|&rate| rate != TARGET_SAMPLE_RATE)
    {
        let chunk = chunk_size(rate);
        let input = signal(chunk * 3 + 7);
        let mut subject = resample::ResamplerState::new(rate).unwrap();
        let mut reference = LegacyReference::new(rate);
        for size in [0, 1, chunk - 1, chunk + 1, chunk * 3 + 7, 0, 3] {
            let packet = &input[..size];
            let mut expected = Vec::new();
            reference.process(packet, &mut expected);
            reference.finish(&mut expected);
            let actual = subject.push_i16(packet).unwrap();
            assert_samples(&actual, &expected, rate);
        }
        println!("stateful_parity rate={rate} deltas=7");
    }
}

#[test]
fn process_and_finish_reuse_preallocated_buffers() {
    let (probe, allocations) = count_allocations(|| std::hint::black_box(vec![1_u8; 32]));
    assert_eq!(allocations, 1, "allocator positive control");
    drop(probe);

    let mut counts = [[[0; 3]; 3]; INPUT_RATES.len()];
    for (rate_index, rate) in INPUT_RATES.into_iter().enumerate() {
        let chunk = chunk_size(rate);
        let input = signal(chunk * 4 - 1);
        let mut subject = resample::ChunkedResampler::new(rate).unwrap();
        // More than a full padded input packet at the ceiling of the ratio;
        // caller output growth must never be included in the measurement.
        let capacity =
            (input.len() + chunk) * (TARGET_SAMPLE_RATE as usize).div_ceil(rate as usize) + 32;
        let mut output = Vec::with_capacity(capacity);
        let original_capacity = output.capacity();
        for (round, phases) in counts[rate_index].iter_mut().enumerate() {
            output.clear();
            let (result, allocations) =
                count_allocations(|| subject.process_chunk(&input, &mut output));
            result.unwrap();
            phases[0] = allocations;
            let (result, allocations) = count_allocations(|| subject.finish(&mut output));
            result.unwrap();
            phases[1] = allocations;
            let (result, allocations) = count_allocations(|| {
                subject.process_chunk(&[], &mut output)?;
                subject.finish(&mut output)
            });
            result.unwrap();
            phases[2] = allocations;
            assert_eq!(output.capacity(), original_capacity, "caller output grew");
            println!(
                "allocations rate={rate} round={round} process={} finish={} empty={}",
                phases[0], phases[1], phases[2]
            );
        }
    }
    assert!(
        counts.iter().flatten().flatten().all(|&count| count == 0),
        "process_chunk and finish must allocate zero times after construction/output reservation"
    );
}

#[test]
#[ignore = "timing only: compile this test with rustc -O against release rubato"]
fn benchmark_resample_buffer_reuse() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "benchmark requires an optimized build"
    );
    for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
        let input = signal(rate as usize * 2 + 47);
        let mut reference = LegacyReference::new(rate);
        let mut expected = Vec::new();
        reference.process(&input, &mut expected);
        reference.finish(&mut expected);
        let mut timings = [Duration::ZERO; 21];
        for elapsed in &mut timings {
            let mut subject = resample::ChunkedResampler::new(rate).unwrap();
            let mut output = Vec::with_capacity(expected.len());
            let start = Instant::now();
            subject
                .process_chunk(std::hint::black_box(&input), &mut output)
                .unwrap();
            subject.finish(&mut output).unwrap();
            *elapsed = start.elapsed();
            assert_samples(&output, &expected, rate);
        }
        timings.sort_unstable();
        println!(
            "benchmark rate={rate} input={} output={} runs=21 min_ns={} median_ns={} max_ns={}",
            input.len(),
            expected.len(),
            timings[0].as_nanos(),
            timings[10].as_nanos(),
            timings[20].as_nanos()
        );
    }
}
