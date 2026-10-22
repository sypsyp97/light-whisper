//! All inputs are unconstrained usize values on the runner's 64-bit target.
//! Only frame-count arithmetic is proved; mixing and concurrency are outside this pilot.
use vstd::prelude::*;
use vstd::arithmetic::div_mod::{lemma_fundamental_div_mod, lemma_mod_bound};
use vstd::arithmetic::mul::{lemma_mul_inequality, lemma_mul_is_commutative};

verus! {

spec fn normalized_channels(channels: usize) -> int {
    if channels == 0 { 1 } else { channels as int }
}

spec fn expected_frames(input_len: usize, channels: usize, used: usize, cap: usize) -> int {
    let frames = (input_len as int) / normalized_channels(channels);
    let room = if used <= cap { (cap as int) - (used as int) } else { 0 };
    if frames <= room { frames } else { room }
}

pub(crate) fn capped_frame_count(
    input_len: usize,
    channels: usize,
    used: usize,
    cap: usize,
) -> (result: usize)
    ensures
        result as int == expected_frames(input_len, channels, used, cap),
        result as int <= (input_len as int) / normalized_channels(channels),
        (result as int) * normalized_channels(channels) <= input_len as int,
        used >= cap ==> result == 0,
        used <= cap ==> (used as int) + (result as int) <= cap as int,
{
    let result = {
/*__CAPTURE_BOUNDS_BODY__*/
    };
    proof {
        let width = normalized_channels(channels);
        lemma_fundamental_div_mod(input_len as int, width);
        lemma_mod_bound(input_len as int, width);
        lemma_mul_is_commutative(width, (input_len as int) / width);
        lemma_mul_inequality(result as int, (input_len as int) / width, width);
    }
    result
}

}
