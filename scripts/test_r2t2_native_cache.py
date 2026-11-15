"""Opt-in CUDA regression: capture a real baseline, then check a candidate DLL.

Capture: --model MODEL --library BASELINE_DLL --dependencies DLL_DIR --output baseline.json
Check:   --model MODEL --library CANDIDATE_DLL --dependencies DLL_DIR --reference baseline.json --output candidate.json
--list-cases reads fixtures only; it never loads a DLL or touches the GPU.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import sys
import time
import wave

import numpy as np


ROOT = Path(__file__).resolve().parents[1]
RESOURCES = ROOT / "src-tauri/resources"
SAMPLE_RATE = 16000
CHUNK_SAMPLES = 2560
FINISH_SAMPLES = 5120
DEFAULT_ZH = ROOT / "tmp/asr-caption-20261017/fixtures/asr_zh.wav"
DEFAULT_EN = ROOT / "tmp/asr-engine-20261017/upstream/samples/fleurs-en.wav"


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def sha256(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def read_wav(path):
    with wave.open(str(path), "rb") as source:
        require((source.getframerate(), source.getnchannels(), source.getsampwidth()) ==
                (SAMPLE_RATE, 1, 2), f"{path}: expected mono 16 kHz PCM16 WAV")
        pcm = np.frombuffer(source.readframes(source.getnframes()), dtype="<i2").astype(np.float32) / 32768
    require(pcm.size > 32000 + 5123, f"{path}: fixture too short for restored speech tail")
    return pcm


def make_cases(args):
    clips, fixtures, cases = {}, {}, {}
    for language, path in (("zh", args.zh_wav), ("en", args.en_wav)):
        if args.fixture not in ("all", language):
            continue
        clips[language] = clip = read_wav(path)
        fixtures[language] = {"path": str(path.absolute()), "sha256": sha256(path), "samples": int(clip.size)}
        cases[f"{language}-full"] = (clip, "finish")
        if clip.size % 160 == 0:


            cases[f"{language}-nonhop"] = (clip[:-37], "finish")
        cases[f"{language}-cut-2s"] = (clip[:32000], "finish")
        cases[f"{language}-restored-5123"] = (clip[:37123], "finish")
        require(np.any(clip[32000:37123] != 0), f"{language}: restored tail contains no fixture signal")

    if "zh" in clips:
        clip = clips["zh"]
        louder = clip.copy()
        louder[:32000] *= np.float32(0.03)
        louder[32000:] *= np.float32(2.4)
        cases["zh-louder-append"] = (louder, "finish")
        cases["zh-rolling-36s"] = (np.tile(clip, (36 * SAMPLE_RATE + clip.size - 1) // clip.size)
                                    [:36 * SAMPLE_RATE], "finish")

    primary = next(iter(clips.values()))


    cases["reset-prime"] = (primary[:32000], "cancel")
    cases["short-after-reset"] = (primary[:5001], "finish")
    cases["start-prime"] = (primary[:32000], "restart")
    cases["silence-after-start"] = (np.zeros(32013, dtype=np.float32), "finish")
    if args.case:
        unknown = set(args.case) - cases.keys()
        require(not unknown, f"unknown cases: {sorted(unknown)}")
        cases = {name: value for name, value in cases.items() if name in args.case}
    return cases, fixtures


def run_case(native, session, session_id, name, pcm, completion):
    started = time.perf_counter()
    response = session.start(session_id)
    start_ms = (time.perf_counter() - started) * 1000
    require(response["sample_count"] == 0 and response["text"] == "" and
            response["tentative_text"] == "", f"{name}: start retained prior result")
    require(native.preview_text == "" and native._offset == 0, f"{name}: native start retained prior state")
    trace, durations, previous = [], [], ""
    for offset in range(0, int(pcm.size), CHUNK_SAMPLES):
        chunk = pcm[offset:offset + CHUNK_SAMPLES]
        before = time.perf_counter()
        response = session.feed(session_id, chunk, offset=offset)
        elapsed = (time.perf_counter() - before) * 1000
        accepted = offset + int(chunk.size)
        preview = response["text"] + response["tentative_text"]
        require(response["text"].startswith(previous), f"{name}: committed prefix regressed at {offset}")
        require(native.preview_text == preview and preview.startswith(response["text"]),
                f"{name}: preview lost committed prefix at {offset}")
        require(response["sample_count"] == accepted and native._offset == accepted,
                f"{name}: audio samples dropped at {offset}")
        trace.append({"input_offset": offset, "input_samples": int(chunk.size),
                      "response": response, "preview": preview, "native_samples": native._offset,
                      "ms": elapsed})
        durations.append(elapsed)
        previous = response["text"]
        if len(trace) % 40 == 0:
            print(f"PROGRESS {name} {accepted}/{pcm.size} samples", flush=True)

    before = time.perf_counter()
    if completion == "finish":
        final = session.finish(session_id)
        require(final["text"].startswith(previous), f"{name}: finish lost committed text")
        require(final["sample_count"] == int(pcm.size), f"{name}: finish changed captured sample count")
        require(native._offset == int(pcm.size) + FINISH_SAMPLES,
                f"{name}: finish did not preserve the original 320 ms padding")
        require(native.preview_text == "" and not session.active and not native._active,
                f"{name}: finish left active state or preview")
    elif completion == "cancel":
        final = session.cancel(session_id)
        require(final["sample_count"] == int(pcm.size), f"{name}: reset changed captured sample count")
        require(native.preview_text == "" and native._offset == 0 and not session.active,
                f"{name}: reset leaked state")
    else:

        final = None
        require(session.active, f"{name}: priming stream ended prematurely")
    finish_ms = (time.perf_counter() - before) * 1000
    if name.endswith("-full") or name == "zh-rolling-36s":
        require(final is not None and bool(final["text"]), f"{name}: complete speech produced no final text")
    if name == "silence-after-start":
        require(not any(item["preview"] or item["response"]["text"] for item in trace) and
                final["text"] == "", "silence retained or invented speech")
    result = {
        "pcm_samples": int(pcm.size), "pcm_sha256": hashlib.sha256(pcm.tobytes()).hexdigest(),
        "completion": completion, "feeds": trace, "final": final,
        "final_preview": native.preview_text, "final_native_samples": native._offset,
        "timings": {"start_ms": start_ms, "feed_ms": durations,
                    "feed_p50_ms": float(np.percentile(durations, 50)),
                    "feed_p95_ms": float(np.percentile(durations, 95)),
                    "feed_max_ms": max(durations), "finish_or_reset_ms": finish_ms},
    }
    print(f"CASE {name} samples={pcm.size} feeds={len(trace)} "
          f"p95_ms={result['timings']['feed_p95_ms']:.2f} finish_ms={finish_ms:.2f}", flush=True)
    return result


def observable(case):

    result = {key: value for key, value in case.items() if key != "timings" and key != "feeds"} | {
        "feeds": [{key: value for key, value in item.items() if key != "ms"} for item in case["feeds"]]}


    for item in result["feeds"]:
        item["response"] = {key: value for key, value in item["response"].items() if key != "session_id"}
    if result["final"] is not None:
        result["final"] = {key: value for key, value in result["final"].items() if key != "session_id"}
    return result


def tail_pairs(cases):
    pairs = {}
    for language in ("zh", "en"):
        cut_name, restored_name = f"{language}-cut-2s", f"{language}-restored-5123"
        if cut_name not in cases or restored_name not in cases:
            continue
        cut, restored = cases[cut_name], cases[restored_name]
        require(restored["pcm_samples"] - cut["pcm_samples"] == 5123, f"{language}: restored tail truncated")
        cut_text, restored_text = cut["final"]["text"], restored["final"]["text"]
        pairs[language] = {"cut_case": cut_name, "restored_case": restored_name,
                           "restored_samples": 5123, "cut_text": cut_text, "restored_text": restored_text,
                           "text_changed": cut_text != restored_text}
    return pairs


def check_reference_inputs(reference, actual):
    require(reference["status"] == "pass", "reference capture did not pass")
    require(reference["schema"] == actual["schema"], "reference schema changed")
    require(reference["config"] == actual["config"], "stream/padding configuration changed")
    for key in ("model_sha256", "native_wrapper_sha256", "stream_wrapper_sha256", "script_sha256"):
        require(reference["provenance"][key] == actual["provenance"][key], f"{key}: reference input changed")


def compare(reference, actual):
    for name, case in actual["cases"].items():
        require(name in reference["cases"], f"{name}: missing baseline case")
        expected = observable(reference["cases"][name])
        observed = observable(case)
        for key in expected:
            if key == "feeds":
                require(len(observed[key]) == len(expected[key]), f"{name}: feed count changed")
                for index, (before, after) in enumerate(zip(expected[key], observed[key])):
                    require(before == after, f"{name}: committed/preview/sample trace changed at feed {index}")
            else:
                require(expected[key] == observed[key], f"{name}: {key} changed")
    for language, pair in actual["tail_pairs"].items():
        require(reference["tail_pairs"].get(language) == pair, f"{language}: observed restored-tail behavior changed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path)
    parser.add_argument("--library", type=Path)
    parser.add_argument("--dependencies", type=Path)
    parser.add_argument("--reference", type=Path, help="Frozen baseline JSON; omit to capture")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--fixture", choices=("all", "zh", "en"), default="all")
    parser.add_argument("--zh-wav", type=Path, default=DEFAULT_ZH)
    parser.add_argument("--en-wav", type=Path, default=DEFAULT_EN)
    parser.add_argument("--case", action="append", help="Optional subset for a smoke check; may repeat")
    parser.add_argument("--list-cases", action="store_true")
    args = parser.parse_args()
    cases, fixtures = make_cases(args)
    if args.list_cases:
        print(json.dumps({name: {"samples": int(pcm.size), "completion": completion}
                          for name, (pcm, completion) in cases.items()}, indent=2))
        return 0
    for name in ("model", "library", "dependencies", "output"):
        if getattr(args, name) is None:
            parser.error(f"--{name} is required for capture/check")
    require(args.model.is_file() and args.library.is_file(), "model/library path missing")
    require(args.dependencies.is_dir(), "dependency directory missing")
    require(not args.output.exists(), "output exists; choose a new capture/check path")
    reference = json.loads(args.reference.read_text(encoding="utf-8")) if args.reference else None
    require(args.reference is None or args.output.absolute() != args.reference.absolute(), "output would overwrite reference")

    sys.path.insert(0, str(RESOURCES))
    from r2t2_native import NativeRuntime
    from r2t2_stream import R2T2StreamSession
    require(NativeRuntime.FINISH_CONTEXT_SAMPLES == FINISH_SAMPLES, "original finish padding changed")
    output = {
        "schema": 1, "mode": "check" if reference else "capture",
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "config": {"backend": "cuda", "rolling": True, "chunk_samples": CHUNK_SAMPLES,
                   "sample_rate": SAMPLE_RATE, "finish_context_samples": FINISH_SAMPLES},
        "provenance": {"library": str(args.library.absolute()), "library_sha256": sha256(args.library),
                       "model": str(args.model.absolute()), "model_sha256": sha256(args.model),
                       "dependencies": str(args.dependencies.absolute()),
                       "native_wrapper_sha256": sha256(RESOURCES / "r2t2_native.py"),
                       "stream_wrapper_sha256": sha256(RESOURCES / "r2t2_stream.py"),
                       "script_sha256": sha256(Path(__file__)), "fixtures": fixtures},
        "cases": {}, "tail_pairs": {}, "status": "running",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)

    def save():
        args.output.write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    native = None
    try:
        if reference:
            check_reference_inputs(reference, output)
            for name, (pcm, completion) in cases.items():
                require(name in reference["cases"], f"{name}: missing baseline case")
                expected = reference["cases"][name]
                require(expected["pcm_samples"] == int(pcm.size) and
                        expected["pcm_sha256"] == hashlib.sha256(pcm.tobytes()).hexdigest() and
                        expected["completion"] == completion, f"{name}: baseline PCM/case recipe changed")
        native = NativeRuntime(args.model, args.library, backend="cuda", rolling=True,
                               chunk_ms=160, dll_directories=[args.dependencies])
        session = R2T2StreamSession(native)
        for session_id, (name, (pcm, completion)) in enumerate(cases.items(), 1):
            output["cases"][name] = run_case(native, session, session_id, name, pcm, completion)
            save()
        output["tail_pairs"] = tail_pairs(output["cases"])
        if reference:
            compare(reference, output)
        output["status"] = "pass"
        save()
    except BaseException as error:
        output["status"] = "failed"
        output["error"] = f"{type(error).__name__}: {error}"
        save()
        raise
    finally:
        if native is not None:
            native.close()
    print(f"PASS {output['mode']} cases={len(cases)} output={args.output}", flush=True)
    for language, pair in output["tail_pairs"].items():
        print(f"TAIL {language} restored_samples=5123 text_changed={pair['text_changed']}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
