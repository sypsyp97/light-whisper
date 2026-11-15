import contextlib
import importlib
import io
import json
import os
from pathlib import Path
import queue
import runpy
import signal
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
MODULE = "scripts.light_whisper_cli"
BACKEND_PYTHON = getattr(sys, "_base_executable", sys.executable)
ENV_KEYS = (
    "OMP_NUM_THREADS", "MKL_NUM_THREADS", "OPENBLAS_NUM_THREADS",
    "HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE", "LIGHT_WHISPER_DATA_DIR",
)
BACKEND = r'''
import json, os, sys, threading

sys.stdout.reconfigure(encoding="utf-8")
sys.stdin.reconfigure(encoding="utf-8")

mode = os.environ.get("CLI_TEST_MODE", "normal")
record = os.environ["CLI_TEST_RECORD"]
def emit(value):
    print(json.dumps(value, ensure_ascii=False), flush=True)
def note(value):
    with open(record, "a", encoding="utf-8") as stream:
        stream.write(json.dumps(value) + "\n")
note({"event": "spawn", "pid": os.getpid()})
if mode == "startup_eof":
    sys.exit(17)
if mode == "startup_closed_stdout":
    os.close(sys.stdout.fileno())
    threading.Event().wait()
if mode == "startup_closed_stdin":
    os.close(sys.stdin.fileno())
if mode != "startup_timeout":
    print("native backend diagnostic", flush=True)
    print("{broken json", flush=True)
    emit(["not a response"])
    emit({"event": "loading"})
    emit({"success": mode != "startup_false", "phase": "startup"})
if mode == "startup_then_exit":
    sys.exit(0)
if mode in ("nonreading_stdin", "startup_closed_stdin"):
    threading.Event().wait()
for line in sys.stdin:
    request = json.loads(line)
    note({"event": "request", "request": request})
    if request["action"] == "exit":
        if mode == "ignore_exit":
            threading.Event().wait()
        emit({"success": True, "request_id": request.get("request_id")})
        break
    if mode == "request_eof":
        sys.exit(18)
    if mode == "request_closed_stdout":
        os.close(sys.stdout.fileno())
        threading.Event().wait()
    if mode == "request_timeout":
        threading.Event().wait()
    if mode == "noise":
        print("inference diagnostic", flush=True)
        emit(None)
        emit({"success": True})
        emit({"success": True, "request_id": request["request_id"] - 1,
              "wrong": "stale"})
        emit({"success": True, "request_id": request["request_id"] + 1,
              "wrong": "future"})
    data_dir = os.environ.get("LIGHT_WHISPER_DATA_DIR", "")
    result = {
        "success": mode != "request_false", "request_id": request["request_id"],
        "request": request, "argv": sys.argv[1:],
        "environment": {key: os.environ.get(key) for key in (
            "OMP_NUM_THREADS", "MKL_NUM_THREADS", "OPENBLAS_NUM_THREADS",
            "HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE", "LIGHT_WHISPER_DATA_DIR")},
        "data_dir_exists": os.path.isdir(data_dir), "text": "真实 JSONL ✓",
    }
    if not result["success"]:
        result["error"] = "backend refused request"
        result["detail"] = {"code": 37}
    emit(result)
'''


def setUpModule():
    global cli
    cli = importlib.import_module(MODULE)


def cleanup_fault_probe(mode, script, record, report):
    module = importlib.import_module(MODULE)
    original_popen = subprocess.Popen
    original_wait = original_popen.wait
    original_start = threading.Thread.start
    owned = []
    injected = False
    error = None
    code = None
    output = io.StringIO()
    environment = {**os.environ, "CLI_TEST_MODE": "ignore_exit", "CLI_TEST_RECORD": record}
    client = module.EngineClient([BACKEND_PYTHON, script], timeout=.3, env=environment)
    log = Path(report + ".engine.log").open("ab")

    def capture(*args, **kwargs):
        kwargs["stderr"] = log
        child = original_popen(*args, **kwargs)
        owned.append(child)
        return child

    def start(worker, *args, **kwargs):
        nonlocal injected
        if not injected:
            injected = True
            if mode == "sender_start_error":
                raise RuntimeError("injected sender launch failure")
            raise KeyboardInterrupt("injected cleanup interruption")
        return original_start(worker, *args, **kwargs)

    def wait(child, *args, **kwargs):
        nonlocal injected
        if not injected and child in owned:
            injected = True
            raise KeyboardInterrupt("injected cleanup interruption")
        return original_wait(child, *args, **kwargs)

    def launch(command, timeout, env=None):
        return client

    start_time = time.monotonic()
    try:
        with mock.patch.object(subprocess, "Popen", side_effect=capture):
            if mode == "main_wait_interrupt":
                with mock.patch.object(original_popen, "wait", new=wait), \
                     mock.patch.object(module, "EngineClient", side_effect=launch), \
                     contextlib.redirect_stdout(output):
                    code = module.main(["--python", BACKEND_PYTHON, "--timeout", ".3",
                                        "--data-dir", str(Path(report).parent / "probe-data"), "status"])
            else:
                client.__enter__()
                start_time = time.monotonic()
                patch = (mock.patch.object(original_popen, "wait", new=wait)
                         if mode == "wait_interrupt" else
                         mock.patch.object(threading.Thread, "start", new=start))
                with patch:
                    client.__exit__(None, None, None)
    except BaseException as caught:
        error = caught
    result = {
        "injected": injected, "elapsed": time.monotonic() - start_time,
        "error_type": type(error).__name__ if error is not None else None,
        "error_message": str(error) if error is not None else None,
        "reaped": bool(owned) and all(child.poll() is not None for child in owned),
        "pipes_closed": bool(owned) and all(child.stdin.closed and child.stdout.closed for child in owned),
        "code": code, "stdout": output.getvalue(),
    }
    Path(report).write_text(json.dumps(result), encoding="utf-8")
    for child in owned:
        if child.poll() is None:
            child.kill()
        original_wait(child, timeout=2)
        for pipe in (child.stdin, child.stdout):
            try:
                pipe.close()
            except OSError:
                pass
    log.close()


class BackendCase(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lw-cli-test-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        environment = mock.patch.dict(os.environ, {
            "APPDATA": str(self.directory / "roaming"),
            "LIGHT_WHISPER_DATA_DIR": str(self.directory / "desktop-config"),
        })
        environment.start()
        self.addCleanup(environment.stop)
        self.script = self.directory / "fake engine.py"
        self.script.write_text(textwrap.dedent(BACKEND), encoding="utf-8")
        self.record = self.directory / "requests.jsonl"
        self.owned_children = []
        self.child_logs = []
        original_popen = subprocess.Popen
        def capture_child(*args, **kwargs):
            if kwargs.get("stderr") is None:
                log = (self.directory / f"child-{len(self.child_logs)}.stderr.log").open("ab")
                self.child_logs.append(log)
                kwargs["stderr"] = log
            child = original_popen(*args, **kwargs)
            self.owned_children.append(child)
            return child
        capture = mock.patch.object(subprocess, "Popen", side_effect=capture_child)
        capture.start()
        self.addCleanup(capture.stop)
        self.addCleanup(self.cleanup_children)

    def cleanup_children(self):
        for child in self.owned_children:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=2)
            for pipe in (child.stdin, child.stdout, child.stderr):
                if pipe is not None:
                    try:
                        pipe.close()
                    except OSError:
                        pass
        for log in self.child_logs:
            log.close()

    def environment(self, mode="normal"):
        return {**os.environ, "CLI_TEST_MODE": mode,
                "CLI_TEST_RECORD": str(self.record)}

    def client(self, mode="normal", timeout=.3):
        return cli.EngineClient([BACKEND_PYTHON, str(self.script)], timeout=timeout,
                                env=self.environment(mode))

    def recorded(self):
        return [json.loads(line) for line in self.record.read_text(encoding="utf-8").splitlines()]

    def assert_reaped(self, client):
        self.assertIsNotNone(client.child)
        self.assertIsNotNone(client.child.poll(), "EngineClient returned with a live child")
        self.assertEqual(client.child.wait(timeout=.1), client.child.returncode)


class EngineClientTests(BackendCase):
    def test_popen_failure_preserves_original_error_without_cleanup(self):
        client = self.client()
        expected = OSError("injected process launch failure")
        caught = None
        try:
            with mock.patch.object(subprocess, "Popen", side_effect=expected):
                with client:
                    self.fail("failed process launch admitted the context")
        except BaseException as error:
            caught = error
        self.assertIs(caught, expected)
        self.assertIsNone(client.child)
        self.assertEqual(self.owned_children, [])

    def test_reader_launch_failures_preserve_error_and_reap_acquired_child(self):
        for boundary in ("construction", "start"):
            for error_type in (RuntimeError, KeyboardInterrupt):
                with self.subTest(boundary=boundary, error=error_type.__name__):
                    original_thread = threading.Thread
                    original_start = original_thread.start
                    expected = error_type("injected reader launch failure")
                    injected = False
                    def fail_once(*args, **kwargs):
                        nonlocal injected
                        if not injected:
                            injected = True
                            raise expected
                        return (original_thread(*args, **kwargs) if boundary == "construction"
                                else original_start(*args, **kwargs))
                    patch = (mock.patch.object(threading, "Thread", side_effect=fail_once)
                             if boundary == "construction" else
                             mock.patch.object(original_thread, "start", new=fail_once))
                    client = self.client()
                    caught = None
                    start_time = time.monotonic()
                    try:
                        with patch:
                            with client:
                                self.fail("reader launch failure admitted the context")
                    except BaseException as error:
                        caught = error
                    self.assertTrue(injected)
                    self.assertIs(caught, expected)
                    self.assertLessEqual(time.monotonic() - start_time, 3)
                    self.assert_reaped(client)
                    self.assertTrue(client.child.stdin.closed)
                    self.assertTrue(client.child.stdout.closed)

    def cleanup_probe(self, mode):
        report = self.directory / (mode + ".json")
        log_path = self.directory / (mode + ".worker.log")
        command = [BACKEND_PYTHON, "-c",
                   "import sys;from scripts.test_light_whisper_cli import cleanup_fault_probe;"
                   "cleanup_fault_probe(*sys.argv[1:])",
                   mode, str(self.script), str(self.record), str(report)]
        with log_path.open("ab") as log:
            worker = subprocess.Popen(command, stdout=log, stderr=log, cwd=ROOT,
                                      start_new_session=os.name != "nt")
            try:
                worker.wait(timeout=3.5)
            except subprocess.TimeoutExpired:
                if os.name == "nt":
                    taskkill = Path(os.environ["SystemRoot"]) / "System32" / "taskkill.exe"
                    subprocess.run([str(taskkill), "/PID", str(worker.pid), "/T", "/F"],
                                   stdout=log, stderr=log, timeout=3,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
                else:
                    os.killpg(worker.pid, signal.SIGKILL)
                if worker.poll() is None:
                    worker.kill()
                worker.wait(timeout=2)
                self.fail(f"{mode}: cleanup watchdog expired with an owned child still live")
        self.assertEqual(worker.returncode, 0, log_path.read_text(encoding="utf-8", errors="replace"))
        return json.loads(report.read_text(encoding="utf-8"))

    def test_cleanup_faults_preserve_error_and_force_reap(self):
        for mode, expected_type, expected_message in (
            ("send_interrupt", "KeyboardInterrupt", "injected cleanup interruption"),
            ("wait_interrupt", "KeyboardInterrupt", "injected cleanup interruption"),
            ("sender_start_error", "RuntimeError", "injected sender launch failure"),
        ):
            with self.subTest(mode=mode):
                result = self.cleanup_probe(mode)
                self.assertTrue(result["injected"])
                self.assertEqual(result["error_type"], expected_type)
                self.assertEqual(result["error_message"], expected_message)
                self.assertTrue(result["reaped"], result)
                self.assertTrue(result["pipes_closed"], result)
                self.assertLessEqual(result["elapsed"], 3)

    def test_main_cleanup_interrupt_returns_one_json_and_reaps(self):
        result = self.cleanup_probe("main_wait_interrupt")
        self.assertTrue(result["injected"])
        self.assertIsNone(result["error_type"])
        self.assertEqual(result["code"], 130)
        self.assertEqual(len(result["stdout"].splitlines()), 1)
        response = json.loads(result["stdout"])
        self.assertFalse(response["success"])
        self.assertTrue(response["error"].strip())
        self.assertTrue(result["reaped"], result)
        self.assertTrue(result["pipes_closed"], result)
        self.assertLessEqual(result["elapsed"], 3)

    def test_startup_noise_correlation_and_increasing_request_identity(self):
        client = self.client("noise")
        with client as entered:
            self.assertIs(entered, client)
            first = client.request("status", marker="first")
            second = client.request("stats", marker="second")
            self.assertEqual(first["request"], {"action": "status", "marker": "first",
                                                "request_id": first["request_id"]})
            self.assertEqual(second["request"]["action"], "stats")
            self.assertEqual(second["request"]["marker"], "second")
            self.assertIs(type(first["request_id"]), int)
            self.assertGreater(first["request_id"], 0)
            self.assertGreater(second["request_id"], first["request_id"])
            self.assertNotIn("wrong", first)
            self.assertNotIn("wrong", second)
            self.assertEqual(first["text"], "真实 JSONL ✓")
        self.assert_reaped(client)
        self.assertEqual(self.recorded()[-1]["request"]["action"], "exit")

    def test_request_identity_correlation_outlives_small_integer_cache(self):
        previous = 0
        with self.client(timeout=1) as client:
            for index in range(260):
                response = client.request("status", marker=str(index))
                self.assertEqual(response["request"]["marker"], str(index))
                self.assertIs(type(response["request_id"]), int)
                self.assertGreater(response["request_id"], previous)
                previous = response["request_id"]
        self.assert_reaped(client)

    def test_matching_failure_response_is_preserved(self):
        with self.client("request_false") as client:
            response = client.request("transcribe", audio_path="example.wav",
                                      options={"language": "de"})
            self.assertFalse(response["success"])
            self.assertEqual(response["error"], "backend refused request")
            self.assertEqual(response["detail"], {"code": 37})
            self.assertEqual(response["request"]["options"], {"language": "de"})
        self.assert_reaped(client)

    def test_completed_requests_release_the_sender_before_handoff_and_exit(self):
        for actions in (("status",), ("status", "stats")):
            with self.subTest(actions=actions):
                client = self.client(timeout=1)
                original_put = queue.Queue.put
                original_join = threading.Thread.join
                tail_published = threading.Event()
                release_tail = threading.Event()
                held = []
                live_at_publication = []

                def put(messages, item, *args, **kwargs):
                    if threading.current_thread() is client.sender and item is None and not held:
                        held.append(threading.current_thread())
                        result = original_put(messages, item, *args, **kwargs)
                        tail_published.set()
                        release_tail.wait()
                        return result
                    return original_put(messages, item, *args, **kwargs)

                def join(worker, timeout=None):
                    if worker in held:
                        self.assertIsNotNone(timeout, "sender handoff used an unbounded wait")
                        self.assertTrue(tail_published.wait(timeout=1),
                                        "sender never published its completed write")
                        release_tail.set()
                    return original_join(worker, timeout=timeout)

                try:
                    with mock.patch.object(queue.Queue, "put", new=put), \
                         mock.patch.object(threading.Thread, "join", new=join):
                        with client:
                            for action in actions:
                                response = client.request(action)
                                self.assertTrue(response["success"])
                                self.assertTrue(tail_published.wait(timeout=1),
                                                "sender tail schedule was not reached")
                                live_at_publication.append(any(worker.is_alive() for worker in held))
                        self.assertFalse(any(live_at_publication),
                                         "request completed while its prior sender still owned a live tail")
                        self.assertEqual(self.recorded()[-1]["request"]["action"], "exit")
                        self.assertEqual(client.child.returncode, 0,
                                         "cooperative child required forced termination after a completed request")
                        self.assert_reaped(client)
                        self.assertTrue(client.child.stdin.closed)
                        self.assertTrue(client.child.stdout.closed)
                finally:
                    release_tail.set()
                    for worker in held:
                        original_join(worker, timeout=1)

    def test_sender_tail_timeout_uses_the_remaining_request_budget_and_reaps(self):
        for dispatch_delay in (.1, 1):
            with self.subTest(dispatch_delay=dispatch_delay):
                client = self.client(timeout=.3)
                original_put = queue.Queue.put
                original_get = queue.Queue.get
                original_join = threading.Thread.join
                tail_published = threading.Event()
                release_tail = threading.Event()
                requesting = threading.Event()
                held = []
                now = [100.0]
                caught = None

                def put(messages, item, *args, **kwargs):
                    if threading.current_thread() is client.sender and item is None and not held:
                        held.append(threading.current_thread())
                        result = original_put(messages, item, *args, **kwargs)
                        tail_published.set()
                        release_tail.wait()
                        return result
                    return original_put(messages, item, *args, **kwargs)

                def get(messages, *args, **kwargs):
                    result = original_get(messages, *args, **kwargs)
                    if messages is not client.messages:
                        self.assertTrue(tail_published.wait(timeout=1),
                                        "sender never reached its published-result barrier")
                        now[0] += dispatch_delay
                    return result

                def join(worker, timeout=None):
                    if worker in held:
                        self.assertIsNotNone(timeout, "sender handoff used an unbounded wait")
                        if requesting.is_set():
                            now[0] += max(0, timeout)
                            return original_join(worker, timeout=0)
                        release_tail.set()
                    return original_join(worker, timeout=timeout)

                try:
                    with mock.patch.object(queue.Queue, "put", new=put), \
                         mock.patch.object(threading.Thread, "join", new=join):
                        with client:
                            with mock.patch.object(queue.Queue, "get", new=get), \
                                 mock.patch.object(time, "monotonic", side_effect=lambda: now[0]):
                                requesting.set()
                                try:
                                    client.request("status")
                                except Exception as error:
                                    caught = error
                                finally:
                                    requesting.clear()
                                self.assertTrue(tail_published.is_set(),
                                                "sender tail schedule was not reached")
                                self.assertIs(type(caught), TimeoutError,
                                              f"request did not time out with a live sender tail: {caught!r}")
                                self.assertTrue(str(caught).strip())
                                self.assertLessEqual(now[0] - 100, max(.3, dispatch_delay) + .000001,
                                                     "sender handoff extended the request deadline")
                        self.assert_reaped(client)
                        self.assertTrue(client.child.stdin.closed)
                        self.assertTrue(client.child.stdout.closed)
                finally:
                    release_tail.set()
                    for worker in held:
                        original_join(worker, timeout=1)

    def test_startup_failure_eof_and_timeout_reap_acquired_process(self):
        cases = (("startup_false", RuntimeError), ("startup_eof", EOFError),
                 ("startup_closed_stdout", EOFError), ("startup_timeout", TimeoutError))
        timeout = .3
        for mode, expected in cases:
            with self.subTest(mode=mode):
                client = None
                entered = False
                caught = None
                start = time.monotonic()
                try:
                    client = self.client(mode, timeout=timeout)
                    with client:
                        entered = True
                except Exception as error:
                    caught = error
                elapsed = time.monotonic() - start
                self.assertIs(type(caught), expected, f"wrong startup error kind: {caught!r}")
                self.assertTrue(str(caught).strip(), "startup error lacks a useful message")
                self.assertLessEqual(elapsed, timeout + 2 + 1,
                                     "startup failure exceeded deadline plus bounded cleanup")
                self.assertFalse(entered, "failed startup admitted requests")
                self.assert_reaped(client)

    def test_request_eof_and_timeout_raise_and_reap(self):
        cases = (("request_eof", EOFError), ("request_closed_stdout", EOFError),
                 ("request_timeout", TimeoutError))
        timeout = .3
        for mode, expected in cases:
            with self.subTest(mode=mode):
                client = None
                returned = False
                caught = None
                start = time.monotonic()
                try:
                    client = self.client(mode, timeout=timeout)
                    with client:
                        client.request("status")
                        returned = True
                except Exception as error:
                    caught = error
                elapsed = time.monotonic() - start
                self.assertIs(type(caught), expected, f"wrong request error kind: {caught!r}")
                self.assertTrue(str(caught).strip(), "request error lacks a useful message")
                self.assertLessEqual(elapsed, timeout + 2 + 1,
                                     "request failure exceeded deadline plus bounded cleanup")
                self.assertFalse(returned, "failed request returned a response")
                self.assert_reaped(client)

    def test_user_exception_survives_context_cleanup(self):
        class CallerFailure(Exception):
            pass
        client = self.client()
        with self.assertRaisesRegex(CallerFailure, "caller failed"):
            with client:
                raise CallerFailure("caller failed")
        self.assert_reaped(client)
        self.assertEqual(self.recorded()[-1]["request"]["action"], "exit")

    def test_request_before_startup_and_after_exit_is_rejected(self):
        client = self.client()
        with self.assertRaises(RuntimeError):
            client.request("status")
        self.assertIsNone(client.child)
        with client:
            self.assertTrue(client.request("status")["success"])
        with self.assertRaises(RuntimeError):
            client.request("stats")
        self.assert_reaped(client)
        self.assertEqual(self.recorded()[-1]["request"]["action"], "exit")

    def test_already_exited_child_is_reaped_without_sending_exit(self):
        client = self.client("startup_then_exit")
        with client:
            self.assertEqual(client.child.wait(timeout=1), 0)
        self.assert_reaped(client)
        self.assertEqual([item["event"] for item in self.recorded()], ["spawn"])

    def test_nonreading_stdin_obeys_request_deadline_and_reaps(self):
        timeout = .3
        client = self.client("nonreading_stdin", timeout=timeout)
        with client:
            start = time.monotonic()
            with self.assertRaises(TimeoutError) as raised:
                client.request("transcribe", options={"context": "x" * (1024 * 1024)})
        self.assertTrue(str(raised.exception).strip())
        self.assertLessEqual(time.monotonic() - start, timeout + 2 + 1)
        self.assert_reaped(client)

    def test_matching_response_dequeued_after_deadline_is_rejected(self):
        timeout = .3
        client = self.client(timeout=timeout)
        expired = threading.Event()
        original_get = queue.Queue.get
        original_clock = time.monotonic
        def late_delivery(messages, *args, **kwargs):
            item = original_get(messages, *args, **kwargs)
            if isinstance(item, str):
                try:
                    response = json.loads(item)
                except json.JSONDecodeError:
                    pass
                else:
                    if isinstance(response, dict) and response.get("request", {}).get("marker") == "late":
                        expired.set()
            return item
        def clock():
            return original_clock() + (timeout + 1 if expired.is_set() else 0)
        with client:
            with mock.patch.object(queue.Queue, "get", new=late_delivery), \
                 mock.patch.object(time, "monotonic", side_effect=clock):
                with self.assertRaises(TimeoutError):
                    client.request("status", marker="late")
        self.assertTrue(expired.is_set(), "schedule never delivered the matching response")
        self.assert_reaped(client)

    def test_response_timeout_uses_only_the_remaining_request_budget(self):
        for dispatch_delay in (0, 1):
            with self.subTest(dispatch_delay=dispatch_delay):
                client = self.client("request_timeout", timeout=.3)
                original_get = queue.Queue.get
                now = [100.0]

                def scheduled_get(messages, *args, **kwargs):
                    if messages is client.messages:
                        timeout = kwargs["timeout"]
                        if timeout < 0:
                            return original_get(messages, *args, **kwargs)
                        now[0] += timeout
                        raise queue.Empty
                    result = original_get(messages, *args, **kwargs)
                    now[0] += dispatch_delay
                    return result

                with client:
                    with mock.patch.object(queue.Queue, "get", new=scheduled_get), \
                         mock.patch.object(time, "monotonic", side_effect=lambda: now[0]):
                        with self.assertRaises(TimeoutError):
                            client.request("status")
                        self.assertLessEqual(now[0] - 100, max(.3, dispatch_delay) + .000001,
                                             "response wait spent more than the request budget")
                self.assert_reaped(client)

    def test_expired_receive_deadline_after_sender_quiescence(self):
        for handoff_delay in (.125, 1):
            with self.subTest(handoff_delay=handoff_delay):
                timeout = .3
                client = self.client("request_timeout", timeout=timeout)
                original_get = queue.Queue.get
                original_join = threading.Thread.join
                now = [100.0]
                sender_stopped = threading.Event()
                receive_waits = []
                caught = None

                def join(worker, timeout=None):
                    result = original_join(worker, timeout=timeout)
                    if worker is client.sender:
                        self.assertFalse(worker.is_alive(), "sender did not finish before receive")
                        sender_stopped.set()
                        now[0] += handoff_delay
                    return result

                def scheduled_get(messages, *args, **kwargs):
                    if messages is client.messages:
                        self.assertTrue(sender_stopped.is_set(), "receive began before sender joined")
                        wait = kwargs["timeout"]
                        receive_waits.append(wait)
                        if wait < 0:
                            return original_get(messages, *args, **kwargs)
                        now[0] += wait
                        raise queue.Empty
                    return original_get(messages, *args, **kwargs)

                with client:
                    with mock.patch.object(threading.Thread, "join", new=join), \
                         mock.patch.object(queue.Queue, "get", new=scheduled_get), \
                         mock.patch.object(time, "monotonic", side_effect=lambda: now[0]):
                        try:
                            client.request("status")
                        except Exception as error:
                            caught = error
                self.assert_reaped(client)
                self.assertTrue(client.child.stdin.closed)
                self.assertTrue(client.child.stdout.closed)
                self.assertTrue(sender_stopped.is_set(), "sender handoff schedule was not reached")
                self.assertTrue(receive_waits, "request did not reach the receive wait")
                self.assertIs(type(caught), TimeoutError,
                              f"receive deadline produced the wrong error: {caught!r}")
                self.assertLessEqual(now[0] - 100, max(timeout, handoff_delay) + .000001,
                                     "receive wait extended the original request deadline")

    def test_blocked_sender_timeout_and_expired_deadline_are_timeout_errors(self):
        for dispatch_delay in (0, 1):
            with self.subTest(dispatch_delay=dispatch_delay):
                client = self.client("nonreading_stdin", timeout=.3)
                original_get = queue.Queue.get
                original_start = threading.Thread.start
                now = [100.0]
                sender_started = threading.Event()

                def start(worker, *args, **kwargs):
                    result = original_start(worker, *args, **kwargs)
                    if worker is client.sender:
                        sender_started.set()
                        now[0] += dispatch_delay
                    return result

                def scheduled_get(messages, *args, **kwargs):
                    if messages is client.messages:
                        return original_get(messages, *args, **kwargs)
                    timeout = kwargs["timeout"]
                    if timeout < 0:
                        return original_get(messages, *args, **kwargs)
                    try:
                        return original_get(messages, block=False)
                    except queue.Empty:
                        now[0] += timeout
                        raise

                with client:
                    with mock.patch.object(threading.Thread, "start", new=start), \
                         mock.patch.object(queue.Queue, "get", new=scheduled_get), \
                         mock.patch.object(time, "monotonic", side_effect=lambda: now[0]):
                        with self.assertRaises(TimeoutError):
                            client.request("transcribe", options={"context": "x" * (1024 * 1024)})
                        self.assertTrue(sender_started.is_set(), "sender dispatch schedule was not reached")
                        self.assertLessEqual(now[0] - 100, max(.3, dispatch_delay) + .000001,
                                             "stdin wait extended the request deadline")
                self.assert_reaped(client)

    def test_closed_backend_stdin_is_eof_and_reaps_the_live_process(self):
        client = self.client("startup_closed_stdin")
        caught = None
        try:
            with client:
                self.assertIsNone(client.child.poll(), "backend must remain alive after closing stdin")
                client.request("status")
        except Exception as error:
            caught = error
        self.assertIs(type(caught), EOFError, f"wrong broken-stdin error kind: {caught!r}")
        self.assertTrue(str(caught).strip())
        self.assert_reaped(client)

    def test_cleanup_worker_wait_respects_total_and_exhausted_budget(self):
        for reap_dispatch_delay, worker_can_finish in ((0, False), (3, False), (0, True)):
            with self.subTest(reap_dispatch_delay=reap_dispatch_delay, worker_can_finish=worker_can_finish):
                client = self.client("startup_then_exit")
                original_put = queue.Queue.put
                original_wait = subprocess.Popen.wait
                original_join = threading.Thread.join
                reader_at_eof = threading.Event()
                release_reader = threading.Event()
                now = [100.0]

                def put(messages, item, *args, **kwargs):
                    if messages is client.messages and item is None:
                        reader_at_eof.set()
                        release_reader.wait()
                    return original_put(messages, item, *args, **kwargs)

                def wait(child, *args, **kwargs):
                    result = original_wait(child, *args, **kwargs)
                    if child is client.child:
                        now[0] += reap_dispatch_delay
                    return result

                def join(worker, timeout=None):
                    if worker is client.reader:
                        if worker_can_finish:
                            until_runnable = max(0, 101.5 - now[0])
                            allotted = max(0, timeout)
                            now[0] += min(allotted, until_runnable)
                            if allotted >= until_runnable:
                                release_reader.set()
                                return original_join(worker, timeout=1)
                            return original_join(worker, timeout=0)
                        original_join(worker, timeout=0)
                        if worker.is_alive():
                            now[0] += max(0, timeout)
                        return
                    return original_join(worker, timeout=timeout)

                with mock.patch.object(queue.Queue, "put", new=put):
                    try:
                        client.__enter__()
                        child_type = type(client.child)
                        original_wait = child_type.wait
                        self.assertTrue(reader_at_eof.wait(timeout=1), "reader did not reach the EOF barrier")
                        client.child.wait(timeout=1)
                        self.assertTrue(client.reader.is_alive(), "cleanup needs an owned live worker")
                        with mock.patch.object(child_type, "wait", new=wait), \
                             mock.patch.object(threading.Thread, "join", new=join), \
                             mock.patch.object(time, "monotonic", side_effect=lambda: now[0]):
                            client.__exit__(None, None, None)
                        if worker_can_finish:
                            self.assertFalse(client.reader.is_alive(), "cleanup returned before the reader quiesced")
                        self.assertLessEqual(now[0] - 100, max(2, reap_dispatch_delay),
                                             "worker wait extended the overall cleanup budget")
                    finally:
                        release_reader.set()
                        if client.reader is not None:
                            client.reader.join(timeout=1)
                self.assert_reaped(client)
                self.assertTrue(client.child.stdin.closed)
                self.assertTrue(client.child.stdout.closed)

    def test_post_kill_wait_and_graceful_stages_share_the_cleanup_budget(self):
        for reap_delay, dispatch_delay in ((1.5, 0), (3, 0), (1.5, 3)):
            with self.subTest(reap_delay=reap_delay, dispatch_delay=dispatch_delay):
                client = self.client("ignore_exit")
                client.__enter__()
                child_type = type(client.child)
                original_wait = child_type.wait
                original_kill = child_type.kill
                original_get = queue.Queue.get
                killed = threading.Event()
                now = [100.0]
                before_reap = [0]
                caught = None

                def get(messages, *args, **kwargs):
                    result = original_get(messages, *args, **kwargs)
                    now[0] += .9 * kwargs["timeout"]
                    return result

                def kill(child):
                    result = original_kill(child)
                    killed.set()
                    return result

                def wait(child, *args, **kwargs):
                    timeout = kwargs["timeout"]
                    if not killed.is_set():
                        self.assertIsNone(child.poll(), "stubborn backend exited before forced cleanup")
                        now[0] += max(0, timeout) + dispatch_delay
                        before_reap[0] = now[0] - 100
                        raise subprocess.TimeoutExpired(child.args, timeout)
                    result = original_wait(child, timeout=1)
                    allotted = max(0, timeout)
                    now[0] += min(allotted, reap_delay)
                    if allotted < reap_delay:
                        raise subprocess.TimeoutExpired(child.args, timeout)
                    return result

                with mock.patch.object(child_type, "wait", new=wait), \
                     mock.patch.object(child_type, "kill", new=kill), \
                     mock.patch.object(queue.Queue, "get", new=get), \
                     mock.patch.object(time, "monotonic", side_effect=lambda: now[0]):
                    try:
                        client.__exit__(None, None, None)
                    except BaseException as error:
                        caught = error
                self.assertTrue(killed.is_set(), "stubborn backend did not reach forced cleanup")
                if reap_delay == 1.5 and dispatch_delay == 0:
                    self.assertIsNone(caught, f"cleanup abandoned a reap that fit its remaining budget: {caught!r}")
                else:
                    self.assertIs(type(caught), subprocess.TimeoutExpired)
                self.assertLessEqual(now[0] - 100, max(2, before_reap[0] if dispatch_delay else 0) + .000001,
                                     "cleanup stages extended the shared two-second budget")
                self.assert_reaped(client)
                self.assertNotEqual(client.child.returncode, 0)
                self.assertTrue(client.child.stdin.closed)
                self.assertTrue(client.child.stdout.closed)

    def test_uncooperative_exit_is_killed_and_reaped_with_bounded_cleanup(self):
        client = self.client("ignore_exit", timeout=.3)
        start = time.monotonic()
        with client:
            self.assertTrue(client.request("status")["success"])
        self.assertLess(time.monotonic() - start, 3,
                        "cleanup must cap its own wait at <=2 seconds")
        self.assert_reaped(client)
        self.assertNotEqual(client.child.returncode, 0)
        self.assertEqual(self.recorded()[-1]["request"]["action"], "exit")

    @unittest.skipUnless(os.name == "nt", "Windows process window contract")
    def test_windows_child_is_hidden(self):
        original = subprocess.Popen
        flags = []
        def capture(*args, **kwargs):
            flags.append(kwargs.get("creationflags", 0))
            return original(*args, **kwargs)
        with mock.patch.object(subprocess, "Popen", side_effect=capture):
            with self.client() as client:
                self.assertTrue(client.request("status")["success"])
        self.assertTrue(flags[0] & subprocess.CREATE_NO_WINDOW)


class CliTests(BackendCase):
    def invoke(self, arguments):
        output, errors = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = cli.main(arguments)
        self.assertIs(type(code), int)
        lines = output.getvalue().splitlines()
        self.assertEqual(len(lines), 1, (output.getvalue(), errors.getvalue()))
        result = json.loads(lines[0])
        self.assertIsInstance(result, dict)
        self.assertIs(type(result.get("success")), bool)
        return code, result

    def launch_fake(self, mode="normal"):
        original = cli.EngineClient
        self.launches = []
        self.clients = []
        def launch(command, timeout, env=None):
            self.launches.append((list(command), timeout, dict(env or {})))
            client = original([BACKEND_PYTHON, str(self.script)], timeout=timeout,
                              env={**(env or {}), "CLI_TEST_MODE": mode,
                                   "CLI_TEST_RECORD": str(self.record)})
            self.clients.append(client)
            return client
        return mock.patch.object(cli, "EngineClient", side_effect=launch)

    def test_doctor_reports_real_version_and_paths_without_spawning_or_writing(self):
        version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
        with mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")), \
             mock.patch.object(tempfile, "TemporaryDirectory", side_effect=AssertionError("write")):
            code, result = self.invoke([json.loads('"doctor"')])
        self.assertEqual(code, 0)
        self.assertTrue(result["success"])
        self.assertIs(result["model_load_checked"], False)
        values = []
        def collect(value):
            if isinstance(value, dict):
                for member in value.values():
                    collect(member)
            elif isinstance(value, list):
                for member in value:
                    collect(member)
            elif isinstance(value, str):
                values.append(value)
        collect(result)
        self.assertIn(version, values)
        self.assertIn(str(ROOT / "src-tauri" / "resources" / "engine.py"), values)
        default_python = ROOT / ".venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
        self.assertIn(str(default_python), values)

    def test_doctor_reports_explicit_runtime_presence_without_loading_models(self):
        for option in ("--python", "--engine-exe"):
            for runtime in (Path(BACKEND_PYTHON), self.directory / "missing runtime.exe"):
                with self.subTest(option=option, runtime=runtime), \
                     mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")), \
                     mock.patch.object(tempfile, "TemporaryDirectory", side_effect=AssertionError("write")):
                    code, result = self.invoke([option, str(runtime), json.loads('"doctor"')])
                    self.assertEqual(code, 0)
                    self.assertTrue(result["success"])
                    self.assertEqual(result["runtime"], str(runtime.resolve()))
                    self.assertIs(result["runtime_present"], runtime.is_file())
                    self.assertIs(result["model_load_checked"], False)

    def test_single_thread_budget_and_nested_explicit_data_directory(self):
        data = self.directory / "missing parent" / "nested" / "data"
        self.assertFalse(data.parent.exists())
        with self.launch_fake():
            code, result = self.invoke(["--python", sys.executable, "--threads", "1",
                                        "--data-dir", str(data), "status"])
        self.assertEqual(code, 0)
        self.assertTrue(result["success"])
        self.assertTrue(result["data_dir_exists"])
        self.assertEqual(result["environment"]["LIGHT_WHISPER_DATA_DIR"], str(data.resolve()))
        self.assertTrue(data.is_dir())
        for key in ENV_KEYS[:3]:
            self.assertEqual(result["environment"][key], "1")
        self.assert_reaped(self.clients[0])

    def test_status_and_stats_use_real_protocol_with_isolated_data(self):
        paths = []
        with self.launch_fake():
            for action in ("status", "stats"):
                code, result = self.invoke(["--python", sys.executable, action])
                self.assertEqual(code, 0)
                self.assertEqual(result["request"]["action"], action)
                self.assertTrue(result["data_dir_exists"])
                paths.append(Path(result["environment"]["LIGHT_WHISPER_DATA_DIR"]))
        self.assertNotEqual(paths[0], paths[1])
        for path, client in zip(paths, self.clients):
            self.assertFalse(path.exists(), "isolated temporary engine data survived the command")
            self.assert_reaped(client)
        for command, timeout, environment in self.launches:
            self.assertEqual(command, [sys.executable,
                str(ROOT / "src-tauri" / "resources" / "engine.py"),
                "serve", "--engine", "qwen3-asr-0.6b"])
            self.assertEqual(timeout, 120)
            for key in ENV_KEYS[:3]:
                self.assertEqual(environment[key], "2")
            for key in ENV_KEYS[3:5]:
                self.assertEqual(environment[key], "1")

    def test_transcribe_forwards_resolved_audio_and_options_and_thread_budget(self):
        audio = self.directory / "audio with spaces.wav"
        audio.write_bytes(b"test-only audio; fake backend does not decode")
        data = self.directory / "explicit data"
        inherited = {key: "99" for key in ENV_KEYS[:3]}
        inherited.update({key: "0" for key in ENV_KEYS[3:5]})
        with self.launch_fake(), mock.patch.dict(os.environ, inherited):
            code, result = self.invoke(["--python", sys.executable,
                "--engine", "confucius4-r2t2", "--threads", "3", "--timeout", "1.5",
                "--data-dir", str(data), json.loads('"transcribe"'), str(audio),
                "--language", "de", "--context", "CT Befund ✓"])
        self.assertEqual(code, 0)
        request = result["request"]
        self.assertEqual(request["action"], "transcribe")
        self.assertEqual(request["audio_path"], str(audio.resolve()))
        self.assertEqual(request["options"]["language"], "de")
        self.assertEqual(request["options"]["context"], "CT Befund ✓")
        command, timeout, environment = self.launches[0]
        self.assertEqual(command[-1], "confucius4-r2t2")
        self.assertEqual(timeout, 1.5)
        self.assertEqual(environment["LIGHT_WHISPER_DATA_DIR"], str(data.resolve()))
        self.assertTrue(data.is_dir())
        for key in ENV_KEYS[:3]:
            self.assertEqual(environment[key], "3")
        for key in ENV_KEYS[3:5]:
            self.assertEqual(environment[key], "1")
        self.assert_reaped(self.clients[0])

    def test_engine_executable_selects_serve_without_python_wrapper(self):
        with self.launch_fake():
            code, _ = self.invoke(["--engine-exe", sys.executable, "status"])
        self.assertEqual(code, 0)
        self.assertEqual(self.launches[0][0], [sys.executable, "serve", "--engine",
                                              "qwen3-asr-0.6b"])

    def test_backend_failure_json_is_preserved_and_returns_one(self):
        with self.launch_fake("request_false"):
            code, result = self.invoke(["--python", sys.executable, "stats"])
        self.assertEqual(code, 1)
        self.assertFalse(result["success"])
        self.assertEqual(result["detail"], {"code": 37})
        self.assertEqual(result["error"], "backend refused request")
        self.assert_reaped(self.clients[0])

    def test_startup_and_request_failures_are_single_json_errors(self):
        for mode in ("startup_false", "startup_eof", "startup_closed_stdout", "startup_timeout",
                     "request_eof", "request_closed_stdout", "request_timeout"):
            with self.subTest(mode=mode), self.launch_fake(mode):
                start = time.monotonic()
                code, result = self.invoke(["--python", sys.executable,
                                            "--timeout", ".3", "status"])
                elapsed = time.monotonic() - start
                self.assertEqual(code, 1)
                self.assertFalse(result["success"])
                self.assertIsInstance(result.get("error"), str)
                self.assertTrue(result["error"].strip())
                self.assertLessEqual(elapsed, .3 + 2 + 1,
                                     "CLI failure exceeded deadline plus bounded cleanup")
                self.assert_reaped(self.clients[0])

    def test_invalid_arguments_are_json_and_never_launch(self):
        cases = [[], ["--python", sys.executable], ["unknown"], ["--engine", "unknown", "status"],
                 ["--python", sys.executable, "--engine-exe", sys.executable, "status"]]
        cases.extend(["--python", sys.executable, "--timeout", value, "status"]
                     for value in ("0", "-1", "nan", "inf", "-inf", "text"))
        cases.extend(["--python", sys.executable, "--threads", value, "status"]
                     for value in ("0", "-1", "1.5", "text"))
        with mock.patch.object(cli, "EngineClient", side_effect=AssertionError("launched")):
            for arguments in cases:
                with self.subTest(arguments=arguments):
                    code, result = self.invoke(arguments)
                    self.assertEqual(code, 2)
                    self.assertFalse(result["success"])
                    self.assertTrue(result.get("error"))

    def test_missing_audio_or_runtime_paths_fail_without_launch(self):
        missing = str(self.directory / "does-not-exist")
        cases = [["--python", sys.executable, "transcribe", missing],
                 ["--python", sys.executable, "transcribe", str(self.directory)],
                 ["--python", missing, "status"],
                 ["--python", str(self.directory), "status"],
                 ["--engine-exe", missing, "status"]]
        with mock.patch.object(cli, "EngineClient", side_effect=AssertionError("launched")):
            for arguments in cases:
                with self.subTest(arguments=arguments):
                    code, result = self.invoke(arguments)
                    self.assertEqual(code, 2)
                    self.assertFalse(result["success"])
                    self.assertTrue(result.get("error"))

    def test_spawn_error_and_keyboard_interrupt_have_json_exit_contract(self):
        for failure, expected in ((OSError("cannot start child"), 1), (KeyboardInterrupt(), 130)):
            with self.subTest(failure=type(failure).__name__), \
                 mock.patch.object(cli, "EngineClient", side_effect=failure):
                code, result = self.invoke(["--python", sys.executable, "status"])
                self.assertEqual(code, expected)
                self.assertFalse(result["success"])
                self.assertTrue(result.get("error"))

    def test_missing_engine_script_is_json_error_without_launch(self):
        missing = self.directory / "missing-engine.py"
        with mock.patch.object(cli, "ENGINE_SCRIPT", missing), \
             mock.patch.object(cli, "EngineClient", side_effect=AssertionError("launched")):
            code, result = self.invoke(["--python", sys.executable, "status"])
        self.assertEqual(code, 2)
        self.assertFalse(result["success"])
        self.assertIn(str(missing), result["error"])

    def test_script_entrypoint_preserves_argument_error_exit_code(self):
        script = ROOT / "scripts" / "light_whisper_cli.py"
        output = io.StringIO()
        with mock.patch.object(sys, "argv", [str(script), "--timeout", "0", "status"]), \
             contextlib.redirect_stdout(output), \
             mock.patch.object(subprocess, "Popen", side_effect=AssertionError("launched")):
            with self.assertRaises(SystemExit) as raised:
                runpy.run_path(str(script), run_name="__main__")
        self.assertEqual(raised.exception.code, 2)
        self.assertEqual(len(output.getvalue().splitlines()), 1)
        result = json.loads(output.getvalue())
        self.assertFalse(result["success"])
        self.assertTrue(result["error"].strip())

    def invoke_script(self, arguments, expected_code):
        completed = subprocess.run(
            [BACKEND_PYTHON, str(ROOT / "scripts" / "light_whisper_cli.py"), *arguments],
            cwd=ROOT, env={**os.environ, "PYTHONIOENCODING": "cp1252"},
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5,
        )
        self.assertEqual(completed.returncode, expected_code,
                         completed.stderr.decode("utf-8", errors="backslashreplace"))
        try:
            result = json.loads(completed.stdout)
        except ValueError as error:
            self.fail(f"CLI did not emit one complete JSON object: {error}; {completed.stdout!r}")
        self.assertIsInstance(result, dict)
        return result

    def test_doctor_preserves_unicode_runtime_with_redirected_cp1252_stdout(self):
        with tempfile.TemporaryDirectory(prefix="lw-cli-stdio-") as directory:
            runtime = Path(directory) / "测试 python.exe"
            runtime.write_bytes(b"test fixture; doctor must not execute this file")
            result = self.invoke_script(["--python", str(runtime), "doctor"], 0)
            self.assertTrue(result["success"])
            self.assertEqual(result["runtime"], str(runtime.resolve()))
            self.assertTrue(result["runtime_present"])
            self.assertFalse(result["model_load_checked"])

    def test_argument_error_preserves_unicode_with_redirected_cp1252_stdout(self):
        invalid = "未知命令"
        result = self.invoke_script([invalid], 2)
        self.assertFalse(result["success"])
        self.assertIn(invalid, result["error"])


if __name__ == "__main__":
    unittest.main()
