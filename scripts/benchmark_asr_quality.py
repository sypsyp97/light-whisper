"""Six fixed natural-speech quality observations, one resident CUDA model per process.

Run baseline and candidate separately with --family, --model, --library,
--dependencies, --source-dir and --output; add --reference BASELINE_JSON to the
candidate. Qwen uses the production full-clip FireRedVAD callback. R2T2 uses the
production rolling 160 ms stream and retains its 320 ms finish context. The last
feed contains exactly 13 original samples: no trimming, padding or resampling.
--self-test needs only the standard library and never imports native wrappers.
"""
import argparse
from contextlib import nullcontext
import ctypes as ct
from datetime import datetime, timezone
import hashlib
import importlib
import io
import json
import os
from pathlib import Path
import sys
import time
import traceback
from types import SimpleNamespace
import unicodedata
import wave

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tmp/asr-opt-20261025/natural-quality/fixtures.json"
RATE, CHUNK, TAIL, FINISH = 16000, 2560, 13, 5120
NORMALIZATION = "Unicode NFKC, lowercase, retain Unicode alphanumeric characters; no number-word conversion"


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def sha256(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def normalize(text):
    return "".join(char for char in unicodedata.normalize("NFKC", text).lower() if char.isalnum())


def edit_distance(left, right):
    if len(left) < len(right):
        left, right = right, left
    previous = list(range(len(right) + 1))
    for index, char in enumerate(left, 1):
        current = [index]
        for other, candidate in enumerate(right, 1):
            current.append(min(current[-1] + 1, previous[other] + 1,
                               previous[other - 1] + (char != candidate)))
        previous = current
    return previous[-1]


def accuracy(reference, hypothesis):
    expected, observed = normalize(reference), normalize(hypothesis)
    distance = edit_distance(expected, observed)
    return {"reference_normalized": expected, "hypothesis_normalized": observed,
            "edits": distance, "reference_chars": len(expected),
            "normalized_char_edit_distance": distance / len(expected) if expected else None}


def read_pcm(source):
    with wave.open(source if hasattr(source, "read") else str(source), "rb") as wav:
        params = wav.getparams()
        require((params.nchannels, params.sampwidth, params.framerate, params.comptype)
                == (1, 2, RATE, "NONE"), f"expected mono 16 kHz PCM16 WAV, got {params}")
        raw = wav.readframes(params.nframes)
    require(len(raw) == params.nframes * 2 and params.nframes >= TAIL,
            "incomplete or too-short PCM16 WAV")
    return raw


def packets(raw):
    count = len(raw) // 2
    require(len(raw) % 2 == 0 and count >= TAIL, "PCM16 requires complete samples and a 13-sample tail")
    tail = count - TAIL
    for offset in range(0, tail, CHUNK):
        yield offset, raw[offset * 2:min(offset + CHUNK, tail) * 2]
    yield tail, raw[tail * 2:]


def account_pcm(raw, accepted):
    offset, chunks = 0, []
    for supplied_offset, chunk in accepted:
        require(supplied_offset == offset and len(chunk) % 2 == 0 and 0 < len(chunk) <= CHUNK * 2,
                "feed offset/size changed captured PCM accounting")
        offset += len(chunk) // 2
        chunks.append(chunk)
    require(chunks and len(chunks[-1]) == TAIL * 2, "13-sample final feed was dropped or changed")
    observed = b"".join(chunks)
    require(observed == raw, "accepted PCM changed, duplicated or dropped original samples")
    return {"samples": offset, "pcm16_sha256": hashlib.sha256(observed).hexdigest(),
            "feed_count": len(chunks), "last_feed_samples": TAIL}


def save(path, report):
    temporary = path.with_suffix(path.suffix + ".partial")
    temporary.write_text(json.dumps(report, ensure_ascii=False, allow_nan=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def load_fixtures(manifest):
    data = json.loads(manifest.read_text(encoding="utf-8"))
    require(data["schema"] == "asr-quality-fixtures-v1", "unknown fixture manifest schema")
    expected = ["ascend-test-00009", "ascend-test-00066", "ascend-test-00068",
                "ascend-test-00081", "fleurs-en-numbers", "chinese-tail"]
    require([item["id"] for item in data["fixtures"]] == expected, "fixed six-fixture selection changed")
    metadata = data["original_metadata"]
    require(sha256(ROOT / metadata["path"]) == metadata["sha256"], "original ASCEND metadata changed")
    clips = []
    for item in data["fixtures"]:
        path = ROOT / item["path"]
        raw = read_pcm(path)
        require(sha256(path) == item["wav"]["file_sha256"]
                and hashlib.sha256(raw).hexdigest() == item["wav"]["pcm16_sha256"]
                and len(raw) // 2 == item["wav"]["samples"], f"{item['id']}: fixture identity changed")
        require(bool(normalize(item["reference"])), f"{item['id']}: empty human reference")
        clips.append((item, raw))
    return data, clips


def compare_reference(report, baseline):
    require(baseline.get("schema") == report["schema"] == "asr-quality-v1", "reference report schema differs")
    require(baseline.get("status") == "completed", "reference did not complete successfully")
    require(report["config"] == baseline["config"], "reference family/recognition policy differs")
    for key in ("model", "manifest", "vad_model", "vad_cmvn", "dependency_identity"):
        require(report["inputs"].get(key) == baseline["inputs"].get(key), f"reference {key} identity differs")
    require([r["id"] for r in report["rows"]] == [r["id"] for r in baseline["rows"]], "reference case selection differs")
    cases = []
    for row, old in zip(report["rows"], baseline["rows"]):
        cases.append({"id": row["id"], "final_exact": row["final"] == old["final"],
                      "pcm_exact": row["accepted_pcm"] == old["accepted_pcm"],
                      "sample_accounting_exact": row["sample_accounting"] == old["sample_accounting"]})
    return {"status": "match" if all(all(case[key] for key in ("final_exact", "pcm_exact", "sample_accounting_exact"))
                                     for case in cases) else "regression", "cases": cases,
            "meaning": "Exact final text, language and sample accounting; independent of human-reference accuracy."}


def run(args, report):
    native = session = server = None
    directories, preloaded = [], []
    try:
        fixture_data, clips = load_fixtures(args.manifest)
        require(os.name == "nt", "native quality run requires Windows")
        source_files = {p.name: sha256(p) for p in sorted(args.source_dir.glob("*.py"))}
        dependencies, dependency_identity = {}, {}
        for directory in args.dependencies:
            require(directory.is_dir(), f"missing dependency directory: {directory}")
            for path in sorted(directory.glob("*.dll")):
                if path.name.lower() == "audiocpp.dll":
                    continue  # Target DLL identity is recorded separately.
                digest = sha256(path)
                require(path.name not in dependency_identity or dependency_identity[path.name] == digest,
                        f"ambiguous dependency identity: {path.name}")
                dependencies[str(path)] = digest
                dependency_identity[path.name] = digest
        report["inputs"] = {"model": sha256(args.model), "model_path": str(args.model),
            "library": sha256(args.library), "library_path": str(args.library),
            "manifest": sha256(args.manifest), "manifest_path": str(args.manifest),
            "source_dir": str(args.source_dir), "source_sha256": source_files,
            "dependencies": dependencies, "dependency_identity": dependency_identity,
            "script_sha256": sha256(__file__)}
        report["fixtures"] = fixture_data
        if args.family == "qwen":
            report["inputs"].update(vad_model=sha256(args.vad_model), vad_cmvn=sha256(args.vad_cmvn))
        save(args.output, report)
        for directory in dict.fromkeys([args.library.parent, *args.dependencies]):
            directories.append(os.add_dll_directory(str(directory)))
        for name in ("cublasLt64_12.dll", "cublas64_12.dll", "cudart64_12.dll"):
            path = next((directory / name for directory in args.dependencies if (directory / name).is_file()), None)
            require(path is not None, f"missing explicit dependency: {name}")
            preloaded.append(ct.WinDLL(str(path)))
        os.environ["LIGHT_WHISPER_DATA_DIR"] = str(args.output.with_suffix(""))
        os.environ["OMP_NUM_THREADS"] = "1"
        sys.path.insert(0, str(args.source_dir))
        import numpy as np
        names = ["r2t2_native", "qwen3_native", "r2t2_stream"]
        if args.family == "qwen":
            names += ["qwen3_asr_server", "firered_vad", "asr_stream_server", "server_common", "hf_cache_utils"]
        modules = {name: importlib.import_module(name) for name in names}
        report["modules"] = {name: str(Path(module.__file__).resolve()) for name, module in modules.items()}
        for name, path in report["modules"].items():
            require(Path(path) == (args.source_dir / (name + ".py")).resolve(), f"wrong imported module: {name}={path}")
        before = time.perf_counter()
        options = {"backend": "cuda", "threads": 1, "dll_directories": args.dependencies}
        native = (modules["qwen3_native"].QwenNativeRuntime(args.model, args.library, **options)
                  if args.family == "qwen" else modules["r2t2_native"].NativeRuntime(
                      args.model, args.library, rolling=True, chunk_ms=160, **options))
        report.update(model_load_ms=(time.perf_counter() - before) * 1000,
                      abi_version=native.functions["abi_version"]())
        handle = native.session.value
        current = None
        if args.family == "qwen":
            server_type = modules["qwen3_asr_server"].Qwen3ASRServer
            server = server_type.__new__(server_type)
            server.session = native
            server.vad_model = modules["firered_vad"].FireRedVad(args.vad_model, args.vad_cmvn)
            server.stdout_suppressor = SimpleNamespace(suppress=nullcontext)
            server._vad_calls = server._vad_rejected = 0
            server._total_vad_ms = 0
            native_run, timestamps = native.run, server.vad_model.speech_timestamps

            def decode(pcm, **kwargs):
                event = {"samples": len(pcm), "pcm32_sha256": hashlib.sha256(pcm.tobytes()).hexdigest(), "status": "running"}
                current["native_decodes"].append(event)
                result = native_run(pcm, **kwargs)
                event["status"] = "completed"
                return result

            def vad(pcm):
                event = {"samples": len(pcm), "status": "running"}
                current["vad_calls"].append(event)
                segments = timestamps(pcm)
                event.update(status="completed", segments=segments)
                return segments

            native.run, server.vad_model.speech_timestamps = decode, vad
        else:
            require(native.FINISH_CONTEXT_SAMPLES == FINISH and native.chunk_samples == CHUNK,
                    "R2T2 160 ms chunks / 320 ms finish context changed")
            session = modules["r2t2_stream"].R2T2StreamSession(native)
        for serial, (fixture, raw) in enumerate(clips, 1):
            current = {"id": fixture["id"], "reference": fixture["reference"], "status": "running",
                       "feeds": [], "native_decodes": [], "vad_calls": []}
            report["rows"].append(current)
            save(args.output, report)
            before = time.perf_counter()
            try:
                require(native.session.value == handle and handle, "resident native session changed")
                accepted = []
                if session is not None:
                    current["start"] = session.start(serial)
                for offset, chunk in packets(raw):
                    feed = {"offset": offset, "samples": len(chunk) // 2}
                    current["feeds"].append(feed)
                    if session is not None:
                        pcm = np.frombuffer(chunk, dtype="<i2").astype(np.float32) / np.float32(32768)
                        feed["response"] = session.feed(serial, pcm, offset=offset)
                        feed["native_samples"] = native._offset
                        require(feed["response"]["sample_count"] == offset + len(chunk) // 2
                                and native._offset == offset + len(chunk) // 2, "R2T2 feed dropped samples")
                    accepted.append((offset, chunk))
                current["accepted_pcm"] = account_pcm(raw, accepted)
                count = current["accepted_pcm"]["samples"]
                finish_started = time.perf_counter()
                if session is not None:
                    final = session.finish(serial)
                    current["finish_response"] = final
                    require(final["sample_count"] == count and native._offset == count + FINISH,
                            "R2T2 finish lost captured samples or 320 ms context")
                    require(not session.active and not native._active and native.preview_text == "",
                            "R2T2 finish retained active state")
                else:
                    # Qwen's full-clip callback has no native sample-count result.
                    # Assemble every original feed byte before the production VAD.
                    pcm = np.frombuffer(b"".join(chunk for _, chunk in accepted), dtype="<i2").astype(np.float32) / np.float32(32768)
                    result = server._transcribe_stream_audio(pcm)
                    final = {"text": result.text, "language": result.language, "sample_count": count}
                current.update(final={key: final[key] for key in ("text", "language", "sample_count")},
                    sample_accounting={"captured_samples": count, "final_samples": final["sample_count"],
                        "native_final_samples": native._offset if session is not None else None,
                        "finish_context_samples": FINISH if session is not None else 0,
                        "count_origin": "R2T2StreamSession result" if session is not None else "captured byte accounting; Qwen callback returns no sample count"},
                    accuracy=accuracy(fixture["reference"], final["text"]), status="completed",
                    finish_ms=(time.perf_counter() - finish_started) * 1000,
                    total_ms=(time.perf_counter() - before) * 1000)
                print(f"CASE {fixture['id']} samples={count} edits={current['accuracy']['edits']}/{current['accuracy']['reference_chars']}", flush=True)
            except BaseException as error:
                current.update(status="failed", error=f"{type(error).__name__}: {error}")
                raise
            finally:
                save(args.output, report)
        require(source_files == {p.name: sha256(p) for p in sorted(args.source_dir.glob("*.py"))},
                "source files changed during quality run")
        load_fixtures(args.manifest)  # Recheck original PCM and references after all calls.
    finally:
        for owner in (session, native):
            if owner is not None:
                try:
                    owner.close()
                except BaseException as error:
                    report["cleanup_errors"].append(f"{type(owner).__name__}.close: {type(error).__name__}: {error}")
        report["native_closed"] = native is None or not any((native.session, native.model, native.registry, native._dll_paths))
        for directory in reversed(directories):
            try:
                directory.close()
            except BaseException as error:
                report["cleanup_errors"].append(f"DLL directory close: {type(error).__name__}: {error}")
        server = session = native = None


def self_test():
    require(normalize("ＡＢＣ—二十五，25 / ３０") == "abc二十五2530", "NFKC/lowercase/alphanumeric normalization")
    require(edit_distance("kitten", "sitting") == 3 and edit_distance("", "尾") == 1, "character edit distance")
    require(accuracy("学习Python", "学习Pytho")["edits"] == 1, "missing spoken tail must count")
    require(accuracy("25", "twenty five")["edits"] > 0, "number words must not be silently rewritten")
    require(accuracy("", "word")["normalized_char_edit_distance"] is None, "missing denominator is unavailable")
    for count in (13, 14, 2559, 2560, 2561, 2573, 5123, 16003):
        raw = b"".join((sample % 65536).to_bytes(2, "little") for sample in range(count))
        accepted = list(packets(raw))
        require(account_pcm(raw, accepted)["samples"] == count, "feed coverage")
        for changed in (accepted[:-1], accepted + accepted[-1:],
                        accepted[:-1] + [(accepted[-1][0], bytes(TAIL * 2))]):
            try:
                account_pcm(raw, changed)
            except AssertionError:
                pass
            else:
                raise AssertionError("PCM corruption/drop/duplicate negative control survived")
    for channels, rate, width in ((1, RATE, 2), (2, RATE, 2), (1, 8000, 2), (1, RATE, 1)):
        stream = io.BytesIO()
        with wave.open(stream, "wb") as wav:
            wav.setparams((channels, width, rate, 0, "NONE", "not compressed"))
            wav.writeframes(bytes(32 * channels * width))
        stream.seek(0)
        try:
            read_pcm(stream)
        except AssertionError:
            require((channels, rate, width) != (1, RATE, 2), "valid PCM fixture rejected")
        else:
            require((channels, rate, width) == (1, RATE, 2), "wrong WAV format negative control survived")
    baseline = {"schema": "asr-quality-v1", "status": "completed", "config": {"family": "qwen"},
                "inputs": {"model": "fixed-model", "manifest": "fixed-fixtures"},
                "rows": [{"id": "self-test", "final": {"text": "中文Python25", "language": "Chinese", "sample_count": 13},
                          "accepted_pcm": {"samples": 13}, "sample_accounting": {"final_samples": 13}}]}
    require(compare_reference(baseline, baseline)["status"] == "match", "identical final parity")
    for field, value in (("text", "中文Python"), ("language", "English"), ("sample_count", 12)):
        changed = json.loads(json.dumps(baseline))
        changed["rows"][0]["final"][field] = value
        require(compare_reference(changed, baseline)["status"] == "regression", f"changed final {field} escaped parity check")
    changed = json.loads(json.dumps(baseline))
    changed["inputs"]["manifest"] = "different-fixtures"
    try:
        compare_reference(changed, baseline)
    except AssertionError:
        pass
    else:
        raise AssertionError("reference fixture identity negative control survived")
    print("SELF-TEST PASS: normalization/edit distance; original PCM accounting; 13-sample tail; corrupt/drop/duplicate, format and parity guards; no native/model/GPU imports")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--family", choices=("qwen", "r2t2"))
    for name in ("model", "library", "source-dir", "output", "reference"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--dependencies", type=Path, action="append")
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    parser.add_argument("--vad-model", type=Path, default=ROOT / "src-tauri/resources/fireredvad_vad.onnx")
    parser.add_argument("--vad-cmvn", type=Path, default=ROOT / "src-tauri/resources/fireredvad_cmvn.json")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if not all((args.family, args.model, args.library, args.source_dir, args.output, args.dependencies)):
        parser.error("--family, --model, --library, --dependencies, --source-dir and --output are required")
    for name, value in vars(args).items():
        if isinstance(value, Path):
            setattr(args, name, value.resolve())
    args.dependencies = [path.resolve() for path in args.dependencies]
    if args.output.with_suffix(args.output.suffix + ".partial").exists():
        parser.error("partial output already exists; choose a new --output")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {"schema": "asr-quality-v1", "status": "running", "created_utc": datetime.now(timezone.utc).isoformat(),
              "pid": os.getpid(), "config": {"family": args.family, "backend": "cuda", "threads": 1,
                  "sample_rate": RATE, "feed_chunk_samples": CHUNK, "last_feed_samples": TAIL,
                  "r2t2_finish_context_samples": FINISH, "normalization": NORMALIZATION,
                  "qwen_mode": "one full-clip production VAD callback; no preview inference"},
              "rows": [], "cleanup_errors": [], "parity": None,
              "limitations": ["Six fixed clips are coverage observations, not a corpus accuracy estimate; four mixed clips share ASCEND speaker 17.",
                  "Character distance does not equate number words with digits or perform lexical tokenization; no WER is reported.",
                  "Qwen sample count is byte-accounted capture input; VAD may trim the separate native decode input.",
                  "Python file replay excludes microphone, Rust IPC and UI; model close does not unload the CUDA process context."]}
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(report, output)
    try:
        run(args, report)
        require(report["native_closed"] and not report["cleanup_errors"], "native cleanup failed; see cleanup_errors")
        edits = sum(row["accuracy"]["edits"] for row in report["rows"])
        chars = sum(row["accuracy"]["reference_chars"] for row in report["rows"])
        report.update(status="completed", accuracy_summary={"edits": edits, "reference_chars": chars,
                      "normalized_char_edit_distance": edits / chars})
        if args.reference:
            report["reference"] = {"path": str(args.reference), "sha256": sha256(args.reference)}
            report["parity"] = compare_reference(report, json.loads(args.reference.read_text(encoding="utf-8")))
            if report["parity"]["status"] != "match":
                report["status"] = "parity_regression"
    except BaseException as error:
        report.update(status="failed", error=f"{type(error).__name__}: {error}", traceback=traceback.format_exc())
    finally:
        report["completed_utc"] = datetime.now(timezone.utc).isoformat()
        save(args.output, report)
    print(f"{report['status'].upper()} output={args.output}", flush=True)
    if "error" in report:
        print(report["error"], file=sys.stderr)
    return 0 if report["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
