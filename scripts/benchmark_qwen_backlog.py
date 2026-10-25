"""Isolated CUDA A/B scheduling replay; --self-test needs no model or GPU.

Paced capture releases 640 samples every 40 ms; Stop is the full clip deadline.
Backlog begins with all audio queued at Stop. Mixed means concatenated public
ZH/EN clips, not a real code-switch recording. Exact text parity is not accuracy.
"""
import argparse
from contextlib import nullcontext
from datetime import datetime, timezone
import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from types import SimpleNamespace

sys.dont_write_bytecode = True
import numpy as np

from test_r2t2_native_cache import ROOT, DEFAULT_ZH, DEFAULT_EN, read_wav, sha256


def pcm_info(pcm):
    return {"samples": len(pcm), "sha256": hashlib.sha256(pcm.tobytes()).hexdigest()}


def replay(runtime, pcm, variant, scenario, clock=time.perf_counter, sleep=time.sleep, row=None):
    """Check Stop between feed batches, including completion of in-flight work."""
    runtime.start()
    start = clock()
    deadline = start + len(pcm) / 16000 if scenario == "paced" else start
    offset, first, accepted, feeds = 0, None, [], []
    row = {} if row is None else row
    row.update(status="running", feeds=feeds, first_nonempty_caption=None)
    batch = 0

    def feed_to(end, preview):
        nonlocal offset, first, batch
        batch += 1
        packet = 640 if variant == "A" else 5120
        while offset < end:
            stop = min(offset + packet, end)
            chunk = pcm[offset:stop]
            before = clock()
            if variant == "A":
                runtime.feed(chunk)
            else:
                runtime.feed(chunk, preview=preview and stop == end)
            accepted.append(chunk.copy())
            feeds.append({"offset": offset, "samples": len(chunk), "batch": batch,
                          "phase": "live" if preview else "stop-drain",
                          "preview": variant == "A" or (preview and stop == end),
                          "start_wall_ms": (before - start) * 1000, "ms": (clock() - before) * 1000})
            offset = stop
        if first is None and preview and runtime.preview_text:
            first = {"wall_ms": (clock() - start) * 1000, "audio_samples": offset,
                     "audio_ms": offset / 16}
            row["first_nonempty_caption"] = first

    while clock() < deadline:
        available = min(len(pcm), int((clock() - start) / .04) * 640)
        if available <= offset:
            sleep(min(.001, max(0, deadline - clock())))
            continue
        # One sink call is indivisible; Stop is checked after it returns.
        feed_to(min(offset + 640, available) if variant == "A" else available, True)
    stop_observed = clock()
    feed_to(len(pcm), False)
    final = runtime.finish()
    done = clock()
    observed = np.concatenate(accepted)
    if not np.array_equal(observed, pcm):
        raise AssertionError("accepted PCM changed or dropped its tail")
    row.update(status="pass", stop_to_final_ms=(done - deadline) * 1000,
               stop_requested_wall_ms=(deadline - start) * 1000,
               stop_observed_ms=(stop_observed - deadline) * 1000,
               accepted_pcm=pcm_info(observed), final={"text": final[0], "language": final[1]})
    return row


def save(path, report):
    temporary = path.with_suffix(path.suffix + ".partial")
    with temporary.open("x", encoding="utf-8") as output:
        output.write(json.dumps(report, ensure_ascii=False, allow_nan=False, indent=2) + "\n")
    os.replace(temporary, path)


def fixtures(args):
    zh, en = read_wav(args.zh_wav), read_wav(args.en_wav)
    return {"zh": zh, "en": en, "mixed-derived": np.concatenate((zh, en))}


def inputs(args, source, library):
    paths = [args.model, library, args.zh_wav, args.en_wav, Path(__file__),
             ROOT / "scripts/test_r2t2_native_cache.py"]
    if args.scope == "product-vad":
        paths.extend((args.vad_model, args.vad_cmvn))
    return {"files": {str(p.resolve()): sha256(p) for p in paths}, "source": str(source.resolve()),
            "source_sha256": {p.name: sha256(p) for p in sorted(source.glob("*.py"))},
            "dependencies": {str(p.resolve()): {f.name: sha256(f) for f in sorted(p.glob("*.dll"))}
                             for p in dict.fromkeys([library.parent, *args.dependencies])}}


def worker(args, report):
    source = args.baseline_source if args.variant == "A" else args.candidate_source
    library = args.baseline_library if args.variant == "A" else args.candidate_library
    native, decodes, callbacks = None, [], []
    try:
        report["inputs"] = inputs(args, source, library)
        os.environ["LIGHT_WHISPER_DATA_DIR"] = str(args.output.with_suffix("").resolve())
        sys.path.insert(0, str(source.resolve()))
        names = ["qwen3_native", "r2t2_native"]
        if args.scope == "product-vad":
            names += ["qwen3_asr_server", "firered_vad", "asr_stream_server", "r2t2_stream", "server_common", "hf_cache_utils"]
        modules = {name: importlib.import_module(name) for name in names}
        report["modules"] = {name: str(Path(module.__file__).resolve()) for name, module in modules.items()}
        for name, path in report["modules"].items():
            if Path(path) != (source / (name + ".py")).resolve():
                raise AssertionError(f"wrong imported module: {name}={path}")
        report["stage"] = "native.load"
        save(args.output, report)
        before = time.perf_counter()
        native = modules["qwen3_native"].QwenNativeRuntime(args.model, library, backend="cuda",
                                                       dll_directories=args.dependencies, threads=1)
        report.update(model_load_ms=(time.perf_counter() - before) * 1000, native_closed=False,
                      abi_version=native.functions["abi_version"]())
        native_run, session_handle = native.run, native.session.value

        def decode(pcm, **kwargs):
            if native.session.value != session_handle or not session_handle:
                raise AssertionError("resident native session changed")
            event = {"samples": len(pcm), "status": "running"}
            decodes.append(event)
            before = time.perf_counter()
            result = native_run(pcm, **kwargs)
            event.update(status="pass", ms=(time.perf_counter() - before) * 1000)
            return result

        native.run = decode  # Count actual native decodes after VAD trimming/rejection.
        callback = native.run
        if args.scope == "product-vad":
            server = modules["qwen3_asr_server"].Qwen3ASRServer.__new__(modules["qwen3_asr_server"].Qwen3ASRServer)
            server.session = native
            server.vad_model = modules["firered_vad"].FireRedVad(args.vad_model, args.vad_cmvn)
            server.stdout_suppressor = SimpleNamespace(suppress=nullcontext)
            server._vad_calls = server._vad_rejected = 0
            server._total_vad_ms = 0
            callback = server._transcribe_stream_audio

        def transcribe(pcm, **kwargs):
            callbacks.append(len(pcm))
            return callback(pcm, **kwargs)

        QwenPreviewRuntime = modules["qwen3_native"].QwenPreviewRuntime
        clips = fixtures(args)
        report["pcm"] = {key: pcm_info(pcm) for key, pcm in clips.items()}
        report["warmup"] = []
        for name, pcm in clips.items():
            for scenario in ("paced", "backlog"):
                # Complete-clip warmup through the same callback; excluded from rows.
                decodes.clear()
                report["stage"] = {"fixture": name, "scenario": scenario, "phase": "warmup"}
                warmup = {"fixture": name, "scenario": scenario, "decodes": decodes}
                report["warmup"].append(warmup)
                save(args.output, report)
                preview = QwenPreviewRuntime(transcribe)
                preview.start()
                if args.variant == "A":
                    preview.feed(pcm)
                else:
                    preview.feed(pcm, preview=False)
                preview.finish()
                warmup["decodes"] = list(decodes)
                for repeat in range(args.repeats):
                    report["stage"] = {"fixture": name, "scenario": scenario, "repeat": repeat}
                    save(args.output, report)
                    decodes.clear()
                    callbacks.clear()
                    row = {"fixture": name, "scenario": scenario, "repeat": repeat, "decodes": decodes}
                    report["rows"].append(row)
                    try:
                        replay(QwenPreviewRuntime(transcribe), pcm, args.variant, scenario, row=row)
                        if not row["final"]["text"].strip():
                            raise AssertionError(f"{name}/{scenario}: complete speech returned no final text")
                    except BaseException as error:
                        row.update(status="failed", error=f"{type(error).__name__}: {error}")
                        raise
                    finally:
                        row.update(decode_count=len(decodes), processed_audio_samples=sum(d["samples"] for d in decodes),
                                   decodes=list(decodes), callback_count=len(callbacks), callback_audio_samples=sum(callbacks))
                    save(args.output, report)
        if inputs(args, source, library) != report["inputs"]:
            raise AssertionError("benchmark input files changed during worker")
        report["status"] = "pass"
    finally:
        if native is not None:
            native.close()
            report["native_closed"] = not (native.session or native.model or native.registry or native._dll_paths)
            if not report["native_closed"]:
                raise AssertionError("native handles remain after close")


def controller(args, report):
    order = args.order + args.order.translate(str.maketrans("AB", "BA"))
    report.update(order=order, inputs={})
    for index, variant in enumerate(order):
        path = args.output.with_name(args.output.stem + f"-{index}-{variant}.json")
        for target in (path, path.with_suffix(".log"), path.with_suffix(".json.partial")):
            if target.exists():
                raise FileExistsError(target)
        command = [sys.executable, str(Path(__file__).resolve()), "--worker", "--variant", variant,
                   "--baseline-source", str(args.baseline_source), "--candidate-source", str(args.candidate_source),
                   "--baseline-library", str(args.baseline_library), "--candidate-library", str(args.candidate_library),
                   "--model", str(args.model), "--output", str(path), "--repeats", str(args.repeats),
                   "--zh-wav", str(args.zh_wav), "--en-wav", str(args.en_wav), "--scope", args.scope,
                   "--vad-model", str(args.vad_model), "--vad-cmvn", str(args.vad_cmvn)]
        for directory in args.dependencies:
            command.extend(["--dependencies", str(directory)])
        block = {"variant": variant, "round": index // 4, "output": str(path), "command": command,
                 "log": str(path.with_suffix(".log")), "timed_out": False, "returncode": None}
        report["workers"].append(block)
        save(args.output, report)
        print(f"BLOCK {index + 1}/{len(order)} {variant} output={path}", flush=True)
        env = os.environ | {"PYTHONIOENCODING": "utf-8", "PYTHONDONTWRITEBYTECODE": "1", "OMP_NUM_THREADS": "1"}
        env["PATH"] = os.pathsep.join(str(p.resolve()) for p in args.dependencies) + os.pathsep + env.get("PATH", "")
        try:
            # run() kills and waits for a timed-out child; each worker owns one model.
            with path.with_suffix(".log").open("x", encoding="utf-8") as log:
                completed = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=args.timeout,
                    cwd=ROOT, env=env, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)
                    | getattr(subprocess, "BELOW_NORMAL_PRIORITY_CLASS", 0))
            block["returncode"] = completed.returncode
        except subprocess.TimeoutExpired:
            block["timed_out"] = True
            raise
        finally:
            block["result"] = json.loads(path.read_text(encoding="utf-8")) if path.exists() else None
            save(args.output, report)
        child = block["result"]
        if block["returncode"] or not child or child.get("status") != "pass" or child.get("native_closed") is not True:
            raise RuntimeError(f"worker failed: {path}")
        expected = report["inputs"].setdefault(variant, child["inputs"])
        if expected != child["inputs"]:
            raise AssertionError(f"{variant}: benchmark inputs changed between workers")
    baseline_files, candidate_files = (report["inputs"][v]["files"] for v in "AB")
    if any(baseline_files[p] != candidate_files[p] for p in baseline_files.keys() & candidate_files.keys()):
        raise AssertionError("shared benchmark input identity differs across A/B")
    rows = [(block["variant"], row) for block in report["workers"] for row in block["result"]["rows"]]
    report["summary"], parity = {}, {}
    for fixture in ("zh", "en", "mixed-derived"):
        for scenario in ("paced", "backlog"):
            key = fixture + "/" + scenario
            selected = [(v, r) for v, r in rows if r["fixture"] == fixture and r["scenario"] == scenario]
            parity[key] = all(len({json.dumps(r[field], sort_keys=True, ensure_ascii=False)
                                  for _, r in selected}) == 1 for field in ("final", "accepted_pcm"))
            report["summary"][key] = {v: {metric: {
                "p50": float(np.percentile([r[metric] for x, r in selected if x == v], 50)),
                "p95": float(np.percentile([r[metric] for x, r in selected if x == v], 95))}
                for metric in ("stop_to_final_ms", "decode_count", "processed_audio_samples")}
                for v in "AB"}
            for variant in "AB":
                captions = [r["first_nonempty_caption"] for v, r in selected
                            if v == variant and r["first_nonempty_caption"] is not None]
                report["summary"][key][variant]["first_caption"] = {metric: {
                    "p50": float(np.percentile([r[metric] for r in captions], 50)),
                    "p95": float(np.percentile([r[metric] for r in captions], 95))}
                    for metric in ("wall_ms", "audio_samples")} if captions else None
    report.update(exact_final_parity=parity, status="pass" if all(parity.values()) else "failed")


def self_test():
    class Clock:
        value = 0.
        def now(self):
            return self.value
        def sleep(self, seconds):
            self.value += seconds
    class Fake:
        preview_text = ""
        def start(self):
            self.preview_text = ""
        def feed(self, pcm, preview=True):
            if preview:
                clock.value += .151
                self.preview_text = "caption"
        def finish(self):
            clock.value += .1
            return "final", "Chinese"
    for variant in "AB":
        for scenario in ("paced", "backlog"):
            clock = Clock()
            pcm = np.arange(16003, dtype=np.float32)
            row = replay(Fake(), pcm, variant, scenario, clock.now, clock.sleep)
            assert row["accepted_pcm"] == pcm_info(pcm)
            assert row["feeds"][-1]["samples"] > 0
            assert row["stop_to_final_ms"] >= 100 - 1e-6
            if scenario == "backlog":
                assert row["first_nonempty_caption"] is None
                assert len(row["feeds"]) == (26 if variant == "A" else 4)
                if variant == "B":
                    assert not any(f["preview"] for f in row["feeds"])
            else:
                assert row["first_nonempty_caption"] is not None
            assert all(f["samples"] <= (640 if variant == "A" else 5120) for f in row["feeds"])
            live = [f for f in row["feeds"] if f["phase"] == "live"]
            for feed in live:
                available = min(len(pcm), int((feed["start_wall_ms"] + 1e-6) / 40) * 640)
                assert feed["offset"] + feed["samples"] <= available
    class SlowPacket(Fake):
        def feed(self, pcm, preview=True):
            clock.value += .5 + (.11 if preview else 0)
            if preview:
                self.preview_text = "caption"
    clock = Clock()
    row = replay(SlowPacket(), pcm, "B", "paced", clock.now, clock.sleep)
    assert [f["preview"] for f in row["feeds"]] == [True, False, True, False, False]
    assert row["feeds"][1]["batch"] == row["feeds"][2]["batch"]
    assert row["feeds"][2]["start_wall_ms"] > row["stop_requested_wall_ms"]
    assert abs(row["stop_observed_ms"] - (1760 - len(pcm) / 16)) < 1e-6
    assert abs(row["stop_to_final_ms"] - (2860 - len(pcm) / 16)) < 1e-6
    assert abs(row["first_nonempty_caption"]["wall_ms"] - 650) < 1e-6
    print("self-test PASS: paced/backlog A/B; exact PCM/tail; arrival bounds; in-flight sink batch; Stop wall latency")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--self-test", action="store_true")
    p.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    p.add_argument("--variant", choices=("A", "B"), default="B")
    p.add_argument("--baseline-source", type=Path, default=ROOT / "tmp/asr-opt-20261025/baseline/python")
    p.add_argument("--candidate-source", type=Path, default=ROOT / "src-tauri/resources")
    p.add_argument("--model", type=Path)
    p.add_argument("--candidate-library", "--library", dest="candidate_library", type=Path,
                   default=ROOT / "src-tauri/resources/r2t2-native/cuda/audiocpp.dll")
    p.add_argument("--baseline-library", type=Path, default=ROOT / "tmp/asr-opt-20261025/baseline/cuda/audiocpp.dll")
    p.add_argument("--dependencies", type=Path, action="append", default=[])
    p.add_argument("--zh-wav", type=Path, default=DEFAULT_ZH)
    p.add_argument("--en-wav", type=Path, default=DEFAULT_EN)
    p.add_argument("--scope", choices=("product-vad", "native-wrapper"), default="product-vad")
    p.add_argument("--vad-model", type=Path, default=ROOT / "src-tauri/resources/fireredvad_vad.onnx")
    p.add_argument("--vad-cmvn", type=Path, default=ROOT / "src-tauri/resources/fireredvad_cmvn.json")
    p.add_argument("--repeats", type=int, default=3)
    p.add_argument("--order", choices=("ABBA", "BAAB"), default="ABBA")
    p.add_argument("--timeout", type=float, default=3600)
    p.add_argument("--output", type=Path)
    args = p.parse_args()
    if args.self_test:
        self_test()
        return
    if args.repeats < 3 or not all((args.model, args.output)) or not np.isfinite(args.timeout) or args.timeout <= 0:
        p.error("--model, --output, --repeats >= 3 and finite positive --timeout required")
    if not args.dependencies:
        args.dependencies = [ROOT / "src-tauri/resources/r2t2-native/cuda"]
    for name in ("model", "baseline_source", "candidate_source", "baseline_library", "candidate_library",
                 "zh_wav", "en_wav", "vad_model", "vad_cmvn", "output"):
        setattr(args, name, getattr(args, name).resolve())
    args.dependencies = [p.resolve() for p in args.dependencies]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as initial:
        initial.write("{}")
    report = {"schema": "qwen-backlog-v1", "status": "running", "scope": args.scope,
              "created_utc": datetime.now(timezone.utc).isoformat(), "pid": os.getpid(),
              "config": {"sample_rate": 16000, "capture_samples": 640, "candidate_packet_samples": 5120,
                         "repeats": args.repeats, "backend": "cuda", "threads": 1,
                         "warmup": "one complete-clip callback per case; excluded from measured rows",
                         "mixed_recipe": "concatenate zh then en without inserted silence; derived, not a code-switch recording"},
              "variant": args.variant if args.worker else None, "rows": [], "workers": [],
              "limitations": "Python replay of Rust scheduling; no microphone/IPC; mixed fixture is concatenated; exact parity is not accuracy"}
    try:
        (worker if args.worker else controller)(args, report)
    except BaseException as error:
        report.update(status="failed", error=f"{type(error).__name__}: {error}")
        raise
    finally:
        report["completed_utc"] = datetime.now(timezone.utc).isoformat()
        save(args.output, report)
    print(f"{report['status'].upper()} output={args.output}", flush=True)
    if report["status"] != "pass":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
