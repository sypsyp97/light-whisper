"""GPU idle-unload policy for the resident local ASR process.

0 seconds disables the feature. The default is off: no timer, no unload,
and no change to status. A positive value unloads the loaded model after
that many seconds without a transcribe or stream command. status, stats
and cleanup are not activity.
"""

from __future__ import annotations

import os

GPU_IDLE_ENV = "LIGHT_WHISPER_GPU_IDLE_SECONDS"
# One week is a fat-finger cap, not a product default.
MAX_GPU_IDLE_SECONDS = 7 * 24 * 60 * 60

GPU_IDLE_ACTIVITY_ACTIONS = frozenset(
    {
        "transcribe",
        "stream_start",
        "stream_feed",
        "stream_finish",
        "stream_cancel",
    }
)
GPU_IDLE_LOCKED_ACTIONS = GPU_IDLE_ACTIVITY_ACTIONS | frozenset(
    {"status", "stats", "cleanup", "set_audio_processing"}
)


def parse_gpu_idle_seconds(value) -> int:
    """Return a non-negative idle timeout. Invalid values and 0 mean off."""
    if isinstance(value, bool) or value is None:
        return 0
    if isinstance(value, int):
        seconds = value
    elif isinstance(value, float):
        if not value.is_integer():
            return 0
        seconds = int(value)
    elif isinstance(value, str):
        text = value.strip()
        if not text:
            return 0
        try:
            seconds = int(text, 10)
        except ValueError:
            return 0
    else:
        return 0
    if seconds <= 0:
        return 0
    return min(seconds, MAX_GPU_IDLE_SECONDS)


def gpu_idle_seconds_from_env(environ=None) -> int:
    env = os.environ if environ is None else environ
    if GPU_IDLE_ENV not in env:
        return 0
    return parse_gpu_idle_seconds(env.get(GPU_IDLE_ENV))


def gpu_idle_should_unload(
    *,
    now: float,
    last_activity: float | None,
    idle_seconds: int,
    stream_active: bool,
) -> bool:
    """True when the model should be unloaded.

    Idle is measured from the end of the last transcribe/stream command.
    Enabling the policy (including at startup) starts a fresh idle window.
    An active R2T2 stream always blocks unload.
    """
    if idle_seconds <= 0 or stream_active or last_activity is None:
        return False
    return (now - last_activity) >= idle_seconds


def status_keeps_process_ready(was_ready: bool, status: dict) -> bool:
    """Mirror Rust ``check_status``: a status reply never clears ready.

    Ready is sticky. It becomes true when the payload says the model is
    initialized or loaded, and otherwise stays at its previous value.
    Unload may report ``model_loaded: false`` without looking like a crash.
    """
    model_loaded = bool(status.get("model_loaded"))
    initialized = bool(status.get("initialized")) or model_loaded
    return bool(was_ready) or initialized
