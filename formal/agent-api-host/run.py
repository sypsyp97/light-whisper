"""Actual windowless native app -> private TCP/HMAC -> real controller -> native pipes.

Only accepts an explicitly named, separately built debug executable whose owning
source has the early agent flag guard. Never discovers or launches installed apps.
The bundled synthetic Rust engine tests IPC/lifecycle, never model inference.
"""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest

REPO = Path(__file__).resolve().parents[2]
MARKER = "LIGHT_WHISPER_AGENT_PROTOCOL_1"
LOCAL = ("qwen3-asr-0.6b", "confucius4-r2t2")
WAIT = 12
OPTIONS = None


def wait_until(predicate, message, timeout=WAIT):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        threading.Event().wait(.01)  # observation polling, never a race barrier
    raise AssertionError(message)


class Job:
    """Kernel-owned containment, exact process handles; no PID/name tree killing."""
    def __init__(self):
        self.api = ctypes.WinDLL("kernel32", use_last_error=True)
        self.api.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        self.api.CreateJobObjectW.restype = wintypes.HANDLE
        self.api.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        self.api.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        self.api.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        self.api.CloseHandle.argtypes = [wintypes.HANDLE]
        self.handle = self.api.CreateJobObjectW(None, None)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        # JOBOBJECT_EXTENDED_LIMIT_INFORMATION: pointer-size aligned basic limits,
        # IO_COUNTERS (6 u64), then four SIZE_T counters. Kill-on-close = 0x2000.
        class Basic(ctypes.Structure):
            _fields_ = [("per_process", ctypes.c_int64), ("per_job", ctypes.c_int64),
                        ("flags", wintypes.DWORD), ("min_ws", ctypes.c_size_t),
                        ("max_ws", ctypes.c_size_t), ("active", wintypes.DWORD),
                        ("affinity", ctypes.c_size_t), ("priority", wintypes.DWORD),
                        ("scheduling", wintypes.DWORD)]
        class Limits(ctypes.Structure):
            _fields_ = [("basic", Basic), ("io", ctypes.c_uint64 * 6),
                        ("memory", ctypes.c_size_t * 4)]
        limits = Limits()
        limits.basic.flags = 0x2000
        if not self.api.SetInformationJobObject(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            raise ctypes.WinError(ctypes.get_last_error())

    def add(self, child):
        if not self.api.AssignProcessToJobObject(self.handle, wintypes.HANDLE(int(child._handle))):
            raise ctypes.WinError(ctypes.get_last_error())

    def close(self):
        if self.handle:
            self.api.TerminateJobObject(self.handle, 94)
            self.api.CloseHandle(self.handle)
            self.handle = None


class ExactProcess:
    def __init__(self, pid):
        self.api = ctypes.WinDLL("kernel32", use_last_error=True)
        self.api.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.api.OpenProcess.restype = wintypes.HANDLE
        self.api.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        self.api.CloseHandle.argtypes = [wintypes.HANDLE]
        self.handle = self.api.OpenProcess(0x0010_0000, False, pid)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())

    def reaped(self):
        return self.api.WaitForSingleObject(self.handle, 0) == 0

    def close(self):
        self.api.CloseHandle(self.handle)


class Process:
    def __init__(self, case, command, name, env=None):
        self.name = name
        self.messages = queue.Queue()
        self.child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE, env=env or case.env,
                                      creationflags=subprocess.CREATE_NO_WINDOW, cwd=case.data)
        case.processes.append(self)
        case.job.add(self.child)
        self.logs = []
        self.workers = []
        for label, pipe in (("stdout", self.child.stdout), ("stderr", self.child.stderr)):
            path = case.evidence / f"{name}-{self.child.pid}-{label}.txt"
            self.logs.append(path)
            worker = threading.Thread(target=self._drain, args=(pipe, path, label), daemon=True)
            worker.start()
            self.workers.append(worker)

    def _drain(self, pipe, path, label):
        retained = 0
        with path.open("wb") as log:
            while line := pipe.readline(1024 * 1024):
                if retained < 2 * 1024 * 1024:
                    log.write(line[:2 * 1024 * 1024 - retained])
                    log.flush()
                    retained += len(line)
                if label == "stdout":
                    try:
                        value = json.loads(line.decode("utf-8"))
                    except (ValueError, UnicodeError):
                        continue
                    self.messages.put(value)
        if label == "stdout":
            self.messages.put(None)

    def send(self, value):
        self.child.stdin.write((json.dumps(value, ensure_ascii=False) + "\n").encode("utf-8"))
        self.child.stdin.flush()

    def receive(self, timeout=WAIT):
        try:
            value = self.messages.get(timeout=timeout)
        except queue.Empty as error:
            raise AssertionError(f"{self.name}: no JSON before deadline; logs={self.logs}") from error
        if value is None:
            code = self.child.wait(timeout=WAIT)
            raise AssertionError(f"{self.name}: exited {code} before expected JSON; logs={self.logs}")
        return value

    def wait(self, expected=0):
        code = self.child.wait(timeout=WAIT)
        if expected is not None:
            assert code == expected, f"{self.name}: exit {code}, expected {expected}; logs={self.logs}"
        return code

    def close_input(self):
        if not self.child.stdin.closed:
            self.child.stdin.close()


class NativeHost(unittest.TestCase):
    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(prefix="lw-native-host-"))
        self.data = self.scratch / "data"
        self.control = self.scratch / "control"
        self.temp = self.scratch / "temp"
        self.cache = self.scratch / "model-cache"
        self.evidence = OPTIONS.evidence / self.id().split(".")[-1]
        self.evidence.mkdir(parents=True, exist_ok=True)
        for path in (self.data, self.control, self.temp, self.cache):
            path.mkdir()
        self.config = {"engine": LOCAL[0], "models_dir": str(self.cache), "gpu_idle_seconds": 0,
                       "unrelated_test_setting": "preserve"}
        (self.data / "engine.json").write_text(json.dumps(self.config), encoding="utf-8")
        self.profile = {"r2t2": {"context": "synthetic profile context", "language": "de"},
                        "hot_words": [], "correction_patterns": [], "vocab_frequency": {},
                        "total_transcriptions": 0, "last_updated": 0,
                        "llm_provider": {"active": "cerebras", "custom_providers": []},
                        "correction_validation_enabled": False}
        (self.data / "user_profile.json").write_text(json.dumps(self.profile), encoding="utf-8")
        self.profile_bytes = (self.data / "user_profile.json").read_bytes()
        self.env = {**os.environ, "LIGHT_WHISPER_DATA_DIR": str(self.data),
                    "LIGHT_WHISPER_HOST_FIXTURE_ROOT": str(self.control),
                    "HF_HOME": str(self.cache.parent), "HF_HUB_CACHE": str(self.cache),
                    "HF_HUB_OFFLINE": "1", "TRANSFORMERS_OFFLINE": "1",
                    "LIGHT_WHISPER_NATIVE_THREADS": "2", "OMP_NUM_THREADS": "2",
                    "MKL_NUM_THREADS": "2", "OPENBLAS_NUM_THREADS": "2",
                    "TEMP": str(self.temp), "TMP": str(self.temp)}
        self.job = Job()
        self.processes = []
        self.native_handles = []
        self.sequence = 0

    @property
    def endpoint(self):
        return self.data / "agent/endpoint.json"

    def audit(self, event=None):
        try:
            lines = (self.control / "audit.jsonl").read_text(encoding="utf-8").splitlines()
        except FileNotFoundError:
            return []
        values = []
        for line in lines:
            try:
                value = json.loads(line)
            except ValueError:  # the other process can be in its final append
                continue
            if event is None or value["event"] == event:
                values.append(value)
        return values

    def host(self, ready=True, env=None):
        process = Process(self, [str(OPTIONS.app_exe), "--agent-host"], "host", env)
        if ready:
            response = process.receive()
            self.assertEqual(response.get("success"), True)
            self.assertEqual(response.get("surface"), "desktop-host")
            self.assertEqual(response.get("agent_protocol"), MARKER)
            self.assertEqual(response.get("pid"), process.child.pid)
            self.assertEqual(response.get("window_count"), 0)
            self.assertTrue(self.endpoint.is_file(), "readiness follows endpoint publication")
        return process

    def client(self):
        process = Process(self, [str(OPTIONS.app_exe), "--agent-client"], "client")
        self.assertTrue(process.receive().get("success"), "broker ready is local only")
        return process

    def call(self, action, **fields):
        self.sequence += 1
        client = self.client()
        client.send({"request_id": self.sequence, "action": action, **fields})
        response = client.receive()
        self.assertEqual(response.get("request_id"), self.sequence)
        client.close_input()
        client.wait(None)
        return response

    def cli(self, *args, timeout="8"):
        process = Process(self, [sys.executable, str(REPO / "scripts/light_whisper_cli.py"),
                                "--target", "app", "--app-exe", str(OPTIONS.app_exe),
                                "--timeout", timeout, *args], "python-cli")
        response = process.receive()
        code = process.wait(None)
        return code, response

    def start_engine(self):
        response = self.call("engine_start")
        self.assertTrue(response.get("success"), response)
        spawn = wait_until(lambda: self.audit("spawn"), "real start_server must spawn fixture")[-1]
        handle = ExactProcess(spawn["pid"])
        self.native_handles.append(handle)
        self.assertFalse(handle.reaped())
        return spawn, handle

    def finish(self, host, eof=False):
        if eof:
            host.close_input()
        else:
            host.send({"action": "exit"})
        host.wait()
        self.assertFalse(self.endpoint.exists(), "owned endpoint removed before lease release")
        for handle in self.native_handles:
            wait_until(handle.reaped, "owned native child was not reaped")
        self.assertEqual(list(self.temp.glob("light_whisper_agent_*.wav")), [])

    def assert_no_history_or_profile_write(self):
        self.assertEqual((self.data / "user_profile.json").read_bytes(), self.profile_bytes)
        self.assertFalse(any(p.suffix in (".db", ".sqlite", ".sqlite3") for p in self.data.rglob("*")))

    def tearDown(self):
        failures = []
        for process in reversed(self.processes):
            try:
                process.close_input()
                process.child.wait(timeout=3)
            except (OSError, ValueError, subprocess.TimeoutExpired):
                if process.child.poll() is None:
                    process.child.kill()
        # Exact job contains all owned descendants even if host cleanup is broken.
        self.job.close()
        for process in self.processes:
            try:
                process.child.wait(timeout=3)
                for worker in process.workers:
                    worker.join(timeout=3)
                    if worker.is_alive():
                        failures.append(f"pipe drain did not settle: {process.name}")
                for pipe in (process.child.stdout, process.child.stderr):
                    pipe.close()
            except (OSError, subprocess.TimeoutExpired) as error:
                failures.append(str(error))
        for handle in self.native_handles:
            if not handle.reaped():
                failures.append("native process survived containment job close")
            handle.close()
        # Retain synthetic audit/evidence outside scratch before deleting state.
        if (self.control / "audit.jsonl").exists():
            shutil.copy2(self.control / "audit.jsonl", self.evidence / "native-audit.jsonl")
        (self.evidence / "cleanup.json").write_text(json.dumps({"failures": failures,
            "owned_processes": len(self.processes), "scratch_removed": not failures}), encoding="utf-8")
        shutil.rmtree(self.scratch)
        self.assertTrue((self.evidence / "cleanup.json").is_file())
        self.assertEqual(failures, [], "cleanup/containment failure")

    def test_01_compiled_protocol_marker(self):
        self.assertTrue(MARKER.encode() in OPTIONS.app_exe.read_bytes(),
                        "INTERFACE_PENDING: real compiled PE must advertise the native broker")

    def test_02_broker_readiness_and_local_exit_without_host(self):
        client = self.client()
        self.assertFalse(self.endpoint.exists())
        self.assertEqual(self.audit(), [])
        client.send({"request_id": 8, "action": "exit"})
        reply = client.receive()
        self.assertTrue(reply.get("success"))
        self.assertEqual(reply.get("request_id"), 8)
        client.wait()
        self.assertFalse(self.endpoint.exists())

    def test_03_broker_eof_without_host(self):
        client = self.client()
        client.close_input()
        client.wait()
        self.assertFalse(self.endpoint.exists())
        self.assertEqual(self.audit(), [])

    def test_04_missing_endpoint_is_correlated_terminal_error(self):
        client = self.client()
        client.send({"request_id": 95, "action": "status"})
        reply = client.receive()
        self.assertFalse(reply.get("success"))
        self.assertEqual(reply.get("request_id"), 95)
        self.assertIsInstance(reply.get("error"), str)
        self.assertNotEqual(client.wait(None), 0)
        self.assertFalse(self.endpoint.exists())

    def test_05_host_requires_nonempty_explicit_data_directory(self):
        source = (REPO / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
        entries = list(re.finditer(r"pub\s+fn\s+run\s*\(\s*\)\s*\{", source))
        self.assertEqual(len(entries), 1, "exact owning run() required before missing-data launch")
        prefix = re.sub(r"\s+", "", source[entries[0].end():])
        guards = {
            "baseline_exit2_stub": '''
                if std::env::args().any(|argument| matches!(argument.as_str(), "--agent-client" | "--agent-host")) {
                    eprintln!("Agent native entrypoint pending implementation");
                    std::process::exit(2);
                }
            ''',
            "explicit_data_guard": '''
                if std::env::args().any(|argument| argument == "--agent-host")
                    && std::env::var_os("LIGHT_WHISPER_DATA_DIR").is_none_or(|directory| directory.is_empty()) {
                    eprintln!("--agent-host requires LIGHT_WHISPER_DATA_DIR");
                    std::process::exit(2);
                }
            ''',
        }
        proof = next((name for name, block in guards.items()
                      if prefix.startswith(re.sub(r"\s+", "", block))), None)
        self.assertIsNotNone(proof, "unsafe missing-data fallback: run() must begin with exact reviewed guard; no process spawned")
        (self.evidence / "missing-data-source-guard.json").write_text(json.dumps({
            "proof": proof, "source_sha256": hashlib.sha256(source.encode("utf-8")).hexdigest(),
            "limit": "static exact-prefix proof; native exit2 checked separately; not a general control-flow proof",
        }), encoding="utf-8")
        for value in (None, ""):
            with self.subTest(value=value):
                env = dict(self.env)
                if value is None:
                    env.pop("LIGHT_WHISPER_DATA_DIR")
                else:
                    env["LIGHT_WHISPER_DATA_DIR"] = value
                host = self.host(False, env)
                self.assertNotEqual(host.wait(None), 0)
                self.assertFalse(self.endpoint.exists())
                self.assertEqual(self.audit(), [])

    def test_06_windowless_host_ready_does_not_start_or_claim_model_ready(self):
        host = self.host()
        self.assertEqual(self.audit(), [], "host startup never autostarts a model")
        code, status = self.cli("status")
        self.assertEqual(code, 0)
        self.assertEqual(status.get("surface"), "desktop")
        self.assertEqual(status.get("configured_engine"), LOCAL[0])
        self.assertFalse(status.get("running"))
        self.assertFalse(status.get("ready"))
        self.assertFalse(status.get("runtime_observed"))
        self.assertIsNone(status.get("runtime_engine"))
        self.assertEqual(self.audit(), [])
        self.assertTrue(host.messages.empty(), "host prints exactly one readiness JSON")
        self.assert_no_history_or_profile_write()
        self.finish(host)

    def test_07_host_local_exit_reaps_owned_engine(self):
        host = self.host()
        self.start_engine()
        self.finish(host)

    def test_08_host_eof_reaps_owned_engine(self):
        host = self.host()
        self.start_engine()
        self.finish(host, eof=True)

    def test_09_second_host_same_directory_cannot_replace_endpoint(self):
        host = self.host()
        before = self.endpoint.read_bytes()
        second = self.host(False)
        self.assertNotEqual(second.wait(None), 0)
        self.assertEqual(self.endpoint.read_bytes(), before)
        self.assertTrue(self.call("engine_get").get("success"))
        self.finish(host)
        # Lease release must allow a fresh owner after complete shutdown.
        replacement = self.host()
        self.finish(replacement)

    def test_10_distinct_data_directory_isolates_hosts(self):
        first = self.host()
        first_endpoint = self.endpoint.read_bytes()
        other = self.scratch / "other-data"
        other.mkdir()
        second = self.host(False, {**self.env, "LIGHT_WHISPER_DATA_DIR": str(other)})
        reply = second.receive()
        self.assertTrue(reply.get("success"), reply)
        self.assertEqual(reply.get("window_count"), 0)
        self.assertEqual(reply.get("pid"), second.child.pid)
        self.assertTrue((other / "agent/endpoint.json").is_file())
        second.close_input()
        second.wait()
        self.assertFalse((other / "agent/endpoint.json").exists())
        self.assertEqual(self.endpoint.read_bytes(), first_endpoint)
        self.assertTrue(self.call("engine_get").get("success"))
        self.finish(first)

    def test_11_selection_persists_stops_previous_without_autostart(self):
        host = self.host()
        spawn, native = self.start_engine()
        self.assertEqual(spawn["args"], ["serve", "--engine", LOCAL[0]])
        self.assertEqual(Path(spawn["data_dir"]), self.data)
        self.assertEqual(Path(spawn["model_cache"]), self.cache)
        code, reply = self.cli("engine", "set", LOCAL[1])
        self.assertEqual(code, 0)
        self.assertTrue(reply.get("success"), reply)
        self.assertEqual(reply.get("configured_engine"), LOCAL[1])
        wait_until(native.reaped, "selection change stops previous native process")
        saved = json.loads((self.data / "engine.json").read_text(encoding="utf-8"))
        self.assertEqual(saved["engine"], LOCAL[1])
        self.assertEqual(saved["unrelated_test_setting"], "preserve")
        self.assertEqual(len(self.audit("spawn")), 1, "selection does not autostart")
        self.assertEqual(self.call("engine_get")["configured_engine"], LOCAL[1])
        next_spawn, _ = self.start_engine()
        self.assertEqual(next_spawn["args"], ["serve", "--engine", LOCAL[1]])
        self.assert_no_history_or_profile_write()
        self.finish(host)

    def test_12_restart_replaces_actual_native_process_and_counters(self):
        host = self.host()
        old_spawn, old_native = self.start_engine()
        reply = self.call("engine_restart")
        self.assertTrue(reply.get("success"), reply)
        wait_until(old_native.reaped, "restart reaps old native process")
        spawns = self.audit("spawn")
        self.assertEqual(len(spawns), 2)
        self.assertNotEqual(spawns[1]["pid"], old_spawn["pid"])
        replacement = ExactProcess(spawns[1]["pid"])
        self.native_handles.append(replacement)
        self.assertEqual(self.call("stats")["stats"]["transcription_count"], 0)
        self.finish(host)

    def test_13_python_cli_transcription_reaches_native_and_preserves_original(self):
        host = self.host()
        self.start_engine()
        audio = self.scratch / "synthetic 原文.wav"
        content = b"RIFF-synthetic-audio-no-inference\x00\x01"
        audio.write_bytes(content)
        default = self.call("transcribe", audio_path=str(audio), options={})
        self.assertTrue(default.get("success"), default)
        defaults = self.audit("audio")[0]["request"]["options"]
        self.assertEqual(defaults["language"], "de", "actual host loads own synthetic profile")
        self.assertEqual(defaults["context"], "synthetic profile context")
        code, reply = self.cli("transcribe", str(audio), "--language", "en", "--context", "", "--hot-word", "原文")
        self.assertEqual(code, 0)
        self.assertEqual(reply["text"], "Synthetic 原文 transcription")
        recorded = self.audit("audio")
        self.assertEqual(len(recorded), 2)
        self.assertEqual(bytes(recorded[1]["bytes"]), content)
        owned = Path(recorded[1]["path"])
        self.assertNotEqual(owned, audio)
        self.assertFalse(owned.exists())
        request = recorded[1]["request"]
        self.assertEqual(request["options"]["language"], "en")
        self.assertEqual(request["options"]["context"], "")
        self.assertEqual(request["hot_words"], ["原文"])
        self.assertEqual(audio.read_bytes(), content)
        self.assertEqual(self.call("stats")["stats"]["transcription_count"], 2)
        self.assert_no_history_or_profile_write()
        self.finish(host)

    def test_14_models_inspects_only_isolated_empty_local_cache(self):
        host = self.host()
        code, reply = self.cli("models")
        self.assertEqual(code, 0)
        models = reply["models"]
        self.assertEqual(models["engine"], LOCAL[0])
        self.assertEqual(Path(models["cache_path"]), self.cache)
        self.assertFalse(models["all_present"])
        self.assertEqual(models["model_state"], "missing")
        self.assertGreater(len(models["missing_models"]), 0)
        self.assertEqual(list(self.cache.iterdir()), [], "model inspection never downloads")
        self.assertEqual(self.audit(), [])
        self.finish(host)

    def test_15_devices_returns_actual_enumeration_shape_without_engine(self):
        host = self.host()
        code, reply = self.cli("devices")
        self.assertEqual(code, 0)
        self.assertIsInstance(reply["devices"], list)
        for device in reply["devices"]:
            self.assertIsInstance(device["name"], str)
            self.assertIs(type(device["isDefault"]), bool)
        self.assertEqual(self.audit(), [])
        self.finish(host)

    def pending_transcribe(self, timeout=".3"):
        (self.control / "hold-transcribe").touch()
        audio = self.scratch / "pending.wav"
        audio.write_bytes(b"synthetic pending audio")
        process = Process(self, [sys.executable, str(REPO / "scripts/light_whisper_cli.py"),
                                "--app-exe", str(OPTIONS.app_exe), "--timeout", timeout,
                                "transcribe", str(audio)], "pending-python-cli")
        wait_until(lambda: (self.control / "entered-transcribe").exists(), "native transcription admission barrier")
        return process, audio

    def test_16_client_timeout_preserves_admitted_work_and_busy_controls(self):
        host = self.host()
        _, native = self.start_engine()
        pending, audio = self.pending_transcribe()
        self.assertFalse(pending.receive().get("success"))
        self.assertEqual(pending.wait(None), 1)
        self.assertFalse(native.reaped(), "client disconnect must preserve native owner")
        owned = Path(self.audit("audio")[0]["path"])
        self.assertTrue(owned.is_file(), "admitted operation still owns its copy")
        count = len(self.audit("command"))
        before = (self.data / "engine.json").read_bytes()
        status = self.call("status")
        self.assertTrue(status.get("busy"), status)
        for action, fields in (("engine_start", {}), ("engine_restart", {}),
                               ("engine_set", {"engine": LOCAL[1]}), ("stats", {}),
                               ("transcribe", {"audio_path": str(audio), "options": {}})):
            reply = self.call(action, **fields)
            self.assertFalse(reply.get("success"), reply)
            self.assertEqual(reply.get("code"), "busy", reply)
        self.assertEqual((self.data / "engine.json").read_bytes(), before)
        self.assertEqual(len(self.audit("command")), count, "busy controls have no native effects")
        (self.control / "release-transcribe").touch()
        wait_until(lambda: not owned.exists(), "completion releases owned audio and lease")
        stats = self.call("stats")
        self.assertEqual(stats["stats"]["transcription_count"], 1)
        self.assertEqual(len(self.audit("audio")), 1, "unknown outcome never retries")
        self.finish(host)

    def test_17_owner_eof_cancels_pending_operation_and_reaps_without_gate_release(self):
        host = self.host()
        self.start_engine()
        pending, _ = self.pending_transcribe(timeout="10")
        owned = Path(self.audit("audio")[0]["path"])
        self.assertTrue(owned.exists())
        self.finish(host, eof=True)
        self.assertFalse(owned.exists())
        self.assertFalse((self.control / "release-transcribe").exists())
        self.assertFalse(pending.receive().get("success"))
        self.assertEqual(pending.wait(None), 1)

    def test_18_startup_owner_remains_busy_until_settled_and_owner_eof_reaps(self):
        (self.control / "hold-startup").touch()
        host = self.host()
        client = self.client()
        client.send({"request_id": 71, "action": "engine_start"})
        wait_until(lambda: (self.control / "entered-startup").exists(), "startup native barrier")
        native = ExactProcess(self.audit("spawn")[0]["pid"])
        self.native_handles.append(native)
        before = (self.data / "engine.json").read_bytes()
        commands = self.audit("command")
        status = self.call("status")
        self.assertTrue(status.get("busy"), status)
        self.assertTrue(status.get("starting"), status)
        self.assertIsNone(status.get("running"), status)
        self.assertEqual(status.get("ready"), False, status)
        self.assertFalse(status.get("runtime_observed"), status)
        for action, fields in (("engine_set", {"engine": LOCAL[1]}), ("engine_restart", {}), ("engine_start", {})):
            reply = self.call(action, **fields)
            self.assertFalse(reply.get("success"), reply)
            self.assertEqual(reply.get("code"), "busy", reply)
            self.assertFalse(native.reaped(), "busy control must preserve exact blocked startup child")
            self.assertEqual(self.audit("command"), commands, "busy control must not send native commands")
            self.assertEqual((self.data / "engine.json").read_bytes(), before)
        self.assertEqual((self.data / "engine.json").read_bytes(), before)
        self.assertEqual(len(self.audit("spawn")), 1)
        self.finish(host, eof=True)
        self.assertFalse((self.control / "release-startup").exists())
        self.assertFalse(client.receive().get("success"))
        # Owner shutdown is an operation reply; the caller still owns broker stdin.
        client.close_input()
        client.wait(None)

    def test_19_fixture_parent_watch_contains_hard_killed_host(self):
        host = self.host()
        _, native = self.start_engine()
        self.pending_transcribe(timeout="10")
        host.child.kill()
        host.wait(None)
        # Assert before job fallback: fixture's exact host handle owns this exit.
        wait_until(native.reaped, "fixture must not outlive exact host after hard kill")

    def test_20_invalid_engine_cloud_and_arbitrary_actions_have_no_effect(self):
        host = self.host()
        _, native = self.start_engine()
        before = (self.data / "engine.json").read_bytes()
        commands = self.audit("command")
        client = self.client()
        for index, request in enumerate((
            {"action": "engine_set", "engine": "qwen3-asr-1.7b"},
            {"action": "engine_set", "engine": "glm-asr"},
            {"action": "engine_set", "engine": "alibaba-asr"},
            {"action": "invoke", "command": "start_recording"},
            {"action": "get_ai_polish_api_key"},
        ), 100):
            client.send({"request_id": index, **request})
            reply = client.receive()
            self.assertFalse(reply.get("success"), reply)
            self.assertEqual(reply.get("request_id"), index)
            self.assertFalse(native.reaped(), "invalid request must preserve exact live native sentinel")
            self.assertEqual(self.audit("command"), commands, "invalid request must not send native commands")
            self.assertEqual((self.data / "engine.json").read_bytes(), before)
        client.send({"request_id": 106, "action": "exit"})
        self.assertTrue(client.receive().get("success"))
        client.wait()
        self.assertEqual((self.data / "engine.json").read_bytes(), before)
        self.assertFalse(native.reaped(), "broker exit must preserve exact native sentinel")
        self.assertEqual(self.audit("command"), commands)
        self.assertTrue(self.call("status").get("success"), "local broker exit never exits host")
        self.finish(host)


def main():
    global OPTIONS
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app-exe", type=Path, required=True)
    parser.add_argument("--fixture-exe", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--case", action="append", default=[])
    OPTIONS = parser.parse_args()
    if os.name != "nt":
        parser.error("native host verification requires Windows")
    OPTIONS.app_exe = OPTIONS.app_exe.resolve(strict=True)
    OPTIONS.fixture_exe = OPTIONS.fixture_exe.resolve(strict=True)
    OPTIONS.evidence = OPTIONS.evidence.resolve()
    # Explicit own-build path plus source guard avoids ever launching old GUI apps.
    if OPTIONS.app_exe.parent.name != "debug" or OPTIONS.app_exe.name != "light-whisper.exe":
        parser.error("--app-exe must be the explicitly built task debug/light-whisper.exe")
    source = (REPO / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
    guard, _, _ = source.partition("tauri::Builder::default()")
    if "--agent-host" not in guard or "--agent-client" not in guard:
        parser.error("owning source lacks verified early agent flags; refusing GUI launch")
    OPTIONS.evidence.mkdir(parents=True, exist_ok=True)
    identity = {"app_sha256": hashlib.sha256(OPTIONS.app_exe.read_bytes()).hexdigest(),
                "fixture_sha256": hashlib.sha256(OPTIONS.fixture_exe.read_bytes()).hexdigest(),
                "baseline": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
                "surface": "actual native Wry headless host with synthetic native IPC engine; no inference"}
    (OPTIONS.evidence / "identity.json").write_text(json.dumps(identity, indent=2), encoding="utf-8")
    names = OPTIONS.case or unittest.defaultTestLoader.getTestCaseNames(NativeHost)
    suite = unittest.TestSuite(NativeHost(name) for name in names)
    # Give this host its own resource tree. A developer build may contain a real
    # engine archive, which the installer would select before an unpacked engine.
    with tempfile.TemporaryDirectory(prefix="lw-native-bundle-") as bundle:
        engine_dir = Path(bundle) / "resources" / "engine"
        engine_dir.mkdir(parents=True)
        shutil.copy2(OPTIONS.fixture_exe, engine_dir / "engine.exe")
        OPTIONS.app_exe = Path(shutil.copy2(OPTIONS.app_exe, bundle))
        result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
