"""Serial native-ASR lifecycle probe; --self-test never loads a model or CUDA.

Run baseline/candidate DLLs in separate processes. Memory trends are observations,
not leak-free verdicts. CUDA queries current-context total and OS-reported device
free memory; total minus free is neither per-process VRAM nor summed process use,
and is not a physical NVML measurement. Other processes may race the query.
Model close leaves the CUDA context/dependency DLLs loaded. Probe reports also
consume Python memory. Default cycles: Qwen 8, R2T2 12; one resident model per load.
"""
import argparse
import ctypes as ct
from datetime import datetime, timezone
import gc
import hashlib
import json
import os
from pathlib import Path
import sys
import time
import tracemalloc

import numpy as np

sys.dont_write_bytecode = True
from test_r2t2_native_cache import (
    RESOURCES, DEFAULT_ZH, DEFAULT_EN, SAMPLE_RATE, CHUNK_SAMPLES,
    FINISH_SAMPLES, read_wav, require, sha256,
)


class _ProcessMemoryCounters(ct.Structure):
    _fields_ = [("cb", ct.c_ulong), ("PageFaultCount", ct.c_ulong)] + [
        (name, ct.c_size_t) for name in (
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
            "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
            "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage", "PrivateUsage")]


class _ThreadEntry32(ct.Structure):
    # Windows SDK TlHelp32.h: THREADENTRY32, DWORD/LONG fields.
    _fields_ = [("dwSize", ct.c_ulong), ("cntUsage", ct.c_ulong),
                ("th32ThreadID", ct.c_ulong), ("th32OwnerProcessID", ct.c_ulong),
                ("tpBasePri", ct.c_long), ("tpDeltaPri", ct.c_long), ("dwFlags", ct.c_ulong)]


_PROCESS_MEMORY_COUNTERS_POINTER = ct.POINTER(_ProcessMemoryCounters)
_THREAD_ENTRY32_POINTER = ct.POINTER(_ThreadEntry32)
_DWORD_POINTER = ct.POINTER(ct.c_ulong)


def process_memory():
    """Current-process memory/handles/threads; unavailable counters stay null."""
    result = {"private_bytes": None, "working_set_bytes": None,
              "handle_count": None, "thread_count": None, "error": None}
    kernel, errors = None, []
    try:
        if os.name != "nt":
            raise OSError("GetProcessMemoryInfo requires Windows")
        library = ct.WinDLL("kernel32", use_last_error=True)
        library.GetCurrentProcess.restype, library.GetCurrentProcess.argtypes = ct.c_void_p, []
        kernel = library
        query = ct.WinDLL("psapi", use_last_error=True).GetProcessMemoryInfo
        query.restype, query.argtypes = ct.c_int, [ct.c_void_p, _PROCESS_MEMORY_COUNTERS_POINTER, ct.c_ulong]
        counters = _ProcessMemoryCounters(cb=ct.sizeof(_ProcessMemoryCounters))
        if not query(kernel.GetCurrentProcess(), ct.byref(counters), counters.cb):
            raise ct.WinError(ct.get_last_error())
        result.update(private_bytes=counters.PrivateUsage, working_set_bytes=counters.WorkingSetSize)
    except Exception as error:
        errors.append(f"GetProcessMemoryInfo: {type(error).__name__}: {error}")
    if kernel is not None:
        try:
            query = kernel.GetProcessHandleCount
            query.restype, query.argtypes = ct.c_int, [ct.c_void_p, _DWORD_POINTER]
            count = ct.c_ulong()
            if not query(kernel.GetCurrentProcess(), ct.byref(count)):
                raise ct.WinError(ct.get_last_error())
            result["handle_count"] = count.value
        except Exception as error:
            errors.append(f"GetProcessHandleCount: {type(error).__name__}: {error}")
        try:
            create, first, next_thread, close = (getattr(kernel, name) for name in
                ("CreateToolhelp32Snapshot", "Thread32First", "Thread32Next", "CloseHandle"))
            create.restype, create.argtypes = ct.c_void_p, [ct.c_ulong, ct.c_ulong]
            for function in (first, next_thread):
                function.restype, function.argtypes = ct.c_int, [ct.c_void_p, _THREAD_ENTRY32_POINTER]
            close.restype, close.argtypes = ct.c_int, [ct.c_void_p]
            snapshot = create(0x00000004, 0)  # TH32CS_SNAPTHREAD, Windows SDK TlHelp32.h.
            if not snapshot or snapshot == ct.c_void_p(-1).value:
                raise ct.WinError(ct.get_last_error())
            try:
                entry = _ThreadEntry32(dwSize=ct.sizeof(_ThreadEntry32))
                if not first(snapshot, ct.byref(entry)):
                    raise ct.WinError(ct.get_last_error())
                count = 0
                while True:
                    count += entry.th32OwnerProcessID == os.getpid()
                    entry.dwSize = ct.sizeof(_ThreadEntry32)
                    if not next_thread(snapshot, ct.byref(entry)):
                        status = ct.get_last_error()
                        if status != 18:  # ERROR_NO_MORE_FILES.
                            raise ct.WinError(status)
                        break
            finally:
                if not close(snapshot):
                    raise ct.WinError(ct.get_last_error())
            result["thread_count"] = count
        except Exception as error:
            errors.append(f"thread snapshot: {type(error).__name__}: {error}")
    result["error"] = "; ".join(errors) or None
    return result


def cuda_memory(library, unavailable):
    result = {"scope": "DEVICE_CURRENT_CONTEXT_OS_REPORTED", "free_bytes": None, "total_bytes": None,
              "used_bytes": None, "synchronized": None, "error": unavailable}
    if library is not None:
        try:
            status = library.cudaDeviceSynchronize()
            result["synchronized"] = status == 0
            require(status == 0, f"cudaDeviceSynchronize status {status}")
            free, total = ct.c_size_t(), ct.c_size_t()
            status = library.cudaMemGetInfo(ct.byref(free), ct.byref(total))
            require(status == 0, f"cudaMemGetInfo status {status}")
            result.update(free_bytes=free.value, total_bytes=total.value,
                          used_bytes=total.value - free.value, error=None)
        except Exception as error:
            result["error"] = f"{type(error).__name__}: {error}"
    return result


def trend(values, *, unit="bytes"):
    valid = [(cycle, value) for cycle, value in values if value is not None]
    delta = slope = None
    if len(valid) >= 2:
        x, y = zip(*valid)
        xbar, ybar = sum(x) / len(x), sum(y) / len(y)
        denominator = sum((value - xbar) ** 2 for value in x)
        delta = y[-1] - y[0]
        if denominator:
            slope = sum((a - xbar) * (b - ybar) for a, b in valid) / denominator
    return {"available_points": len(valid), f"delta_{unit}": delta, f"slope_{unit}_per_cycle": slope}


def summarize(points):
    result = {}
    for load in sorted({point["load"] for point in points if point["phase"] == "cycle_complete"}):
        completed = [p for p in points if p["phase"] == "cycle_complete" and p["load"] == load]
        late = completed[len(completed) // 2:]
        result[str(load)] = {"late_cycles": [p["cycle"] for p in late]}
        for group, field in (("process", "private_bytes"), ("process", "working_set_bytes"),
                             ("process", "handle_count"), ("process", "thread_count"),
                             ("cuda_global", "used_bytes"), ("python_traced", "current_bytes")):
            result[str(load)][group + "." + field] = trend(
                [(p["cycle"], (p[group] or {}).get(field)) for p in late],
                unit="count" if field.endswith("_count") else "bytes")
    return result


def save(path, report):
    temporary = path.with_suffix(path.suffix + ".partial")
    temporary.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def state(runtime, session):
    result = {"wrapper_active": session.active, "wrapper_samples": session._sample_count,
              "runtime_active": runtime._active, "preview_empty": runtime.preview_text == ""}
    if hasattr(runtime, "_chunks"):
        result.update(retained_chunks=len(runtime._chunks), retained_samples=runtime._sample_count,
                      window_samples=len(runtime._window), new_samples=runtime._new_samples)
    else:
        result["native_samples"] = runtime._offset
    return result


def check_clear(observed, *, reset=False):
    require(not observed["wrapper_active"] and not observed["runtime_active"]
            and observed["wrapper_samples"] == 0 and observed["preview_empty"],
            f"completed stream retained state: {observed}")
    if "retained_chunks" in observed:
        require(all(observed[key] == 0 for key in (
            "retained_chunks", "retained_samples", "window_samples", "new_samples")),
            f"Qwen retained captured audio: {observed}")
    elif reset:
        require(observed["native_samples"] == 0, f"R2T2 reset retained audio: {observed}")


def probe(args, report):
    cuda, cuda_error, directories, preloaded = None, "not queried before CUDA initialization", [], []
    started, expected, serial = time.perf_counter(), {}, 0

    def point(phase, load=None, cycle=None, observed=None):
        gpu = cuda_memory(cuda, cuda_error)
        traced = tracemalloc.get_traced_memory() if args.tracemalloc else None
        report["points"].append({"phase": phase, "load": load, "cycle": cycle,
            "elapsed_s": time.perf_counter() - started, "process": process_memory(),
            "cuda_global": gpu, "python_traced": (
                {"current_bytes": traced[0], "peak_bytes": traced[1]} if traced else None), "state": observed})
        save(args.output, report)

    if args.tracemalloc:
        tracemalloc.start()
    try:
        point("cold_process")
        require(os.name == "nt", "native resource probe requires Windows")
        paths = [args.model, args.library, args.zh_wav, args.en_wav]
        require(all(path.is_file() for path in paths), "model, library and both WAV files must exist")
        clips = {"zh-short": read_wav(args.zh_wav)[:5001], "zh-full": read_wav(args.zh_wav),
                 "en-full": read_wav(args.en_wav)}
        if args.long_seconds:
            count = args.long_seconds * SAMPLE_RATE
            clips["zh-grown"] = np.tile(clips["zh-full"], (count + len(clips["zh-full"]) - 1)
                                        // len(clips["zh-full"]))[:count]
        pcm_info = {name: {"samples": len(pcm), "sha256": hashlib.sha256(pcm.tobytes()).hexdigest()}
                    for name, pcm in clips.items()}
        report["provenance"] = {"files": {str(path.resolve()): sha256(path) for path in paths},
            "wrappers": {name: sha256(RESOURCES / name) for name in
                         ("qwen3_native.py", "r2t2_native.py", "r2t2_stream.py")},
            "script_sha256": sha256(__file__), "fixture_helper_sha256": sha256(
                Path(__file__).with_name("test_r2t2_native_cache.py")), "pcm": pcm_info,
            "derived_audio": {"zh-short": "first 5001 samples of zh WAV",
                "zh-grown": f"repeat zh WAV then truncate to {args.long_seconds} seconds"}
                if args.long_seconds else {"zh-short": "first 5001 samples of zh WAV"},
            "dependencies": {str(p.resolve()): sha256(p) for directory in args.dependencies
                             for p in sorted(directory.glob("*.dll"))}}
        for directory in args.dependencies:
            directories.append(os.add_dll_directory(str(directory.resolve())))
        report["preloaded_dependencies"] = []
        for name in ("cublasLt64_12.dll", "cublas64_12.dll", "cudart64_12.dll"):
            path = next((directory / name for directory in args.dependencies if (directory / name).is_file()), None)
            require(path is not None, f"missing explicit dependency: {name}")
            preloaded.append(ct.WinDLL(str(path.resolve())))
            report["preloaded_dependencies"].append(str(path.resolve()))
        loaded_cudart = path.resolve()
        cuda_error = "CUDA metrics not requested; supply --cudart with an explicit cudart64*.dll"
        if args.cudart:
            try:
                require(args.cudart.name.lower().startswith("cudart64") and args.cudart.suffix.lower() == ".dll",
                        "--cudart must identify cudart64*.dll")
                require(args.cudart.resolve() == loaded_cudart, "--cudart must match the preloaded native dependency")
                report["provenance"]["cudart"] = {"path": str(args.cudart.resolve()), "sha256": sha256(args.cudart)}
                cuda = ct.WinDLL(str(args.cudart.resolve()))
                cuda.cudaDeviceSynchronize.restype, cuda.cudaDeviceSynchronize.argtypes = ct.c_int, []
                cuda.cudaMemGetInfo.restype = ct.c_int
                cuda.cudaMemGetInfo.argtypes = [ct.POINTER(ct.c_size_t), ct.POINTER(ct.c_size_t)]
            except Exception as error:
                cuda, cuda_error = None, f"{type(error).__name__}: {error}"
        sys.path.insert(0, str(RESOURCES))
        from qwen3_native import QwenNativeRuntime, QwenPreviewRuntime
        from r2t2_native import NativeRuntime
        from r2t2_stream import R2T2StreamSession
        require(NativeRuntime.FINISH_CONTEXT_SAMPLES == FINISH_SAMPLES, "320 ms finish padding changed")
        for load in range(args.model_loads):
            native = runtime = session = None
            try:
                point("before_load", load)
                options = {"dll_directories": args.dependencies, "threads": args.threads}
                native = (QwenNativeRuntime(args.model, args.library, **options) if args.family == "qwen"
                          else NativeRuntime(args.model, args.library, rolling=True, chunk_ms=160, **options))
                runtime = QwenPreviewRuntime(native.run) if args.family == "qwen" else native
                session = R2T2StreamSession(runtime)
                point("after_load", load)

                def begin():
                    nonlocal serial
                    serial += 1
                    session.start(serial)
                    require(session._sample_count == 0, "start retained captured count")
                    if args.family == "qwen":
                        require(not runtime._chunks and runtime._sample_count == 0, "start retained Qwen audio")

                def feed(pcm, regular=False):
                    for offset in range(0, len(pcm), CHUNK_SAMPLES):
                        session.feed(serial, pcm[offset:offset + CHUNK_SAMPLES], offset=offset,
                                     preview=args.family != "qwen" or regular)

                def complete(name, cycle=None, regular=False):
                    begin()  # An active predecessor is reset by the production wrapper.
                    point("after_start", load, cycle, state(runtime, session))
                    feed(clips[name], regular)
                    point("before_finish", load, cycle, state(runtime, session))
                    final = session.finish(serial)
                    observed = state(runtime, session)
                    mode = "regular" if args.family == "qwen" and regular else "stop_append" if args.family == "qwen" else "native"
                    key = (pcm_info[name]["sha256"], mode)
                    observable = {key: final[key] for key in ("text", "language", "sample_count")}
                    matches = expected[key] == observable if key in expected else None
                    report["rows"].append({"load": load, "cycle": cycle, "case": name, "mode": mode,
                        "pcm": pcm_info[name], "final": observable, "matches_previous": matches})
                    point("cycle_complete" if cycle is not None else "warmup_complete", load, cycle, observed)
                    check_clear(observed)
                    require(final["sample_count"] == len(clips[name]), "finish changed captured sample count")
                    if args.family == "r2t2":
                        require(native._offset == len(clips[name]) + FINISH_SAMPLES, "finish padding changed")
                    require(matches is not False, f"same-input transcript changed: load={load} cycle={cycle} {name}")
                    expected.setdefault(key, observable)

                complete("zh-short", regular=True)
                complete("zh-full")
                point("after_warmup", load)
                for cycle in range(args.cycles):
                    begin()
                    feed(clips["zh-full"][:5123])
                    point("active_before_cancel" if cycle % 2 == 0 else "active_before_restart", load, cycle, state(runtime, session))
                    if cycle % 2 == 0:
                        session.cancel(serial)
                        observed = state(runtime, session)
                        point("after_cancel", load, cycle, observed)
                        check_clear(observed, reset=True)
                    complete(list(clips)[cycle % len(clips)], cycle)
                    print(f"LOAD {load} CYCLE {cycle} complete", flush=True)
            finally:
                for obj, method in ((session, "close"), (runtime, "reset"), (native, "close")):
                    if obj is not None:
                        try:
                            getattr(obj, method)()
                        except Exception as error:
                            report["cleanup_errors"].append(f"{method}: {type(error).__name__}: {error}")
                handles_clear = native is None or not any((native.session, native.model, native.registry, native._dll_paths))
                point("after_model_close", load, observed={"native_handles_cleared": handles_clear})
                if not handles_clear:
                    report["cleanup_errors"].append("native handles remain after close")
                obj = session = runtime = native = None
                gc.collect()
                point("after_gc", load)
    finally:
        for directory in reversed(directories):
            directory.close()
        if args.tracemalloc:
            tracemalloc.stop()


def self_test():
    require(trend([(0, 100), (2, 120), (4, 140)]) == {
        "available_points": 3, "delta_bytes": 40, "slope_bytes_per_cycle": 10.0}, "trend math")
    counts = summarize([{"phase": "cycle_complete", "load": 0, "cycle": cycle,
        "process": {"handle_count": 10 + cycle, "thread_count": None},
        "cuda_global": {}, "python_traced": None} for cycle in range(4)])["0"]
    require(counts["process.handle_count"] == {
        "available_points": 2, "delta_count": 1, "slope_count_per_cycle": 1.0}
        and counts["process.thread_count"]["delta_count"] is None, "count summaries/unavailable metrics")
    for values in ([], [(0, None)], [(0, 9), (1, None)]):
        require(trend(values)["slope_bytes_per_cycle"] is None and trend(values)["delta_bytes"] is None,
                "missing/insufficient metrics must remain unavailable")
    missing = cuda_memory(None, "test unavailable")
    require(all(missing[k] is None for k in ("free_bytes", "total_bytes", "used_bytes", "synchronized"))
            and missing["error"] == "test unavailable", "unavailable CUDA counters")
    class FailedSync:
        def cudaDeviceSynchronize(self):
            return 46
    failed = cuda_memory(FailedSync(), None)
    require(failed["free_bytes"] is None and failed["used_bytes"] is None
            and failed["synchronized"] is False and "46" in failed["error"], "failed CUDA query")
    if os.name == "nt":
        def structure_types():
            return {item for item in gc.get_objects() if isinstance(item, type)
                    and issubclass(item, ct.Structure) and item.__module__ == __name__}
        warm = process_memory()  # Counter values may be unavailable.
        require(all(key in warm for key in ("handle_count", "thread_count")),
                "process sampler omitted handle_count/thread_count")
        gc.collect()
        pointers_before, types_before = set(ct._pointer_type_cache), structure_types()
        for _ in range(5):
            process_memory()
        gc.collect()
        require(not (set(ct._pointer_type_cache) - pointers_before)
                and not (structure_types() - types_before),
                "five warmed samples retained fresh ctypes pointer/structure types")
        after = process_memory()
        delta = (after["handle_count"] - warm["handle_count"]
                 if after["handle_count"] is not None and warm["handle_count"] is not None else None)
        require(delta is None or delta == 0, f"sampler retained OS handles after five warmed calls: {delta}")
        print(f"WINDOWS COUNTER REUSE PASS: five warmed samples and GC; handle_delta={delta}")
    print("SELF-TEST PASS: trend math and unavailable metrics; no model/CUDA loaded")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("model", "library", "output", "cudart"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--family", choices=("qwen", "r2t2"))
    parser.add_argument("--cycles", type=int)
    parser.add_argument("--threads", type=int, choices=(1, 2), default=1)
    parser.add_argument("--model-loads", type=int, choices=(1, 2, 3), default=1)
    parser.add_argument("--long-seconds", type=int, choices=(0, 16, 36), default=16)
    parser.add_argument("--dependencies", type=Path, action="append")
    parser.add_argument("--zh-wav", type=Path, default=DEFAULT_ZH)
    parser.add_argument("--en-wav", type=Path, default=DEFAULT_EN)
    parser.add_argument("--tracemalloc", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if not all((args.model, args.library, args.output, args.family)):
        parser.error("--model, --library, --output and --family are required")
    args.cycles = args.cycles if args.cycles is not None else (8 if args.family == "qwen" else 12)
    if args.cycles < 2:
        parser.error("--cycles must be at least 2")
    args.dependencies = args.dependencies or [RESOURCES / "r2t2-native/cuda"]
    if args.output.with_suffix(args.output.suffix + ".partial").exists():
        parser.error("partial output already exists; choose a new --output")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {"schema": 1, "status": "running", "created_utc": datetime.now(timezone.utc).isoformat(),
              "pid": os.getpid(), "config": {key: str(value) if isinstance(value, Path) else value
              for key, value in vars(args).items() if key != "dependencies"}, "points": [], "rows": [], "cleanup_errors": []}
    report["config"]["dependencies"] = [str(path.resolve()) for path in args.dependencies]
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(report, output)
    try:
        probe(args, report)
        report["status"] = "completed" if not report["cleanup_errors"] else "failed"
    except BaseException as error:
        report.update(status="failed", error=f"{type(error).__name__}: {error}")
    report["late_cycle_trends"] = summarize(report["points"])
    save(args.output, report)
    print(f"{report['status']}: {args.output}")
    return 0 if report["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
