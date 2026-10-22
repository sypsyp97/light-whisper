//! The caller starts at the base interval and assigns every update through this function.
//! Elapsed time is arbitrary; timers and inference execution remain outside this proof.
use vstd::prelude::*;

verus! {

mod verified_interval {
use vstd::prelude::*;

/*__INTERIM_CONSTANTS__*/

spec fn expected_interval(current: u64, executed: bool, elapsed_ms: u64) -> int {
    if !executed {
        if (current as int) - 8 < (INTERIM_INTERVAL_MIN_MS as int) {
            INTERIM_INTERVAL_MIN_MS as int
        } else if (current as int) - 8 > (INTERIM_INTERVAL_BASE_MS as int) {
            INTERIM_INTERVAL_BASE_MS as int
        } else {
            current as int - 8
        }
    } else if elapsed_ms >= INTERIM_HEAVY_COST_MS {
        if (current as int) + (INTERIM_INTERVAL_UP_STEP_MS as int) > (INTERIM_INTERVAL_MAX_MS as int) {
            INTERIM_INTERVAL_MAX_MS as int
        } else {
            current as int + INTERIM_INTERVAL_UP_STEP_MS as int
        }
    } else if elapsed_ms <= INTERIM_LIGHT_COST_MS {
        if (current as int) - (INTERIM_INTERVAL_DOWN_STEP_MS as int) < (INTERIM_INTERVAL_MIN_MS as int) {
            INTERIM_INTERVAL_MIN_MS as int
        } else {
            current as int - INTERIM_INTERVAL_DOWN_STEP_MS as int
        }
    } else if current > INTERIM_INTERVAL_BASE_MS {
        if (current as int) - 8 < (INTERIM_INTERVAL_BASE_MS as int) {
            INTERIM_INTERVAL_BASE_MS as int
        } else {
            current as int - 8
        }
    } else if current < INTERIM_INTERVAL_BASE_MS {
        if (current as int) + 4 > (INTERIM_INTERVAL_BASE_MS as int) {
            INTERIM_INTERVAL_BASE_MS as int
        } else {
            current as int + 4
        }
    } else {
        current as int
    }
}

fn adjust_interval(current: u64, executed: bool, elapsed_ms: u64) -> (result: u64)
    requires
        INTERIM_INTERVAL_MIN_MS <= current <= INTERIM_INTERVAL_MAX_MS,
    ensures
        result as int == expected_interval(current, executed, elapsed_ms),
        INTERIM_INTERVAL_MIN_MS <= result <= INTERIM_INTERVAL_MAX_MS,
        !executed ==> result <= INTERIM_INTERVAL_BASE_MS,
        (!executed || elapsed_ms <= INTERIM_LIGHT_COST_MS) ==> (
            result <= current && (current > INTERIM_INTERVAL_MIN_MS ==> result < current)
        ),
        (executed && elapsed_ms >= INTERIM_HEAVY_COST_MS) ==> (
            result >= current && (current < INTERIM_INTERVAL_MAX_MS ==> result > current)
        ),
        (executed && INTERIM_LIGHT_COST_MS < elapsed_ms < INTERIM_HEAVY_COST_MS) ==> (
            (current > INTERIM_INTERVAL_BASE_MS ==> INTERIM_INTERVAL_BASE_MS <= result < current)
            && (current < INTERIM_INTERVAL_BASE_MS ==> current < result <= INTERIM_INTERVAL_BASE_MS)
            && (current == INTERIM_INTERVAL_BASE_MS ==> result == current)
        ),
{
/*__INTERVAL_BODY__*/
}

}

}
