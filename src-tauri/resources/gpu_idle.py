







from __future__ import annotations

import os

GPU_IDLE_ENV = "LIGHT_WHISPER_GPU_IDLE_SECONDS"

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






    if idle_seconds <= 0 or stream_active or last_activity is None:
        return False
    return (now - last_activity) >= idle_seconds


def status_keeps_process_ready(was_ready: bool, status: dict) -> bool:






    model_loaded = bool(status.get("model_loaded"))
    initialized = bool(status.get("initialized")) or model_loaded
    return bool(was_ready) or initialized
