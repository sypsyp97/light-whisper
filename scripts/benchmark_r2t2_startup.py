"""Compare native startup previews and exact committed traces on local PCM WAVs.

Run once with the baseline DLL and --output, then with a candidate DLL,
--baseline pointing to that JSON, and --max-first-audio-seconds 0.96.
This is an opt-in GPU integration check, not a microphone/UI latency test.
"""
import argparse
import json
from pathlib import Path
import sys
import time
import wave

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src-tauri/resources"))
from r2t2_native import NativeRuntime


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--dependencies", type=Path, required=True)
    parser.add_argument("--wav", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--max-first-audio-seconds", type=float)
    args = parser.parse_args()
    clips = {}
    for path in args.wav:
        with wave.open(str(path), "rb") as source:
            assert (source.getframerate(), source.getnchannels(), source.getsampwidth()) == (16000, 1, 2)
            clips[path.stem] = np.frombuffer(source.readframes(source.getnframes()), dtype="<i2").astype(np.float32) / 32768
    clips["silence"] = np.zeros(32000, dtype=np.float32)
    # Repeated startup/reset and sub-word truncations must not contaminate final text.
    for name, clip in list(clips.items()):
        if name != "silence":
            clips[name + "-short"] = clip[:12800]
    results = {}
    native = NativeRuntime(args.model, args.library, rolling=True, chunk_ms=160,
                           dll_directories=[args.dependencies])
    try:
        for name, clip in clips.items():
            native.start()
            committed, previews, durations = [], [], []
            previous = ""
            for start in range(0, len(clip), 2560):
                before = time.perf_counter()
                text, _ = native.feed(clip[start:start + 2560])
                durations.append((time.perf_counter() - before) * 1000)
                assert text.startswith(previous), "committed text regressed"
                assert native.preview_text.startswith(text), "preview lost committed prefix"
                committed.append(text)
                previews.append(native.preview_text)
                previous = text
            final, _ = native.finish()
            assert final.startswith(previous)
            assert native.preview_text == "", "final preview leaked into next session"
            first = next(((i + 1) * .16 for i, text in enumerate(previews) if text), None)
            results[name] = dict(first_audio_s=first, final=final, committed=committed,
                                 previews=previews, p95_ms=float(np.percentile(durations, 95)))
    finally:
        native.close()
    args.output.write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    if args.baseline:
        baseline = json.loads(args.baseline.read_text(encoding="utf-8"))
        assert results.keys() == baseline.keys()
        for name, actual in results.items():
            assert actual["committed"] == baseline[name]["committed"], f"{name}: committed trace changed"
            assert actual["final"] == baseline[name]["final"], f"{name}: final text changed"
    assert not any(results["silence"]["previews"]) and not results["silence"]["final"]
    for path in args.wav:
        result = results[path.stem]
        print(path.stem, result["first_audio_s"], result["p95_ms"], flush=True)
        if args.max_first_audio_seconds is not None:
            assert result["first_audio_s"] is not None
            assert result["first_audio_s"] <= args.max_first_audio_seconds, "startup latency target missed"


if __name__ == "__main__":
    main()
