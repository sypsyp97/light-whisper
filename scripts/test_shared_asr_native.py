"""Opt-in audio.cpp C ABI check for R2T2 and Qwen3-ASR, without server wrappers.

Capture: --library DLL --model GGUF --family qwen3_asr --backend cuda --audio WAV --output baseline.json
Compare: use the same inputs with a candidate DLL and --reference baseline.json.
--load-only stops before session creation; --self-test never loads a DLL.
An external oracle may contain text, family, model_sha256, audio_sha256 and
requested_language (null for automatic detection). Exact text is compared.
"""
import argparse
from array import array
from contextlib import ExitStack, contextmanager
import ctypes as ct
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import sys
import time
import unittest
import wave


SAMPLE_RATE = 16000


class ModelConfig(ct.Structure):
    _fields_ = [(name, ct.c_char_p) for name in
                ("family_hint", "config_id", "weight_id", "model_spec_override")]


class BackendConfig(ct.Structure):
    _fields_ = [("backend", ct.c_char_p), ("device", ct.c_int), ("threads", ct.c_int)]


class CheckFailure(RuntimeError):
    def __init__(self, message, kind="semantic", **details):
        super().__init__(message)
        self.kind, self.details = kind, details


def require(condition, message):
    if not condition:
        raise CheckFailure(message)


def sha256(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def validate_pcm(pcm):
    if (not isinstance(pcm, array) or pcm.typecode != "f" or pcm.itemsize != 4
            or not pcm or not all(math.isfinite(value) for value in pcm)):
        raise ValueError("expected nonempty, finite float32 PCM")


def read_wav(path):
    with wave.open(str(path), "rb") as source:
        if (source.getframerate(), source.getnchannels(), source.getsampwidth(),
                source.getcomptype()) != (SAMPLE_RATE, 1, 2, "NONE"):
            raise ValueError("fixture must be mono 16 kHz PCM16 WAV")
        frames = source.getnframes()
        raw = source.readframes(frames)
    if len(raw) != frames * 2:
        raise ValueError("fixture PCM is truncated")
    integers = array("h", raw)
    if sys.byteorder != "little":
        integers.byteswap()
    pcm = array("f", (sample / 32768.0 for sample in integers))
    validate_pcm(pcm)
    return pcm


class CAbi:

    def __init__(self, library):
        handle, string, integer, pointer = ct.c_void_p, ct.c_char_p, ct.c_int, ct.POINTER
        signatures = {
            "abi_version": (ct.c_uint32, []),
            "build_version": (string, []),
            "last_error": (string, []),
            "registry_create": (integer, [string, pointer(handle)]),
            "registry_family_count": (ct.c_size_t, [handle]),
            "registry_family": (integer, [handle, ct.c_size_t, pointer(string)]),
            "model_load": (integer, [handle, string, pointer(ModelConfig), handle, pointer(handle)]),
            "model_family": (string, [handle]),
            "model_supports": (integer, [handle, string, string]),
            "session_create": (integer, [handle, string, string, pointer(BackendConfig), handle, pointer(handle)]),
            "request_create": (handle, []),
            "request_set_text": (integer, [handle, string, string]),
            "request_set_audio": (integer, [handle, pointer(ct.c_float), ct.c_size_t, integer, integer]),
            "request_set_option": (integer, [handle, string, string]),
            "session_run": (integer, [handle, handle, pointer(handle)]),
            "result_text": (integer, [handle, pointer(string), pointer(string)]),
        }
        for name in ("registry", "model", "session", "request", "result"):
            signatures[name + "_free"] = (None, [handle])
        self.functions = {}
        for name, (result_type, argument_types) in signatures.items():
            function = getattr(library, "audiocpp_" + name)
            function.restype, function.argtypes = result_type, argument_types
            self.functions[name] = function

    def __getitem__(self, name):
        return self.functions[name]

    def call(self, name, *arguments):
        status = self[name](*arguments)
        if status:

            detail = (self["last_error"]() or b"").decode("utf-8", errors="replace")
            raise CheckFailure(f"audiocpp_{name} failed ({status}): {detail}",
                               "semantic" if status == 2 else "native_runtime",
                               native_function=name, native_status=status, native_error=detail)


@contextmanager
def load_abi(library, dependencies):
    with ExitStack() as directories:
        if os.name == "nt":
            for path in dict.fromkeys([library.resolve().parent, *[item.resolve() for item in dependencies]]):
                directories.enter_context(os.add_dll_directory(str(path)))
        yield CAbi(ct.CDLL(str(library.resolve())))


def run_native(api, args, pcm, report):
    validate_pcm(pcm)
    version = api["abi_version"]()
    require(version >> 16 == 0 and ((version >> 8) & 255) >= 4,
            f"expected compatible audio.cpp ABI >= 0.4, got {version:#x}")
    report["abi"] = {"packed_version": version,
                     "build_version": (api["build_version"]() or b"").decode("utf-8")}
    with ExitStack() as handles:
        def owned(kind):
            handle = ct.c_void_p()

            handles.callback(api[kind + "_free"], handle)
            return handle

        def timed_call(name, *arguments):
            report["stage"] = name
            start = time.perf_counter()
            try:
                api.call(name, *arguments)
            finally:
                report["timings_ms"][name] = (time.perf_counter() - start) * 1000

        registry = owned("registry")
        timed_call("registry_create", None, ct.byref(registry))
        require(bool(registry), "registry_create returned a null handle")
        families = []
        for index in range(api["registry_family_count"](registry)):
            family = ct.c_char_p()
            api.call("registry_family", registry, index, ct.byref(family))
            families.append((family.value or b"").decode("utf-8"))
        report["registry_families"] = families

        model = owned("model")
        config = ModelConfig(args.family.encode("utf-8"), None, None, None)
        timed_call("model_load", registry, str(args.model.resolve()).encode("utf-8"),
                   ct.byref(config), None, ct.byref(model))
        require(bool(model), "model_load returned a null handle")
        loaded_family = (api["model_family"](model) or b"").decode("utf-8")
        report["loaded_family"] = loaded_family
        require(loaded_family == args.family, "loaded model family differs from requested family")
        if args.load_only:
            return
        require(api["model_supports"](model, b"asr", b"offline") == 1,
                "model does not advertise offline ASR")
        session = owned("session")
        backend = BackendConfig(args.backend.encode("ascii"), 0, args.threads)
        timed_call("session_create", model, b"asr", b"offline", ct.byref(backend), None, ct.byref(session))
        require(bool(session), "session_create returned a null handle")
        request = owned("request")
        request.value = api["request_create"]()
        require(bool(request), "request_create returned a null handle")
        if args.language:
            api.call("request_set_text", request, b"", args.language.encode("utf-8"))
        for key, value in report["config"]["request_options"].items():
            api.call("request_set_option", request, key.encode("utf-8"), value.encode("utf-8"))
        samples = (ct.c_float * len(pcm)).from_buffer(pcm)
        api.call("request_set_audio", request, samples, len(pcm), SAMPLE_RATE, 1)
        result = owned("result")
        timed_call("session_run", session, request, ct.byref(result))
        require(bool(result), "session_run returned a null handle")
        text, language = ct.c_char_p(), ct.c_char_p()
        report["stage"] = "result_text"
        status = api["result_text"](result, ct.byref(text), ct.byref(language))
        if status != 7:
            if status:
                detail = (api["last_error"]() or b"").decode("utf-8", errors="replace")
                raise CheckFailure(f"audiocpp_result_text failed ({status}): {detail}",
                                   "native_runtime", native_function="result_text", native_status=status)
        report["result"] = {"text": (text.value or b"").decode("utf-8") if status == 0 else "",
                            "language": (language.value or b"").decode("utf-8") or None,
                            "text_status": status}


def oracle_from_report(report):
    return {"text": report["result"]["text"], "family": report["config"]["family"],
            "model_sha256": report["model"]["sha256"], "audio_sha256": report["fixture"]["sha256"],
            "requested_language": report["config"]["language"]}


def check_reference(reference, report, *, compare_text=False):
    if "schema" in reference:
        require(reference.get("status") == "pass" and reference.get("coverage") == "offline_asr",
                "reference must be a successful offline ASR capture")
        reference = oracle_from_report(reference)
    actual = oracle_from_report(report) if compare_text else {
        "family": report["config"]["family"], "model_sha256": report["model"]["sha256"],
        "audio_sha256": report["fixture"]["sha256"], "requested_language": report["config"]["language"]}
    for key in ("text", "family", "model_sha256", "audio_sha256", "requested_language"):
        require(key in reference, f"reference missing {key}")
        if key in actual:
            require(reference[key] == actual[key], f"baseline {key} differs")


def check_transcript(args, report):
    text = report["result"]["text"]
    if args.expected is not None:
        require(text == args.expected, "transcript differs from --expected")
    elif args.expect_empty:
        require(text == "", "expected no recognized text")
    else:
        require(bool(text.strip()), "speech fixture produced no recognized text")


def make_report(args, pcm):
    return {"schema": 1, "created_utc": datetime.now(timezone.utc).isoformat(), "status": "running",
            "coverage": "model_load" if args.load_only else "offline_asr", "stage": "inputs",
            "config": {"family": args.family, "backend": args.backend, "threads": args.threads,
                       "language": args.language, "request_options":
                       {"return_timestamps": "false", "audio_chunk_mode": "none"}
                       if args.family == "qwen3_asr" else {}},

            "library": {"name": args.library.name, "sha256": sha256(args.library)},
            "model": {"name": args.model.name, "sha256": sha256(args.model), "bytes": args.model.stat().st_size},
            "fixture": {"name": args.audio.name, "sha256": sha256(args.audio), "samples": len(pcm),
                        "sample_rate": SAMPLE_RATE, "channels": 1, "pcm_type": "float32",
                        "duration_seconds": len(pcm) / SAMPLE_RATE,
                        "silent": all(value == 0 for value in pcm)},
            "script_sha256": sha256(__file__), "timings_ms": {}}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("library", "model", "audio", "output", "reference"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--dependencies", type=Path, action="append", default=[])
    parser.add_argument("--family", choices=("confucius4_r2t2", "qwen3_asr"))
    parser.add_argument("--backend", choices=("cpu", "cuda"), default="cpu")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--language", help="Omit for automatic language detection")
    expected = parser.add_mutually_exclusive_group()
    expected.add_argument("--expected", help="Exact measured transcript, including punctuation and whitespace")
    expected.add_argument("--expect-empty", action="store_true", help="Assert empty text for an explicitly supplied silence fixture")
    parser.add_argument("--load-only", action="store_true", help="No session creation or inference")
    parser.add_argument("--self-test", action="store_true", help="Run validation/cleanup tests without native libraries")
    args = parser.parse_args(argv)
    if args.self_test:
        result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(HarnessTests))
        return 0 if result.wasSuccessful() else 1
    for name in ("library", "model", "audio", "family"):
        if getattr(args, name) is None:
            parser.error(f"--{name} is required")
    if args.threads <= 0:
        parser.error("--threads must be positive")
    if args.load_only and (args.reference or args.expected is not None or args.expect_empty):
        parser.error("transcript assertions require inference; remove --load-only")
    if args.output and args.output.exists():
        parser.error("--output already exists; choose a new evidence file")
    report = {"schema": 1, "status": "running", "stage": "inputs"}
    try:
        pcm = read_wav(args.audio)
        report = make_report(args, pcm)
        if args.expect_empty and not report["fixture"]["silent"]:
            raise ValueError("--expect-empty requires a silent PCM fixture")
        reference = json.loads(args.reference.read_text(encoding="utf-8")) if args.reference else None
        if reference is not None:
            check_reference(reference, report)
        with load_abi(args.library, args.dependencies) as api:
            run_native(api, args, pcm, report)
        if not args.load_only:
            report["stage"] = "transcript"
            check_transcript(args, report)
            if reference is not None:
                check_reference(reference, report, compare_text=True)
        report["status"], report["stage"] = "pass", "complete"
    except (CheckFailure, OSError, ValueError, AttributeError, KeyError, TypeError, wave.Error) as error:
        report["status"] = "failed"
        report["failure_kind"] = error.kind if isinstance(error, CheckFailure) else "infrastructure"
        report["error"] = str(error)
        if isinstance(error, CheckFailure):
            report.update(error.details)
    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with args.output.open("x", encoding="utf-8") as target:
            target.write(rendered)
    print(rendered, end="", flush=True)
    return 0 if report["status"] == "pass" else 1


class HarnessTests(unittest.TestCase):
    def arguments(self):
        return argparse.Namespace(model=Path("model.gguf"), family="qwen3_asr", backend="cpu",
                                  threads=1, language=None, load_only=False, expected=None, expect_empty=False)

    def report(self):
        return {"config": {"family": "qwen3_asr", "language": None,
                           "request_options": {"return_timestamps": "false", "audio_chunk_mode": "none"}},
                "fixture": {"sha256": "fixture"}, "model": {"sha256": "model"},
                "result": {"text": "measured text"}, "timings_ms": {}}

    def test_pcm_validation_rejects_empty_nonfinite_and_wrong_precision(self):
        validate_pcm(array("f", [0, -1, 0.5]))
        for invalid in (array("f"), array("d", [0]), array("f", [float("nan")]), array("f", [float("inf")])):
            with self.subTest(value=invalid), self.assertRaises(ValueError):
                validate_pcm(invalid)

    def test_reference_requires_fixture_model_and_exact_measured_text(self):
        report = self.report()
        reference = oracle_from_report(report)
        check_reference(reference, report, compare_text=True)
        for key in reference:
            changed = dict(reference, **{key: "changed"})
            with self.subTest(key=key), self.assertRaises(CheckFailure):
                check_reference(changed, report, compare_text=True)
        with self.assertRaises(CheckFailure):
            check_reference({"text": "measured text"}, report)

    def test_transcript_assertions_do_not_normalize_output(self):
        args, report = self.arguments(), self.report()
        check_transcript(args, report)
        args.expected = "measured text "
        with self.assertRaises(CheckFailure):
            check_transcript(args, report)
        args.expected, args.expect_empty = None, True
        with self.assertRaises(CheckFailure):
            check_transcript(args, report)
        report["result"]["text"] = ""
        check_transcript(args, report)
        args.expect_empty = False
        with self.assertRaises(CheckFailure):
            check_transcript(args, report)

    def test_handles_freed_even_when_status_call_sets_output_then_fails(self):
        for failure in (None, "registry_create", "model_load", "session_create", "request_set_option",
                        "request_set_audio", "session_run", "result_text"):
            api = FakeAbi(failure)
            with self.subTest(failure=failure):
                if failure is None:
                    run_native(api, self.arguments(), array("f", [0.5]), self.report())
                else:
                    with self.assertRaises(CheckFailure):
                        run_native(api, self.arguments(), array("f", [0.5]), self.report())
                self.assertEqual(set(api.allocated), set(api.freed))
                self.assertEqual(len(api.freed), len(set(api.freed)))
                self.assertEqual(api.freed, list(reversed(api.allocated)))

    def test_load_only_never_creates_session_or_runs_audio(self):
        args, api = self.arguments(), FakeAbi()
        args.load_only = True
        run_native(api, args, array("f", [0.5]), self.report())
        self.assertEqual(api.allocated, ["registry", "model"])
        self.assertNotIn("session_run", api.called)


class FakeAbi:

    def __init__(self, failure=None):
        self.failure, self.allocated, self.freed, self.called = failure, [], [], []
        self.functions = {}
        self.functions.update(abi_version=lambda: 0x400, build_version=lambda: b"test-only",
                              last_error=lambda: b"injected test failure", registry_family_count=lambda registry: 0,
                              model_family=lambda model: b"qwen3_asr", model_supports=lambda *args: 1)
        for kind in ("registry", "model", "session", "request", "result"):
            def free(handle, kind=kind):
                if handle.value:
                    self.freed.append(kind)
            self.functions[kind + "_free"] = free

    def __getitem__(self, name):
        if name in self.functions:
            return self.functions[name]
        def function(*arguments):
            self.called.append(name)
            kinds = {"registry_create": "registry", "model_load": "model",
                     "session_create": "session", "session_run": "result"}
            if name in kinds:
                self.allocated.append(kinds[name])
                ct.cast(arguments[-1], ct.POINTER(ct.c_void_p))[0] = len(self.allocated)
            if name == "request_create":
                self.allocated.append("request")
                return len(self.allocated)
            if name == "result_text":
                ct.cast(arguments[1], ct.POINTER(ct.c_char_p))[0] = b"measured text"
            return 4 if name == self.failure else 0
        return function

    def call(self, name, *arguments):
        status = self[name](*arguments)
        if status:
            raise CheckFailure("injected test failure", "native_runtime")


if __name__ == "__main__":
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    raise SystemExit(main())
