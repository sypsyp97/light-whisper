"""Real on/off/unload/resume smoke; unpaced feeds are not microphone latency."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
RESOURCES = ROOT / "src-tauri/resources"


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def run(args, report):
    import numpy as np

    sys.path.insert(0, str(RESOURCES))
    from server_common import BaseASRServer

    audio, source_rate = BaseASRServer._read_mono_audio(str(args.audio))
    audio = BaseASRServer._resample(audio, source_rate)
    require(audio.size > 0 and np.isfinite(audio).all(), "audio must contain finite samples")
    pcm = np.clip(np.rint(audio * 32768), -32768, 32767).astype("<i2")
    report["audio"] = {"path": str(args.audio), "sha256": sha256(args.audio),
                       "source_rate": source_rate, "samples_16khz": len(pcm)}
    command = ([str(args.engine_exe)] if args.engine_exe else
               [str(ROOT / ".venv/Scripts/python.exe"), str(RESOURCES / "engine.py")])
    command += ["serve", "--engine", args.engine]
    report["command"] = command
    paths = [Path(command[0]), Path(__file__), RESOURCES / "engine.py",
             RESOURCES / "server_common.py", RESOURCES / "r2t2_stream.py",
             RESOURCES / "reuse_enhancement.py",
             RESOURCES / ("qwen3_asr_server.py" if args.engine.startswith("qwen") else "r2t2_asr_server.py")]
    report["file_sha256"] = {str(path): sha256(path) for path in paths}
    report["timing_note"] = "Unpaced capture: audio_captured_ms and accumulated request_ms are separate; no measured microphone latency."
    stderr_path, stdout_path = args.output.with_suffix(".stderr.log"), args.output.with_suffix(".stdout.log")
    report.update(stderr_log=str(stderr_path), stdout_log=str(stdout_path), requests=[], sessions=[])
    with tempfile.TemporaryDirectory(prefix="light-whisper-reuse-smoke-") as data_dir, \
            stderr_path.open("w", encoding="utf-8") as stderr_log, \
            stdout_path.open("w", encoding="utf-8") as stdout_log:
        environment = os.environ.copy()
        environment.update(LIGHT_WHISPER_AUDIO_PROCESSING="1", LIGHT_WHISPER_DATA_DIR=data_dir,
                           LIGHT_WHISPER_GPU_IDLE_SECONDS="0", OMP_NUM_THREADS="2", PYTHONUNBUFFERED="1",
                           PYTHONIOENCODING="utf-8")
        environment.setdefault("HF_HUB_CACHE", str(Path.home() / ".cache/huggingface/hub"))
        report["isolated_data_dir"] = data_dir
        child = subprocess.Popen(command, cwd=ROOT, env=environment, stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=stderr_log, text=True, encoding="utf-8",
                                 errors="replace", bufsize=1,
                                 creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        report["child_pid"] = child.pid
        messages = queue.Queue()

        def read_stdout():
            try:
                for line in child.stdout:
                    stdout_log.write(line)
                    stdout_log.flush()
                    messages.put(line)
            finally:
                messages.put(None)

        reader = threading.Thread(target=read_stdout, daemon=True)
        reader.start()

        def receive(timeout):
            deadline = time.monotonic() + timeout
            while True:
                try:
                    line = messages.get(timeout=max(0, deadline - time.monotonic()))
                except queue.Empty as error:
                    raise TimeoutError(f"server response exceeded {timeout:.2f}s") from error
                if line is None:
                    raise RuntimeError(f"server stdout closed; child exit={child.poll()}")
                try:
                    response = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if isinstance(response, dict) and "success" in response:
                    return response

        def request(action, *, timeout=60, **fields):
            request_id = len(report["requests"]) + 1
            entry = {"request_id": request_id, "action": action}
            report["requests"].append(entry)
            started = time.perf_counter()
            try:
                child.stdin.write(json.dumps({"request_id": request_id, "action": action, **fields}) + "\n")
                child.stdin.flush()
                response = receive(timeout)
                entry["response"] = response
                require(response.get("request_id") == request_id, f"response request_id mismatch for {action}")
                require(response.get("success") is True, f"{action} failed: {response}")
                return response, (time.perf_counter() - started) * 1000
            finally:
                entry["elapsed_ms"] = (time.perf_counter() - started) * 1000

        def capture(session_id, label):
            session = {"label": label, "session_id": session_id, "feed_ms": [], "first_available": None}
            report["sessions"].append(session)
            started, session["start_ms"] = request("stream_start", session_id=session_id)
            require(started.get("session_id") == session_id and started.get("sample_count") == 0,
                    "stream_start identity/count mismatch")
            processing_ms = session["start_ms"]
            previous_text = ""

            def observe(response, accepted, stage):
                nonlocal previous_text
                require(response.get("session_id") == session_id and response.get("sample_count") == accepted,
                        f"{label}/{stage}: accepted raw count or session mismatch")
                text, tentative = response.get("text", ""), response.get("tentative_text", "")
                require(isinstance(text, str) and text.startswith(previous_text), "committed text regressed")
                previous_text = text
                if session["first_available"] is None and (text.strip() or tentative.strip()):
                    session["first_available"] = {"stage": stage, "audio_captured_ms": accepted / 16,
                                                  "accumulated_request_ms": processing_ms,
                                                  "text": text, "tentative_text": tentative}

            for offset in range(0, len(pcm), 640):
                chunk = pcm[offset:offset + 640]
                response, elapsed = request("stream_feed", session_id=session_id, offset=offset,
                                            sample_rate=16000, audio_format="pcm_s16le",
                                            audio_base64=base64.b64encode(chunk.tobytes()).decode("ascii"))
                session["feed_ms"].append(elapsed)
                processing_ms += elapsed
                observe(response, offset + len(chunk), "feed")
                require(response.get("final") is False, "feed unexpectedly final")
            final, session["finish_ms"] = request("stream_finish", session_id=session_id)
            processing_ms += session["finish_ms"]
            observe(final, len(pcm), "finish")
            require(final.get("final") is True and final.get("text", "").strip(), "final transcript is empty")
            session.update(final=final, total_request_ms=processing_ms, sample_count=len(pcm))

        try:
            started = time.perf_counter()
            report["initialization"] = receive(120)
            report["initialization_ms"] = (time.perf_counter() - started) * 1000
            require(report["initialization"].get("success") is True and
                    report["initialization"].get("model_loaded") is True, "initialization did not load the model")
            capture(1, "processing_on")
            disabled, _ = request("set_audio_processing", enabled=False)
            require(disabled.get("audio_processing_enabled") is False, "disable not acknowledged")
            capture(2, "processing_off")
            enabled, _ = request("set_audio_processing", enabled=True)
            require(enabled.get("audio_processing_enabled") is True, "enable not acknowledged")
            idle, _ = request("set_gpu_idle", seconds=1)
            require(idle.get("gpu_idle_seconds") == 1, "idle policy not acknowledged")
            deadline = time.monotonic() + 6
            report["idle_statuses"] = []
            while time.monotonic() < deadline:
                status, _ = request("status", timeout=max(0.01, deadline - time.monotonic()))
                report["idle_statuses"].append(status)
                if status.get("gpu_suspended") is True and status.get("model_loaded") is False:
                    break
                time.sleep(min(0.25, max(0, deadline - time.monotonic())))
            else:
                raise TimeoutError("runtime did not actually unload within 6 seconds")
            capture(3, "processing_on_after_resume")
            report["resumed_status"], _ = request("status")
            require(report["resumed_status"].get("model_loaded") is True and
                    report["resumed_status"].get("initialized") is True, "runtime did not resume")
            request("exit")
            report["child_exit_code"] = child.wait(timeout=20)
            require(child.returncode == 0, f"child exited with {child.returncode}")
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=10)
                report["child_killed_after_failure"] = True
            report["child_exit_code"] = child.returncode
            reader.join(timeout=2)
            child.stdin.close()
            child.stdout.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", required=True, choices=("qwen3-asr-0.6b", "confucius4-r2t2"))
    parser.add_argument("--engine-exe", type=Path, help="bundled engine.exe; default uses source .venv Python")
    parser.add_argument("--audio", type=Path, default=ROOT / "tmp/reuse-investigation/mic_test2.wav")
    parser.add_argument("--output", type=Path, required=True, help="JSON receipt; sibling stdout/stderr logs")
    args = parser.parse_args()
    args.audio, args.output = args.audio.resolve(), args.output.resolve()
    args.engine_exe = args.engine_exe.resolve() if args.engine_exe else None
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {"status": "running", "engine": args.engine}
    try:
        run(args, report)
        report["status"] = "pass"
    except (Exception, KeyboardInterrupt) as error:
        report.update(status="fail", error=f"{type(error).__name__}: {error}")
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "output": str(args.output), "error": report.get("error")}))
    return int(report["status"] != "pass")


if __name__ == "__main__":
    raise SystemExit(main())
