//! Arithmetic-only tests for the production capture bounds module.

use super::capture_bounds::capped_frame_count;

#[test]
fn cap_and_complete_frames_have_explicit_boundary_examples() {
    // (input samples, channels, existing mono samples, cap, appended frames)
    let cases = [
        (7, 0, 2, 6, 4),
        (7, 1, 2, 6, 4),
        (7, 3, 0, 10, 2),
        (7, 3, 4, 5, 1),
        (2, 3, 0, 10, 0),
        (0, 0, 0, 10, 0),
        (7, 2, 5, 5, 0),
        (7, 2, 6, 5, 0),
        (7, 0, 3, 0, 0),
        (usize::MAX, 0, 0, usize::MAX, usize::MAX),
        (usize::MAX, 2, 0, usize::MAX, usize::MAX / 2),
        (usize::MAX, usize::MAX, 0, usize::MAX, 1),
        (usize::MAX - 1, usize::MAX, 0, usize::MAX, 0),
        (usize::MAX, 1, usize::MAX - 1, usize::MAX, 1),
        (usize::MAX, 1, usize::MAX, 0, 0),
    ];
    for (input_len, channels, used, cap, expected) in cases {
        assert_eq!(
            capped_frame_count(input_len, channels, used, cap),
            expected,
            "input={input_len}, channels={channels}, used={used}, cap={cap}",
        );
    }
}

#[test]
fn small_domain_matches_appending_one_complete_frame_at_a_time() {
    // This oracle simulates consumption rather than repeating division/min.
    for input_len in 0..=9 {
        for channels in 0..=5 {
            for used in 0..=6 {
                for cap in 0..=6 {
                    let width = if channels == 0 { 1 } else { channels };
                    let mut consumed = 0;
                    let mut appended = 0;
                    while consumed + width <= input_len && used + appended < cap {
                        consumed += width;
                        appended += 1;
                    }
                    assert_eq!(
                        capped_frame_count(input_len, channels, used, cap),
                        appended,
                        "input={input_len}, channels={channels}, used={used}, cap={cap}",
                    );
                }
            }
        }
    }
}

#[test]
fn extreme_values_satisfy_widened_safety_and_maximality_oracle() {
    assert_eq!(
        usize::BITS,
        64,
        "this pilot targets 64-bit capture arithmetic"
    );
    let edges = [0, 1, 2, 3, usize::MAX / 2, usize::MAX - 1, usize::MAX];
    for input_len in edges {
        for channels in edges {
            for used in edges {
                for cap in edges {
                    let result = capped_frame_count(input_len, channels, used, cap) as u128;
                    let width = if channels == 0 { 1 } else { channels as u128 };
                    let input = input_len as u128;
                    let used = used as u128;
                    let cap = cap as u128;
                    assert!(result * width <= input);
                    assert!(result <= input / width);
                    if used >= cap {
                        assert_eq!(result, 0);
                    } else {
                        assert!(used + result <= cap);
                        // A safe result is exact only if one more frame cannot fit.
                        assert!((result + 1) * width > input || used + result == cap);
                    }
                }
            }
        }
    }
}
