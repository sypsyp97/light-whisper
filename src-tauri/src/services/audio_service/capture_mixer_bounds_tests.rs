//! Golden outputs protect conversion/mixing while frame-count arithmetic moves.

use super::capture::{mix_to_mono_capped_f32, mix_to_mono_capped_i16, mix_to_mono_capped_u16};

#[test]
fn zero_channels_means_mono_and_preserves_format_conversion() {
    let mut signed = vec![99];
    mix_to_mono_capped_i16(&[-3, 2], 0, &mut signed, 10);
    assert_eq!(signed, [99, -3, 2]);

    let mut floating = vec![99];
    mix_to_mono_capped_f32(
        &[-1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5],
        0,
        &mut floating,
        10,
    );
    assert_eq!(
        floating,
        [99, -32768, -32768, -16384, 0, 16383, 32767, 32767]
    );

    let mut unsigned = vec![99];
    mix_to_mono_capped_u16(&[0, 32768, 65535], 0, &mut unsigned, 10);
    assert_eq!(unsigned, [99, -32768, 0, 32767]);
}

#[test]
fn stereo_averages_only_complete_frames() {
    let mut signed = vec![99];
    mix_to_mono_capped_i16(&[-9, 4, 100, 300, 1000], 2, &mut signed, 10);
    assert_eq!(signed, [99, -2, 200]);

    let mut floating = vec![99];
    mix_to_mono_capped_f32(&[-1.0, 0.0, 0.5, 1.0, 1.0], 2, &mut floating, 10);
    assert_eq!(floating, [99, -16384, 24575]);

    let mut unsigned = vec![99];
    mix_to_mono_capped_u16(&[0, 65535, 32768, 32770, 40000], 2, &mut unsigned, 10);
    assert_eq!(unsigned, [99, -1, 1]);
}

#[test]
fn existing_samples_count_toward_cap_and_are_never_replaced() {
    for cap in 0..=3 {
        let mut signed = vec![91, 92];
        let mut floating = signed.clone();
        let mut unsigned = signed.clone();
        mix_to_mono_capped_i16(&[-9, 4, 100, 300], 2, &mut signed, cap);
        mix_to_mono_capped_f32(&[-1.0, 0.0, 0.5, 1.0], 2, &mut floating, cap);
        mix_to_mono_capped_u16(&[0, 65535, 32768, 32770], 2, &mut unsigned, cap);
        if cap <= 2 {
            assert_eq!(signed, [91, 92]);
            assert_eq!(floating, [91, 92]);
            assert_eq!(unsigned, [91, 92]);
        } else {
            assert_eq!(signed, [91, 92, -2]);
            assert_eq!(floating, [91, 92, -16384]);
            assert_eq!(unsigned, [91, 92, -1]);
        }
    }
}
